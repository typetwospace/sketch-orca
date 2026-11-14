// The same-check: the published reference engine (parity reference) and the engine staged from
// this checkout (<work>/dist/engine, after build, smoke and stage) each run every smoke case
// (smoke.ts SMOKE_CASES) in the environment a client gives the engine (engineEnv.ts), and every
// output of every engine run is compared (masks.ts): identical, identical after the clock and
// entropy masks of parity/masks.json, or different. What differs is a candidate for the families
// of differences later work sorts out (run to run, thread count, folder, a hazard in Orca's code);
// the check itself only records it, in <work>/parity/same.json.
//
// Both engines run the cases in one folder, <work>/parity/runs/same/cases, one after the other, so
// the jobs, and the paths in them and in the results, are the same bytes. Before and after each
// engine run the case's folder is read: the files the run made, changed or removed are its
// outputs, kept under runs/same/<engine>/<case>/<n>/files/, with its stdout and stderr next to
// them. The folders named `work` are left out: they are the jobs' work folders, where Orca keeps
// temporary files named after the clock and its process id. The smoke checks run as well, and
// same.json says whether each engine passed them; a failed check is recorded, not a failure of
// the same-check. A pass may act on the case's folder once a run is recorded, before the case
// reads it (AfterRun): parity certify goes on past a run the Rust program declined with the
// golden C++ run's outputs (certify.ts goldenCarry); the other passes leave the folder as it is.

import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { listPatches } from '../checkout.ts'
import { fileEntry, hashTree, listFiles, nativePath, sha256Bytes, copyFile } from '../files.ts'
import type { OrcaLock } from '../lock.ts'
import type { ParityPaths } from '../paths.ts'
import { writePresetSet, type PresetFiles } from '../profiles.ts'
import { StepError } from '../run.ts'
import { SMOKE_PRESETS, SMOKE_VERSION, type EngineRun, type SmokeCase, type SmokePresetSets } from '../smoke.ts'
import { readManifest, STAGE_VERSION, verifyStaged, type EngineManifest } from '../stage.ts'
import type { Platform } from '../env.ts'
import { engineEnvProblems, parityEngineEnv, type EngineEnv } from './engineEnv.ts'
import { compareOutput, type Mask, type OutputStatus } from './masks.ts'

export const SAME_FORMAT = 'sketch-orca-parity-same/1'

export type EngineLabel = 'reference' | 'head'

export interface EngineUnderTest {
  label: EngineLabel
  dir: string
  manifest: EngineManifest
  exe: string
  resources: string
  libraryPath: string
}

/** A staged engine folder, checked against its manifest (every file, its size and SHA-256). */
export function engineUnderTest(label: EngineLabel, dir: string): EngineUnderTest {
  if (!existsSync(dir)) throw new StepError(`no ${label} engine at ${dir}`)
  const manifest = readManifest(dir)
  const problems = verifyStaged(dir, manifest)
  if (problems.length > 0) throw new StepError(`the ${label} engine in ${dir} is not what its manifest lists: ${problems.slice(0, 10).join('; ')}`)
  return { label, dir, manifest, exe: nativePath(dir, manifest.engine.exe), resources: nativePath(dir, manifest.engine.resources), libraryPath: nativePath(dir, manifest.engine.libraryPath) }
}

/**
 * Why a staged engine is not the one this checkout stages now: the stage version, a full smoke run
 * of the current cases, the locked commit, patches/ and cli/ as they are. Empty when it is.
 */
export function headEngineProblems(m: EngineManifest, want: { commit: string; patches: { name: string; sha256: string }[]; cli: string }): string[] {
  const out: string[] = []
  if (m.stageVersion !== STAGE_VERSION) out.push(`it is at stage version ${m.stageVersion}, this checkout stages ${STAGE_VERSION}`)
  if (!m.smoke.passed || m.smoke.smokeVersion !== SMOKE_VERSION) out.push(`its smoke run is ${m.smoke.passed ? '' : 'failed, '}version ${m.smoke.smokeVersion}, this checkout's is ${SMOKE_VERSION}`)
  if (m.orca.commit !== want.commit) out.push(`it is built from ${m.orca.commit}, the lock is at ${want.commit}`)
  const key = (p: { name: string; sha256: string }[]) => JSON.stringify(p.map((x) => [x.name, x.sha256]))
  if (key(m.patches) !== key(want.patches)) out.push(`its patches are ${m.patches.map((p) => p.name).join(', ')}, not patches/ as it is`)
  if (m.cli !== want.cli) out.push('it is built from another cli/')
  return out
}

/** headEngineProblems for this checkout (`engineDir`, the repository root). */
export function headEngineProblemsFor(m: EngineManifest, engine: { patches: string; cli: string }, lock: Pick<OrcaLock, 'commit'>): string[] {
  return headEngineProblems(m, { commit: lock.commit, patches: listPatches(engine.patches).map((p) => ({ name: p.name, sha256: p.sha256 })), cli: hashTree(engine.cli) })
}

// ---------------------------------------------------------------- recording engine runs

/** Each file of a case folder by its forward-slash path, with its size and SHA-256. */
export type Snapshot = Map<string, { size: number; sha256: string }>

/** Leaves out the jobs' work folders (`work`), where Orca keeps its temporary files. */
export function snapshotDir(dir: string): Snapshot {
  const out: Snapshot = new Map()
  for (const rel of listFiles(dir, (r, isDir) => !(isDir && basename(r) === 'work'))) {
    const e = fileEntry(dir, rel)
    out.set(rel, { size: e.size, sha256: e.sha256 })
  }
  return out
}

/** What changed between two snapshots: files made or changed, and files gone. */
export function snapshotChanges(before: Snapshot, after: Snapshot): { changed: string[]; gone: string[] } {
  const changed: string[] = []
  for (const [p, e] of after) {
    const b = before.get(p)
    if (!b || b.sha256 !== e.sha256 || b.size !== e.size) changed.push(p)
  }
  const gone = [...before.keys()].filter((p) => !after.has(p))
  return { changed: changed.sort(), gone: gone.sort() }
}

export interface ProcessOutcome {
  code: number | null
  timedOut: boolean
  stdout: Buffer
  stderr: Buffer
}

export type RunProcess = (exe: string, args: string[], o: { cwd: string; env: EngineEnv; timeoutMs: number }) => Promise<ProcessOutcome>

/** Runs a program hidden (no window), stdin closed, until it exits or the time is up (then it is ended). */
export const runHidden: RunProcess = (exe, args, o) =>
  new Promise((resolve, reject) => {
    const child = spawn(exe, args, { cwd: o.cwd, env: o.env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
    const out: Buffer[] = []
    const err: Buffer[] = []
    child.stdout.on('data', (d: Buffer) => out.push(d))
    child.stderr.on('data', (d: Buffer) => err.push(d))
    let timedOut = false
    const timer = setTimeout(() => {
      timedOut = true
      child.kill()
    }, o.timeoutMs)
    child.on('error', (e) => {
      clearTimeout(timer)
      reject(e)
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      resolve({ code, timedOut, stdout: Buffer.concat(out), stderr: Buffer.concat(err) })
    })
  })

export interface RecordedRun {
  index: number
  /** The command line, the cases' folder written as <cases>. */
  args: string[]
  /** Names of the variables the case added (the crash test hook). */
  extraEnv: string[]
  code: number | null
  timedOut: boolean
  /** Files the run made or changed, relative to the case's folder. */
  files: string[]
  /** Files the run removed. */
  gone: string[]
  /**
   * The oracle's dumps kept of the run (record.ts KEPT_DUMPS), file names in <store>/<n>/dumps/; a
   * run of a pass that keeps none (the engine's, or one recorded before dumps were kept) has none.
   */
  dumps?: string[]
  /**
   * parity certify's runs only: the program declined this run, and the case went on from the golden
   * C++ run's outputs, put in the case's folder in place of the decline's (AfterRun, certify.ts
   * goldenCarry). What is kept of the run here is still the decline's own.
   */
  carried?: boolean
}

/**
 * What a pass may do once a run is recorded, before the smoke case reads the case's folder (see the
 * top of this file). It gets the case, its folder, the run as recorded and the folder's files before
 * the run. What it returns is what the case is told the run gave, and the run is marked `carried`;
 * undefined leaves the folder and the run as they are.
 */
export type AfterRun = (o: { caseId: string; caseDir: string; run: RecordedRun; before: Snapshot }) => EngineRun | undefined

/** Where run `index` of a case keeps its outputs: <store>/<index>/{stdout,stderr,files/,dumps/}. */
export function runStore(store: string, index: number): { dir: string; stdout: string; stderr: string; files: string; dumps: string } {
  const dir = join(store, String(index))
  return { dir, stdout: join(dir, 'stdout'), stderr: join(dir, 'stderr'), files: join(dir, 'files'), dumps: join(dir, 'dumps') }
}

/** The command line as same.json gives it: the cases' folder as <cases>, so both engines' read alike. */
export function portableArgs(args: string[], root: string): string[] {
  return args.map((a) => a.split(root).join('<cases>'))
}

/**
 * The `exec` a smoke case gets: it runs the engine in the case's folder with `env` (plus what the
 * case adds), and keeps what the run wrote (see the top of this file) in `store`. `after`, when
 * given, acts on the folder once the run is kept (AfterRun, the case's id bound).
 */
export function recordingExec(o: {
  exe: string
  caseDir: string
  root: string
  store: string
  env: EngineEnv
  timeoutMs: number
  runs: RecordedRun[]
  run?: RunProcess
  after?: (x: { caseDir: string; run: RecordedRun; before: Snapshot }) => EngineRun | undefined
}): (args: string[], extraEnv?: Record<string, string>) => Promise<EngineRun> {
  return async (args, extraEnv) => {
    const env = extraEnv ? { ...o.env, ...extraEnv } : o.env
    const problems = engineEnvProblems(env)
    if (problems.length > 0) throw new StepError(`refusing to run the engine: ${problems.join('; ')}`)
    const before = snapshotDir(o.caseDir)
    const r = await (o.run ?? runHidden)(o.exe, args, { cwd: o.caseDir, env, timeoutMs: o.timeoutMs })
    const { changed, gone } = snapshotChanges(before, snapshotDir(o.caseDir))
    const index = o.runs.length
    const s = runStore(o.store, index)
    rmSync(s.dir, { recursive: true, force: true })
    mkdirSync(s.files, { recursive: true })
    writeFileSync(s.stdout, r.stdout)
    writeFileSync(s.stderr, r.stderr)
    for (const rel of changed) copyFile(nativePath(o.caseDir, rel), nativePath(s.files, rel))
    const recorded: RecordedRun = { index, args: portableArgs(args, o.root), extraEnv: Object.keys(extraEnv ?? {}).sort(), code: r.code, timedOut: r.timedOut, files: changed, gone }
    o.runs.push(recorded)
    const carried = o.after?.({ caseDir: o.caseDir, run: recorded, before })
    if (carried) {
      recorded.carried = true
      return carried
    }
    return { code: r.code, timedOut: r.timedOut, stdout: r.stdout.toString('utf8') }
  }
}

export interface CaseRecord {
  id: string
  ok: boolean
  problems: string[]
  warnings: string[]
  runs: RecordedRun[]
}

export interface EngineCasesOptions {
  /** The cases' folder, the same for both engines. */
  root: string
  /** Where this engine's outputs are kept: <store>/<case>/<n>/. */
  store: string
  cases: SmokeCase[]
  sets: SmokePresetSets
  lock: OrcaLock
  /** The Orca checkout the engines are built from, when at hand (config-def reads it). */
  orcaSource?: string
  baseEnv: NodeJS.ProcessEnv
  platform: Platform
  timeoutMs: number
  run?: RunProcess
  /** Acts on a case's folder after each of its runs (AfterRun); parity certify's goldenCarry. */
  after?: AfterRun
  log: (msg: string) => void
}

/**
 * Runs the cases with one engine, as smoke.ts runCases does, recording every engine run. The label
 * names the engine in the log: reference or head here, the oracle's runs in parity neutral.
 */
export async function runEngineCases(engine: Pick<EngineUnderTest, 'exe' | 'resources' | 'libraryPath'> & { label: string }, o: EngineCasesOptions): Promise<CaseRecord[]> {
  rmSync(o.root, { recursive: true, force: true })
  rmSync(o.store, { recursive: true, force: true })
  mkdirSync(o.root, { recursive: true })
  const env = parityEngineEnv(engine, o.baseEnv, o.platform)
  const files = {} as Record<keyof typeof SMOKE_PRESETS, PresetFiles>
  for (const k of Object.keys(SMOKE_PRESETS) as (keyof typeof SMOKE_PRESETS)[]) files[k] = writePresetSet(join(o.root, 'presets', k), o.sets[k])
  const out: CaseRecord[] = []
  for (const c of o.cases) {
    const dir = join(o.root, c.id)
    rmSync(dir, { recursive: true, force: true })
    mkdirSync(dir, { recursive: true })
    const runs: RecordedRun[] = []
    const caseEnv: Parameters<SmokeCase['run']>[0] = {
      exe: engine.exe,
      dir,
      resources: engine.resources,
      ...(o.orcaSource ? { orcaSource: o.orcaSource } : {}),
      lock: o.lock,
      sets: o.sets,
      files,
      exec: recordingExec({ exe: engine.exe, caseDir: dir, root: o.root, store: join(o.store, c.id), env, timeoutMs: o.timeoutMs, runs, run: o.run, ...(o.after ? { after: (x) => o.after!({ caseId: c.id, ...x }) } : {}) }),
    }
    let problems: string[]
    let warnings: string[]
    try {
      ;({ problems, warnings } = await c.run(caseEnv))
    } catch (err) {
      problems = [(err as Error).message]
      warnings = []
    }
    out.push({ id: c.id, ok: problems.length === 0, problems, warnings, runs })
    o.log(`${engine.label}: ${problems.length === 0 ? 'pass' : 'FAIL'} ${c.id} (${runs.length} engine run${runs.length === 1 ? '' : 's'})`)
  }
  return out
}

// ---------------------------------------------------------------- comparing

export interface OutputReport {
  /** exit, stdout, stderr, args, or files/<path relative to the case's folder>. */
  name: string
  status: OutputStatus
  masks: string[]
  detail?: string
}

export interface RunReport {
  index: number
  args: string[]
  outputs: OutputReport[]
}

export interface CaseReport {
  id: string
  smoke: Record<EngineLabel, { ok: boolean; problems: string[] }>
  runs: RunReport[]
}

function readIf(path: string): Buffer | undefined {
  return existsSync(path) ? readFileSync(path) : undefined
}

/** How the details of a comparison name its two sides: the reference and the head here; parity neutral names its own pairs. */
export const SAME_LABELS = { reference: 'reference', head: 'head' } as const

/**
 * Compares one engine run of each engine: exit code, stdout, stderr and every file either wrote or
 * removed. `labels` names the two sides in the details.
 */
export function compareRun(ref: RecordedRun | undefined, head: RecordedRun | undefined, stores: { reference: string; head: string }, masks: Mask[], labels: { reference: string; head: string } = SAME_LABELS): RunReport {
  const index = (ref ?? head)!.index
  if (!ref || !head) return { index, args: (ref ?? head)!.args, outputs: [{ name: 'run', status: 'different', masks: [], detail: `only the ${ref ? labels.reference : labels.head} engine ran it` }] }
  const outputs: OutputReport[] = []
  const add = (name: string, c: { status: OutputStatus; masks: string[]; detail?: string }) => outputs.push({ name, status: c.status, masks: c.masks, ...(c.detail ? { detail: c.detail } : {}) })
  if (ref.args.join('\u0000') !== head.args.join('\u0000') || ref.extraEnv.join(',') !== head.extraEnv.join(',')) {
    add('args', { status: 'different', masks: [], detail: `the ${labels.reference} ran ${JSON.stringify(ref.args)}, the ${labels.head} ${JSON.stringify(head.args)}` })
  }
  const exit = (r: RecordedRun) => (r.timedOut ? 'timed out' : String(r.code))
  add('exit', exit(ref) === exit(head) ? { status: 'identical', masks: [] } : { status: 'different', masks: [], detail: `${exit(ref)} against ${exit(head)}` })
  const rs = runStore(stores.reference, ref.index)
  const hs = runStore(stores.head, head.index)
  const sides: [string, string] = [labels.reference, labels.head]
  add('stdout', compareOutput('stdout', readIf(rs.stdout) ?? Buffer.alloc(0), readIf(hs.stdout) ?? Buffer.alloc(0), masks, sides))
  add('stderr', compareOutput('stderr', readIf(rs.stderr) ?? Buffer.alloc(0), readIf(hs.stderr) ?? Buffer.alloc(0), masks, sides))
  for (const p of [...new Set([...ref.files, ...head.files])].sort()) {
    add(`files/${p}`, compareOutput(p, ref.files.includes(p) ? readIf(nativePath(rs.files, p)) : undefined, head.files.includes(p) ? readIf(nativePath(hs.files, p)) : undefined, masks, sides))
  }
  for (const p of [...new Set([...ref.gone, ...head.gone])].sort()) {
    const both = ref.gone.includes(p) && head.gone.includes(p)
    add(`removed/${p}`, both ? { status: 'identical', masks: [] } : { status: 'different', masks: [], detail: `only the ${ref.gone.includes(p) ? labels.reference : labels.head} engine removed it` })
  }
  return { index, args: ref.args, outputs }
}

/** compareRun for each case's runs; `labels` names the two sides in the details. */
export function compareCases(ref: CaseRecord[], head: CaseRecord[], stores: { reference: string; head: string }, masks: Mask[], labels: { reference: string; head: string } = SAME_LABELS): CaseReport[] {
  return ref.map((r) => {
    const h = head.find((c) => c.id === r.id)
    const runs: RunReport[] = []
    const n = Math.max(r.runs.length, h?.runs.length ?? 0)
    for (let i = 0; i < n; i++) runs.push(compareRun(r.runs[i], h?.runs[i], { reference: join(stores.reference, r.id), head: join(stores.head, r.id) }, masks, labels))
    return {
      id: r.id,
      smoke: { reference: { ok: r.ok, problems: r.problems }, head: { ok: h?.ok ?? false, problems: h ? h.problems : ['the case did not run'] } },
      runs,
    }
  })
}

export interface SameEngineInfo {
  dir: string
  version: string
  stageVersion: number
  orcaCommit: string
  exeSha256: string
  manifestSha256: string
}

export interface SameReport {
  format: typeof SAME_FORMAT
  createdAt: string
  smokeVersion: number
  masks: { file: string; sha256: string }
  /** The variables each engine ran with (their names; parityEngineEnv). */
  env: string[]
  engines: Record<EngineLabel, SameEngineInfo>
  summary: { cases: number; runs: number; outputs: number } & Record<OutputStatus, number>
  /** Every output that differs, as <case>/<run>/<output>: the candidates for sorting out. */
  different: string[]
  cases: CaseReport[]
}

export function engineInfo(e: EngineUnderTest): SameEngineInfo {
  const file = e.manifest.files.find((f) => f.path === e.manifest.engine.exe)!
  return {
    dir: e.dir,
    version: e.manifest.engine.version,
    stageVersion: e.manifest.stageVersion,
    orcaCommit: e.manifest.orca.commit,
    exeSha256: file.sha256,
    manifestSha256: sha256Bytes(readFileSync(join(e.dir, 'manifest.json'))),
  }
}

export function sameReport(o: { cases: CaseReport[]; engines: Record<EngineLabel, SameEngineInfo>; masks: { file: string; sha256: string }; env: string[] }): SameReport {
  const summary = { cases: o.cases.length, runs: 0, outputs: 0, identical: 0, masked: 0, different: 0 }
  const different: string[] = []
  for (const c of o.cases) {
    for (const r of c.runs) {
      summary.runs++
      for (const out of r.outputs) {
        summary.outputs++
        summary[out.status]++
        if (out.status === 'different') different.push(`${c.id}/${r.index}/${out.name}`)
      }
    }
  }
  return { format: SAME_FORMAT, createdAt: new Date().toISOString(), smokeVersion: SMOKE_VERSION, masks: o.masks, env: o.env, engines: o.engines, summary, different, cases: o.cases }
}

/** <work>/parity/same.json and the folders the same-check runs in. */
export function sameFiles(paths: Pick<ParityPaths, 'parity' | 'runs'>): { report: string; root: string; stores: Record<EngineLabel, string> } {
  const base = join(paths.runs, 'same')
  return { report: join(paths.parity, 'same.json'), root: join(base, 'cases'), stores: { reference: join(base, 'reference'), head: join(base, 'head') } }
}
