// parity record: every corpus case (corpus.ts) run with the oracle, time and seed pinned, at thread
// counts 1, 2, 8 and the machine's concurrency, and with the engine staged from this checkout five
// times, each as a client runs it (engineEnv.ts), all in one cases folder so the paths in the jobs
// and the outputs are the same bytes. Each run of a pass is kept as same.ts keeps it
// (<work>/parity/runs/record/<pass>/<case>/<n>/), with pass.json saying what made it, so a pass whose
// engine, knobs and corpus have not changed is not run again. Then every output gets its class
// (classes.ts); the oracle's pass at one thread is kept as the goldens (<work>/parity/goldens, whose
// goldens.json names the oracle: its program's SHA-256 and the cli/ it was built from); the
// front end's goldens are written into the repository (parity/goldens/front/); and parity.json lists
// every family (corpus.ts FAMILIES) with the classes of the outputs of its cases, certified false.
// The oracle's passes run with SKETCH_ORCA_ORACLE_DUMP set to a fresh folder per engine run
// (neutral.ts knobRunner) and keep the dumps of the boundaries in KEPT_DUMPS, each beside its run as
// <pass>/<case>/<n>/dumps/<file> and named in the run's record (RecordedRun.dumps); every other dump
// (the G-code export's, phase O4's) is removed as soon as its run ends. KEPT_DUMPS is every boundary
// of the front end and of Print::process() that domain A reaches, in the order process() writes
// them: front-objects (phase O2), front-config, each PrintObject step the oracle dumps
// (object<i>-<step>-<n>.sop1, one boundary per step for every object i: dumpBoundary), the tool
// ordering and the skirt and brim (print-psWipeTower, print-psSkirtBrim), and the end of process(),
// print-psConflictCheck (oracle patch 0007). Each kept dump gets a class across the thread counts
// (classifyDumps: B when it is the same at every count, its RUN record left out, T when not), which
// goldens.json lists (GoldensIndex.dumps), and the goldens keep the one-thread pass's dumps with its
// outputs. When the goldens would pass GOLDENS_MAX_BYTES (8 GiB), each dump of a run outside domain A
// (a case with a family of corpus.ts MARKER_FAMILIES) that would take them past it is kept as its
// SHA-256 instead (<file>.sha256, the hash of its portable bytes; GoldensIndex.hashedDumps), which
// certify compares by hash; the runs of domain A, which the replays read, keep their bytes. Goldens
// recorded before dumps were kept have no `dumps`, and certify reads them as before.

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { copyTree, nativePath, sha256Bytes } from '../files.ts'
import type { OrcaLock } from '../lock.ts'
import type { ParityPaths } from '../paths.ts'
import type { Platform } from '../env.ts'
import { StepError } from '../run.ts'
import { SMOKE_VERSION, type SmokeCase, type SmokePresetSets } from '../smoke.ts'
import { classesOf, classify, portableBytes, type OutputClass } from './classes.ts'
import { corpusFile, MARKER_FAMILIES, readCorpus, type CorpusDocument } from './corpus.ts'
import type { Mask } from './masks.ts'
import { knobRunner, PINNED_KNOBS, portableDump, readSop1, sop1Differences } from './neutral.ts'
import { ORACLE_KNOBS, type OracleUnderTest } from './oracle.ts'
import { runEngineCases, runHidden, runStore, type CaseRecord, type EngineUnderTest, type RunProcess } from './same.ts'

export const RECORD_FORMAT = 'sketch-orca-parity-record/1'
export const PARITY_FORMAT = 'sketch-orca-parity/1'
export const GOLDENS_FORMAT = 'sketch-orca-parity-goldens/1'

/** How many times the engine runs each case. */
export const RECORD_ENGINE_RUNS = 5

/** The thread counts the oracle runs at: 1, 2, 8 and the machine's concurrency, each once. */
export function recordThreads(concurrency: number): number[] {
  return [...new Set([1, 2, 8, Math.max(1, Math.floor(concurrency))])].sort((a, b) => a - b)
}

export interface Pass {
  /** oracle-p<P> or engine-<n>. */
  label: string
  /** The oracle's knobs, none for the engine. */
  knobs: Record<string, string>
  cases: CaseRecord[]
}

/** Where parity record works: the cases' folder, a store per pass, the report. `base` is runs/record, or runs/record-only for --only. */
export function recordFiles(paths: Pick<ParityPaths, 'parity' | 'runs' | 'goldens'>, base = 'record') {
  const dir = join(paths.runs, base)
  return {
    dir,
    root: join(dir, 'cases'),
    store: (label: string) => join(dir, label),
    report: join(paths.parity, base === 'record' ? 'record.json' : `${base}.json`),
    goldens: paths.goldens,
  }
}

export function oracleLabel(p: number): string {
  return `oracle-p${p}`
}

export function engineLabel(n: number): string {
  return `engine-${n}`
}

/**
 * What makes a pass: the program, its knobs, the corpus, the smoke cases' version and the cases'
 * folder, and for a pass that keeps dumps the boundaries it keeps (so a pass kept from before
 * dumps were kept, or before a boundary was added, runs again).
 */
export function passKey(o: { exeSha256: string; knobs: Record<string, string>; corpusSha256: string; caseIds: string[]; root: string; dumps?: readonly string[] }): string {
  const parts: unknown[] = [o.exeSha256, Object.entries(o.knobs).sort(), o.corpusSha256, SMOKE_VERSION, o.caseIds, o.root]
  if (o.dumps && o.dumps.length > 0) parts.push(['dumps', ...o.dumps])
  return sha256Bytes(JSON.stringify(parts))
}

// ---------------------------------------------------------------- the dumps a pass keeps

/**
 * The PrintObject steps whose dumps domain A reaches in Print::process(), in the order it runs them
 * (Print.cpp process: slicing and walls, the curled extrusions, the infill, ironing, Z contouring,
 * the supports, the overhangs for lift, then, after the tool ordering and the skirt and brim, the
 * three simplifications). posEstimateCurledExtrusions is dumped only for an object that shares
 * another's layers (PrintObject::estimate_curled_extrusions never sets its step done; the curled
 * lines are in the next dump's LEXT records), and posSimplifyWall never, so it is not one.
 */
export const OBJECT_STEPS = [
  'posSlice',
  'posPerimeters',
  'posEstimateCurledExtrusions',
  'posPrepareInfill',
  'posInfill',
  'posIroning',
  'posContouring',
  'posSupportMaterial',
  'posDetectOverhangsForLift',
  'posSimplifyPath',
  'posSimplifyInfill',
  'posSimplifySupportPath',
] as const

const objectBoundary = (step: string) => `object-${step}`

/**
 * The boundaries whose dumps parity record keeps of the oracle's passes and certify compares, by
 * the name part of the dump's file (<boundary>-<n>.sop1, parity/SOP1.md "Files"; an object's
 * object<i>-<step>-<n>.sop1 is the boundary object-<step>), in the order Print::process() writes
 * them: front-objects, the model once the objects are placed (phase O2); front-config, the configs
 * once applied, before process(); every object step of OBJECT_STEPS up to the overhangs for lift;
 * the tool ordering (print-psWipeTower) and the skirt and brim (print-psSkirtBrim); the three
 * simplifications; and the end of process(), print-psConflictCheck, which oracle patch 0007 writes
 * after the conflict check. The G-code export's dump (print-psGCodeExport) is phase O4's.
 */
export const KEPT_DUMPS: readonly string[] = [
  'front-objects',
  'front-config',
  ...OBJECT_STEPS.slice(0, OBJECT_STEPS.indexOf('posDetectOverhangsForLift') + 1).map(objectBoundary),
  'print-psWipeTower',
  'print-psSkirtBrim',
  ...OBJECT_STEPS.slice(OBJECT_STEPS.indexOf('posSimplifyPath')).map(objectBoundary),
  'print-psConflictCheck',
]

/**
 * The boundary a dump's file is of, or undefined for a file that is no dump: front-objects for
 * front-objects-1.sop1, print-psSkirtBrim for print-psSkirtBrim-1.sop1, and object-<step> for
 * object<i>-<step>-<n>.sop1, that step's boundary for object i (dumpObject gives i).
 */
export function dumpBoundary(file: string): string | undefined {
  const o = /^object(\d+)-(.+)-(\d+)\.sop1$/.exec(file)
  if (o) return objectBoundary(o[2]!)
  const m = /^(.+)-(\d+)\.sop1$/.exec(file)
  return m ? m[1] : undefined
}

/** The object an object dump's file is of (2 for object2-posSlice-1.sop1), or undefined for another file. */
export function dumpObject(file: string): number | undefined {
  const o = /^object(\d+)-(.+)-(\d+)\.sop1$/.exec(file)
  return o ? Number(o[1]) : undefined
}

/** Whether a dump's file is of a boundary in `kept`. */
export function keptDump(file: string, kept: readonly string[] = KEPT_DUMPS): boolean {
  const b = dumpBoundary(file)
  return b !== undefined && kept.includes(b)
}

/** The dumps folder of one pass's engine runs: <dir>/<label>-dumps (knobRunner's <case>/<n>/ under it). */
export function dumpsRoot(dir: string, label: string): string {
  return join(dir, `${label}-dumps`)
}

/**
 * Moves the kept dumps of every run (knobRunner's <dumps>/<case>/<n>/) beside the run in the store
 * (<store>/<case>/<n>/dumps/) and names them in the run's record, sorted; then removes `dumps`.
 * Every run gets `dumps`, empty when it wrote none.
 */
export function collectDumps(dumps: string, store: string, cases: CaseRecord[], kept: readonly string[] = KEPT_DUMPS): void {
  for (const c of cases) {
    for (const r of c.runs) {
      const from = join(dumps, c.id, String(r.index))
      const names = existsSync(from) ? readdirSync(from).filter((f) => keptDump(f, kept)).sort() : []
      const to = runStore(join(store, c.id), r.index).dumps
      rmSync(to, { recursive: true, force: true })
      if (names.length > 0) mkdirSync(to, { recursive: true })
      for (const f of names) renameSync(join(from, f), join(to, f))
      r.dumps = names
    }
  }
  rmSync(dumps, { recursive: true, force: true })
}

/** Every dump of a case's run `index` across passes, sorted. */
export function dumpNames(runs: (CaseRecord['runs'][number] | undefined)[]): string[] {
  return [...new Set(runs.flatMap((r) => r?.dumps ?? []))].sort()
}

/**
 * The bytes of a kept dump of one run in a pass's store, or undefined when the run did not keep it
 * or the goldens keep it as its SHA-256 alone (dumpSha256).
 */
export function dumpBytes(store: string, caseId: string, run: CaseRecord['runs'][number] | undefined, file: string): Buffer | undefined {
  if (!run || !(run.dumps ?? []).includes(file)) return undefined
  const path = join(runStore(join(store, caseId), run.index).dumps, file)
  if (!existsSync(path) && existsSync(`${path}${HASHED_DUMP}`)) return undefined
  return readFileSync(path)
}

/** What a dump kept as its SHA-256 is written as, beside where its bytes would be: <file>.sha256. */
export const HASHED_DUMP = '.sha256'

/**
 * The SHA-256 (hex) of the portable bytes of a dump the goldens keep as its hash alone
 * (keepGoldens, past GOLDENS_MAX_BYTES), or undefined when the run did not keep it or keeps its
 * bytes.
 */
export function dumpSha256(store: string, caseId: string, run: CaseRecord['runs'][number] | undefined, file: string): string | undefined {
  if (!run || !(run.dumps ?? []).includes(file)) return undefined
  const path = `${join(runStore(join(store, caseId), run.index).dumps, file)}${HASHED_DUMP}`
  return existsSync(path) ? readFileSync(path, 'utf8').trim() : undefined
}

/** A kept dump with its side's paths portable (neutral.ts portableDump, portableRoots). */
export function portableDumpBytes(store: string, caseId: string, run: CaseRecord['runs'][number] | undefined, file: string, roots: { root: string; resources: string; work: string }): Buffer | undefined {
  return portableDump(dumpBytes(store, caseId, run, file), portableRoots({ root: roots.root, caseId, resources: roots.resources, work: roots.work }))
}

/** A kept dump's class across the oracle's thread counts, as goldens.json lists it. */
export interface DumpClass {
  case: string
  run: number
  dump: string
  /** B: the same at every thread count, its RUN record left out; T: not. */
  class: 'B' | 'T'
  detail?: string
}

/**
 * Each kept dump of the oracle's passes against the first pass's (the one-thread pass): B when every
 * pass wrote it and its records are the same, the RUN record left out (it names the thread count),
 * T when a pass did not write it or a record differs. The passes ran in one cases' folder, so no
 * path differs between them.
 */
export function classifyDumps(passes: Pass[], stores: (label: string) => string): DumpClass[] {
  const oracle = passes.filter((p) => p.label.startsWith('oracle-'))
  const first = oracle[0]
  if (!first) return []
  const out: DumpClass[] = []
  for (const c of first.cases) {
    const byPass = (p: Pass) => p.cases.find((x) => x.id === c.id)
    const runCount = Math.max(...oracle.map((p) => byPass(p)?.runs.length ?? 0))
    for (let i = 0; i < runCount; i++) {
      const runs = oracle.map((p) => byPass(p)?.runs[i])
      for (const dump of dumpNames(runs)) {
        const bytes = oracle.map((p) => dumpBytes(stores(p.label), c.id, byPass(p)?.runs[i], dump))
        const missing = oracle.filter((_, k) => bytes[k] === undefined).map((p) => p.label)
        let detail: string | undefined
        if (missing.length > 0) detail = `not written by ${missing.join(', ')}`
        for (let k = 1; k < oracle.length && detail === undefined; k++) {
          try {
            const d = sop1Differences(readSop1(bytes[0]!), readSop1(bytes[k]!))
            if (d.length > 0) detail = `${oracle[k]!.label} differs from ${first.label}: ${d.slice(0, 3).join('; ')}`
          } catch (err) {
            detail = `unreadable: ${(err as Error).message}`
          }
        }
        out.push({ case: c.id, run: i, dump, class: detail === undefined ? 'B' : 'T', ...(detail === undefined ? {} : { detail }) })
      }
    }
  }
  return out
}

interface PassFile {
  key: string
  label: string
  knobs: Record<string, string>
  cases: CaseRecord[]
}

function readPass(store: string, key: string): PassFile | undefined {
  const f = join(store, 'pass.json')
  if (!existsSync(f)) return undefined
  try {
    const p = JSON.parse(readFileSync(f, 'utf8')) as PassFile
    return p.key === key ? p : undefined
  } catch {
    return undefined
  }
}

export interface RecordOptions {
  doc: CorpusDocument
  corpusSha256: string
  cases: SmokeCase[]
  oracle: OracleUnderTest
  engine: EngineUnderTest
  engineExeSha256: string
  threads: number[]
  engineRuns: number
  files: ReturnType<typeof recordFiles>
  sets: SmokePresetSets
  lock: OrcaLock
  orcaSource?: string
  baseEnv: NodeJS.ProcessEnv
  platform: Platform
  timeoutMs: number
  run?: RunProcess
  /** The boundaries whose dumps the oracle's passes keep; KEPT_DUMPS when not given. */
  keptDumps?: readonly string[]
  log: (msg: string) => void
}

/** Runs (or reuses) every pass, oracle first, and returns them in that order. */
export async function runPasses(o: RecordOptions): Promise<Pass[]> {
  const base = o.run ?? runHidden
  const caseIds = o.cases.map((c) => c.id)
  const kept = o.keptDumps ?? KEPT_DUMPS
  const plan: { label: string; knobs: Record<string, string>; exe: { label: string; exe: string; resources: string; libraryPath: string }; exeSha256: string; dumps: readonly string[] }[] = [
    ...o.threads.map((p) => ({
      label: oracleLabel(p),
      knobs: { ...PINNED_KNOBS, [ORACLE_KNOBS.threads]: String(p) },
      exe: { label: oracleLabel(p), exe: o.oracle.exe, resources: o.oracle.resources, libraryPath: o.oracle.libraryPath },
      exeSha256: o.oracle.manifest.exe.sha256,
      dumps: kept,
    })),
    ...Array.from({ length: o.engineRuns }, (_, i) => ({
      label: engineLabel(i + 1),
      knobs: {},
      exe: { label: engineLabel(i + 1), exe: o.engine.exe, resources: o.engine.resources, libraryPath: o.engine.libraryPath },
      exeSha256: o.engineExeSha256,
      dumps: [] as readonly string[],
    })),
  ]
  const out: Pass[] = []
  for (const step of plan) {
    const store = o.files.store(step.label)
    const key = passKey({ exeSha256: step.exeSha256, knobs: step.knobs, corpusSha256: o.corpusSha256, caseIds, root: o.files.root, dumps: step.dumps })
    const kept = readPass(store, key)
    if (kept) {
      o.log(`${step.label}: kept from an earlier run (the same program, knobs and corpus)`)
      out.push({ label: step.label, knobs: step.knobs, cases: kept.cases })
      continue
    }
    o.log(`${step.label}: ${o.cases.length} cases${Object.keys(step.knobs).length > 0 ? ` with ${Object.entries(step.knobs).map(([k, v]) => `${k}=${v}`).join(' ')}` : ''}${step.dumps.length > 0 ? `, keeping the dumps of ${step.dumps.join(', ')}` : ''}`)
    const dumps = step.dumps.length > 0 ? dumpsRoot(o.files.dir, step.label) : undefined
    if (dumps !== undefined) rmSync(dumps, { recursive: true, force: true })
    const cases = await runEngineCases(step.exe, {
      root: o.files.root,
      store,
      cases: o.cases,
      sets: o.sets,
      lock: o.lock,
      ...(o.orcaSource ? { orcaSource: o.orcaSource } : {}),
      baseEnv: o.baseEnv,
      platform: o.platform,
      timeoutMs: o.timeoutMs,
      run: dumps !== undefined ? knobRunner(step.knobs, dumps, base, (f) => keptDump(f, step.dumps)) : Object.keys(step.knobs).length > 0 ? knobRunner(step.knobs, undefined, base) : base,
      log: o.log,
    })
    if (dumps !== undefined) collectDumps(dumps, store, cases, step.dumps)
    const file: PassFile = { key, label: step.label, knobs: step.knobs, cases }
    writeFileSync(join(store, 'pass.json'), JSON.stringify(file, null, 1) + '\n')
    out.push({ label: step.label, knobs: step.knobs, cases })
  }
  return out
}

// ---------------------------------------------------------------- outputs and their classes

/** The bytes of one output of one run in a pass's store: exit, stdout, stderr, files/<p>, removed/<p>. */
export function outputBytes(store: string, caseId: string, run: CaseRecord['runs'][number] | undefined, name: string): Buffer | undefined {
  if (!run) return undefined
  const s = runStore(join(store, caseId), run.index)
  if (name === 'exit') return Buffer.from(run.timedOut ? 'timed out' : String(run.code))
  if (name === 'stdout' || name === 'stderr') {
    const p = name === 'stdout' ? s.stdout : s.stderr
    return existsSync(p) ? readFileSync(p) : Buffer.alloc(0)
  }
  if (name.startsWith('files/')) {
    const rel = name.slice(6)
    return run.files.includes(rel) ? readFileSync(nativePath(s.files, rel)) : undefined
  }
  if (name.startsWith('removed/')) return run.gone.includes(name.slice(8)) ? Buffer.from('removed') : undefined
  return undefined
}

/** Every output name of a case's run `index` across passes, in a fixed order. */
export function outputNames(runs: (CaseRecord['runs'][number] | undefined)[]): string[] {
  const files = new Set<string>()
  const gone = new Set<string>()
  for (const r of runs) {
    for (const f of r?.files ?? []) files.add(f)
    for (const g of r?.gone ?? []) gone.add(g)
  }
  return ['exit', 'stdout', 'stderr', ...[...files].sort().map((f) => `files/${f}`), ...[...gone].sort().map((g) => `removed/${g}`)]
}

/** The name compareOutput's masks see for an output: its file's path, or the stream's name. */
export function maskName(name: string): string {
  if (name.startsWith('files/')) return name.slice(6)
  return name
}

export interface OutputRecord {
  case: string
  run: number
  output: string
  class: OutputClass
  flags: OutputClass[]
  details: Partial<Record<OutputClass, string>>
}

export interface RecordReport {
  format: typeof RECORD_FORMAT
  createdAt: string
  corpusSha256: string
  smokeVersion: number
  threads: number[]
  engineRuns: number
  passes: { label: string; knobs: Record<string, string>; failed: string[] }[]
  summary: Record<OutputClass, number> & { outputs: number }
  outputs: OutputRecord[]
}

/** Classifies every output of every case (classes.ts), the oracle passes first in `passes`. */
export function classifyPasses(passes: Pass[], stores: (label: string) => string, masks: Mask[]): OutputRecord[] {
  const oracle = passes.filter((p) => p.label.startsWith('oracle-'))
  const engine = passes.filter((p) => p.label.startsWith('engine-'))
  const ids = passes[0]?.cases.map((c) => c.id) ?? []
  const out: OutputRecord[] = []
  for (const id of ids) {
    const byPass = (p: Pass) => p.cases.find((c) => c.id === id)
    const runCount = Math.max(...passes.map((p) => byPass(p)?.runs.length ?? 0))
    for (let i = 0; i < runCount; i++) {
      const runs = passes.map((p) => byPass(p)?.runs[i])
      for (const name of outputNames(runs)) {
        const sides = {
          name: maskName(name),
          oracle: oracle.map((p) => outputBytes(stores(p.label), id, byPass(p)?.runs[i], name)),
          engine: engine.map((p) => outputBytes(stores(p.label), id, byPass(p)?.runs[i], name)),
        }
        const c = classify(sides, masks)
        out.push({ case: id, run: i, output: name, class: c.class, flags: c.flags, details: c.details })
      }
    }
  }
  return out
}

export function recordSummary(outputs: OutputRecord[]): RecordReport['summary'] {
  const s = { outputs: outputs.length, H: 0, R: 0, T: 0, N: 0, E: 0, B: 0 }
  for (const o of outputs) s[o.class]++
  return s
}

// ---------------------------------------------------------------- parity.json

export interface FamilyEntry {
  name: string
  step: string
  what: string
  /** The corpus cases that exercise it. */
  cases: string[]
  /** The classes of their outputs, H first. */
  classes: OutputClass[]
  certified: boolean
}

export interface HSite {
  case: string
  run: number
  output: string
  detail: string
}

export interface ParityDocument {
  format: typeof PARITY_FORMAT
  corpus: { file: string; sha256: string }
  threads: number[]
  engineRuns: number
  families: FamilyEntry[]
  /** Outputs that differ between two work folders (parity goldens --check): each wants an oracle H-patch. */
  hSites: HSite[]
}

/** parity/parity.json in the repository. */
export function parityFile(engineDir: string): string {
  return join(engineDir, 'parity', 'parity.json')
}

/**
 * parity.json from the classified outputs: each family with the cases that exercise it and the
 * classes of their outputs. Certification is the families' to earn later (parity certify), so a
 * family that the previous parity.json certified keeps that, and every other one is false.
 */
export function parityDocument(o: { doc: CorpusDocument; corpusSha256: string; threads: number[]; engineRuns: number; outputs: OutputRecord[]; hSites: HSite[]; previous?: ParityDocument }): ParityDocument {
  const byCase = new Map<string, OutputClass[]>()
  for (const r of o.outputs) byCase.set(r.case, [...(byCase.get(r.case) ?? []), r.class])
  const certified = new Set((o.previous?.families ?? []).filter((f) => f.certified).map((f) => f.name))
  const families = o.doc.families.map((f): FamilyEntry => {
    const cases = o.doc.cases.filter((c) => c.families.includes(f.name)).map((c) => c.id)
    return { name: f.name, step: f.step, what: f.what, cases, classes: classesOf(cases.flatMap((c) => byCase.get(c) ?? [])), certified: certified.has(f.name) }
  })
  return { format: PARITY_FORMAT, corpus: { file: 'parity/corpus.json', sha256: o.corpusSha256 }, threads: o.threads, engineRuns: o.engineRuns, families, hSites: o.hSites }
}

export function readParity(path: string): ParityDocument | undefined {
  if (!existsSync(path)) return undefined
  const p = JSON.parse(readFileSync(path, 'utf8')) as ParityDocument
  if (p.format !== PARITY_FORMAT) throw new StepError(`${path} is ${JSON.stringify(p.format)}, not ${PARITY_FORMAT}`)
  return p
}

export function writeJsonAtomic(path: string, value: unknown): void {
  mkdirSync(join(path, '..'), { recursive: true })
  const tmp = `${path}.tmp`
  writeFileSync(tmp, JSON.stringify(value, null, 2) + '\n')
  renameSync(tmp, path)
}

// ---------------------------------------------------------------- goldens

export interface GoldensIndex {
  format: typeof GOLDENS_FORMAT
  /** The pass kept: the oracle at one thread, time and seed pinned. */
  pass: string
  knobs: Record<string, string>
  corpusSha256: string
  /** The cases' folder the outputs' paths name, and the oracle's resources. */
  root: string
  resources: string
  work: string
  /**
   * The oracle the goldens were recorded with: its program's SHA-256 and the cli/ it was built
   * from (oracle.json). Goldens recorded before the index named it have none, and certify refuses
   * them (certify.ts oracleChainProblems).
   */
  oracle?: GoldensOracle
  /**
   * The class of each dump the goldens keep (classifyDumps), across the oracle's thread counts.
   * Goldens recorded before dumps were kept have none, and certify compares no dumps against them.
   */
  dumps?: DumpClass[]
  /**
   * The dumps kept as their SHA-256 alone (<case>/<run>/<file>), each in its run's dumps/ as
   * <file>.sha256: those of runs outside domain A that would have taken the goldens past
   * GOLDENS_MAX_BYTES (hashedDumps). None when the goldens fit.
   */
  hashedDumps?: string[]
  cases: CaseRecord[]
}

/** The most the goldens hold before a dump of a run outside domain A is kept as its SHA-256 alone: 8 GiB. */
export const GOLDENS_MAX_BYTES = 8 * 1024 ** 3

/** The bytes of every file under `dir` (none when it does not exist). */
function treeBytes(dir: string): number {
  if (!existsSync(dir)) return 0
  let total = 0
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    total += e.isDirectory() ? treeBytes(p) : statSync(p).size
  }
  return total
}

/**
 * The dumps of a pass's runs that the goldens keep as their SHA-256 alone (see the top of this
 * file), as { case, run, dump }: none when the pass's store (every output and dump) fits in
 * `maxBytes`. Otherwise the outputs and the dumps of the runs of domain A count first, as they are
 * always kept whole; then each dump of a run outside domain A (`outsideDomainA` of its case), in the
 * pass's order of cases and runs and each run's dumps sorted, is kept whole while it fits and as its
 * hash once it would take the goldens past `maxBytes`.
 */
export function hashedDumps(o: { pass: Pass; store: string; maxBytes: number; outsideDomainA: (caseId: string) => boolean }): { case: string; run: number; dump: string }[] {
  if (treeBytes(o.store) <= o.maxBytes) return []
  const dumpSize = (c: CaseRecord, r: CaseRecord['runs'][number], d: string) => {
    const p = join(runStore(join(o.store, c.id), r.index).dumps, d)
    return existsSync(p) ? statSync(p).size : 0
  }
  let kept = 0
  const candidates: { case: string; run: number; dump: string; size: number }[] = []
  for (const c of o.pass.cases) {
    kept += treeBytes(join(o.store, c.id))
    if (!o.outsideDomainA(c.id)) continue
    for (const r of c.runs) {
      for (const d of [...(r.dumps ?? [])].sort()) {
        const size = dumpSize(c, r, d)
        kept -= size
        candidates.push({ case: c.id, run: r.index, dump: d, size })
      }
    }
  }
  const out: { case: string; run: number; dump: string }[] = []
  for (const x of candidates) {
    if (kept + x.size <= o.maxBytes) kept += x.size
    else out.push({ case: x.case, run: x.run, dump: x.dump })
  }
  return out
}

/** Whether a corpus case is outside domain A: one of its families is a marker (corpus.ts MARKER_FAMILIES). */
export function outsideDomainAOf(doc: Pick<CorpusDocument, 'cases'>): (caseId: string) => boolean {
  const outside = new Set(doc.cases.filter((c) => c.families.some((f) => MARKER_FAMILIES.includes(f))).map((c) => c.id))
  return (id) => outside.has(id)
}

export interface GoldensOracle {
  exeSha256: string
  /** files.ts hashTree of the cli/ the oracle was built from. */
  cli: string
}

/**
 * Keeps a pass's store as the goldens: <goldens>/cases/<case>/<n>/ (its dumps/ too) and
 * <goldens>/goldens.json. Past `maxBytes` (GOLDENS_MAX_BYTES), the dumps hashedDumps names are kept
 * as <file>.sha256, the SHA-256 of their portable bytes (portableDump with the goldens' roots), and
 * listed in GoldensIndex.hashedDumps; the pass's store keeps their bytes. `outsideDomainA` tells a
 * case outside domain A, by default from the repository's parity/corpus.json.
 */
export function keepGoldens(o: {
  pass: Pass
  store: string
  goldens: string
  corpusSha256: string
  root: string
  resources: string
  work: string
  oracle: GoldensOracle
  dumps?: DumpClass[]
  maxBytes?: number
  outsideDomainA?: (caseId: string) => boolean
}): GoldensIndex {
  rmSync(o.goldens, { recursive: true, force: true })
  const cases = join(o.goldens, 'cases')
  mkdirSync(cases, { recursive: true })
  for (const c of o.pass.cases) if (existsSync(join(o.store, c.id))) copyTree(join(o.store, c.id), join(cases, c.id), undefined, { link: true })
  const outside = o.outsideDomainA ?? ((id: string) => outsideDomainAOf(readCorpus(corpusFile(REPOSITORY)))(id))
  const hashed = hashedDumps({ pass: o.pass, store: o.store, maxBytes: o.maxBytes ?? GOLDENS_MAX_BYTES, outsideDomainA: outside })
  for (const h of hashed) {
    const run = o.pass.cases.find((c) => c.id === h.case)?.runs.find((r) => r.index === h.run)
    const bytes = portableDump(dumpBytes(o.store, h.case, run, h.dump), portableRoots({ root: o.root, caseId: h.case, resources: o.resources, work: o.work }))!
    const at = join(runStore(join(cases, h.case), h.run).dumps, h.dump)
    rmSync(at, { force: true })
    writeFileSync(`${at}${HASHED_DUMP}`, `${sha256Bytes(bytes)}\n`)
  }
  const index: GoldensIndex = {
    format: GOLDENS_FORMAT,
    pass: o.pass.label,
    knobs: o.pass.knobs,
    corpusSha256: o.corpusSha256,
    root: o.root,
    resources: o.resources,
    work: o.work,
    oracle: o.oracle,
    ...(o.dumps ? { dumps: o.dumps } : {}),
    ...(hashed.length > 0 ? { hashedDumps: hashed.map((h) => `${h.case}/${h.run}/${h.dump}`) } : {}),
    cases: o.pass.cases,
  }
  writeJsonAtomic(join(o.goldens, 'goldens.json'), index)
  return index
}

/** This checkout: scripts/lib/parity/ is three folders below it. */
const REPOSITORY = join(import.meta.dir, '..', '..', '..')

export function readGoldens(goldens: string): GoldensIndex {
  const f = join(goldens, 'goldens.json')
  if (!existsSync(f)) throw new StepError(`no goldens in ${goldens}; run parity record`)
  return JSON.parse(readFileSync(f, 'utf8')) as GoldensIndex
}

/** The roots a golden's paths are written with in the repository: the case's folder, the cases' folder, the resources, the work folder. */
export function portableRoots(o: { root: string; caseId: string; resources: string; work: string }): [string, string][] {
  return [
    [join(o.root, o.caseId), '<case>'],
    [o.root, '<cases>'],
    [o.resources, '<resources>'],
    [o.work, '<work>'],
  ]
}

/** parity/goldens/front in the repository. */
export function frontGoldensDir(engineDir: string): string {
  return join(engineDir, 'parity', 'goldens', 'front')
}

/**
 * The front end's goldens from the kept pass: version.json (the version command's stdout),
 * config-def.json (what config-def wrote), and intake/<case>.result.json for every example and
 * refusal case (its result.json, paths written as <case>, <cases>, <resources> and <work>).
 */
export function writeFrontGoldens(o: { doc: CorpusDocument; index: GoldensIndex; goldens: string; engineDir: string }): string[] {
  const dir = frontGoldensDir(o.engineDir)
  const store = join(o.goldens, 'cases')
  const written: string[] = []
  const find = (id: string) => o.index.cases.find((c) => c.id === id)
  const version = find('smoke-version')?.runs[0]
  const v = outputBytes(store, 'smoke-version', version, 'stdout')
  if (!version || version.code !== 0 || !v || v.length === 0) throw new StepError('the kept pass has no version output (smoke-version)')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'version.json'), v)
  written.push('version.json')
  const cd = find('smoke-config-def')?.runs[0]
  const def = outputBytes(store, 'smoke-config-def', cd, 'files/config-def.json')
  if (!def) throw new StepError('the kept pass has no config-def.json (smoke-config-def)')
  writeFileSync(join(dir, 'config-def.json'), def)
  written.push('config-def.json')
  const intake = join(dir, 'intake')
  rmSync(intake, { recursive: true, force: true })
  mkdirSync(intake, { recursive: true })
  for (const c of o.doc.cases.filter((x) => x.kind === 'example' || x.kind === 'refusal')) {
    const r = find(c.id)?.runs[0]
    const bytes = outputBytes(store, c.id, r, 'files/result.json')
    if (!bytes) throw new StepError(`the kept pass has no result.json for ${c.id}`)
    writeFileSync(join(intake, `${c.id}.result.json`), portableBytes(bytes, portableRoots({ root: o.index.root, caseId: c.id, resources: o.index.resources, work: o.index.work }))!)
    written.push(`intake/${c.id}.result.json`)
  }
  return written
}
