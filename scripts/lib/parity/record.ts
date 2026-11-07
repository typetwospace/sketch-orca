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

import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { copyTree, nativePath, sha256Bytes } from '../files.ts'
import type { OrcaLock } from '../lock.ts'
import type { ParityPaths } from '../paths.ts'
import type { Platform } from '../env.ts'
import { StepError } from '../run.ts'
import { SMOKE_VERSION, type SmokeCase, type SmokePresetSets } from '../smoke.ts'
import { classesOf, classify, portableBytes, type OutputClass } from './classes.ts'
import type { CorpusDocument } from './corpus.ts'
import type { Mask } from './masks.ts'
import { knobRunner, PINNED_KNOBS } from './neutral.ts'
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

/** What makes a pass: the program, its knobs, the corpus, the smoke cases' version and the cases' folder. */
export function passKey(o: { exeSha256: string; knobs: Record<string, string>; corpusSha256: string; caseIds: string[]; root: string }): string {
  return sha256Bytes(JSON.stringify([o.exeSha256, Object.entries(o.knobs).sort(), o.corpusSha256, SMOKE_VERSION, o.caseIds, o.root]))
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
  log: (msg: string) => void
}

/** Runs (or reuses) every pass, oracle first, and returns them in that order. */
export async function runPasses(o: RecordOptions): Promise<Pass[]> {
  const base = o.run ?? runHidden
  const caseIds = o.cases.map((c) => c.id)
  const plan: { label: string; knobs: Record<string, string>; exe: { label: string; exe: string; resources: string; libraryPath: string }; exeSha256: string }[] = [
    ...o.threads.map((p) => ({
      label: oracleLabel(p),
      knobs: { ...PINNED_KNOBS, [ORACLE_KNOBS.threads]: String(p) },
      exe: { label: oracleLabel(p), exe: o.oracle.exe, resources: o.oracle.resources, libraryPath: o.oracle.libraryPath },
      exeSha256: o.oracle.manifest.exe.sha256,
    })),
    ...Array.from({ length: o.engineRuns }, (_, i) => ({
      label: engineLabel(i + 1),
      knobs: {},
      exe: { label: engineLabel(i + 1), exe: o.engine.exe, resources: o.engine.resources, libraryPath: o.engine.libraryPath },
      exeSha256: o.engineExeSha256,
    })),
  ]
  const out: Pass[] = []
  for (const step of plan) {
    const store = o.files.store(step.label)
    const key = passKey({ exeSha256: step.exeSha256, knobs: step.knobs, corpusSha256: o.corpusSha256, caseIds, root: o.files.root })
    const kept = readPass(store, key)
    if (kept) {
      o.log(`${step.label}: kept from an earlier run (the same program, knobs and corpus)`)
      out.push({ label: step.label, knobs: step.knobs, cases: kept.cases })
      continue
    }
    o.log(`${step.label}: ${o.cases.length} cases${Object.keys(step.knobs).length > 0 ? ` with ${Object.entries(step.knobs).map(([k, v]) => `${k}=${v}`).join(' ')}` : ''}`)
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
      run: Object.keys(step.knobs).length > 0 ? knobRunner(step.knobs, undefined, base) : base,
      log: o.log,
    })
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
  cases: CaseRecord[]
}

export interface GoldensOracle {
  exeSha256: string
  /** files.ts hashTree of the cli/ the oracle was built from. */
  cli: string
}

/** Keeps a pass's store as the goldens: <goldens>/cases/<case>/<n>/ and <goldens>/goldens.json. */
export function keepGoldens(o: { pass: Pass; store: string; goldens: string; corpusSha256: string; root: string; resources: string; work: string; oracle: GoldensOracle }): GoldensIndex {
  rmSync(o.goldens, { recursive: true, force: true })
  const cases = join(o.goldens, 'cases')
  mkdirSync(cases, { recursive: true })
  for (const c of o.pass.cases) if (existsSync(join(o.store, c.id))) copyTree(join(o.store, c.id), join(cases, c.id), undefined, { link: true })
  const index: GoldensIndex = { format: GOLDENS_FORMAT, pass: o.pass.label, knobs: o.pass.knobs, corpusSha256: o.corpusSha256, root: o.root, resources: o.resources, work: o.work, oracle: o.oracle, cases: o.pass.cases }
  writeJsonAtomic(join(o.goldens, 'goldens.json'), index)
  return index
}

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
