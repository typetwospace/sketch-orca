// The neutrality gate: the oracle (oracle.ts) is the engine plus knobs that act only when set, and
// parity neutral checks that, on every smoke case's jobs (smoke.ts SMOKE_CASES), run as parity same
// runs them (same.ts runEngineCases, the environment a client gives the engine):
//   (a) unset   with no knob set, the oracle's outputs equal those of the engine staged from this
//               checkout (<work>/dist/engine) after parity/masks.json, as the reference's do;
//   (b) pinned  with SKETCH_ORCA_ORACLE_TIME and SKETCH_ORCA_ORACLE_SEED set, two runs of the oracle
//               are byte for byte the same, with no mask at all;
//   (c) threads with SKETCH_ORCA_ORACLE_THREADS=1 and =8 (TIME and SEED pinned, and
//               SKETCH_ORCA_ORACLE_DUMP set to a folder of each engine run's own), every run
//               finishes, and every output or dump that differs between the two is recorded as a
//               T candidate (a difference that comes from the thread count), not a failure.
// (a) and (b) are the gate: parity neutral exits 1 when either fails. (c) also records what the
// RUN record of each dump (parity/SOP1.md) says TBB reports, so whether the arena of patch 0003
// reports P to the code that asks is read from the oracle itself, not assumed.
// Everything goes in <work>/parity/neutral.json; the runs are kept in <work>/parity/runs/neutral.
// The SOP1 reader here (readSop1, sop1Differences) and portableDump (a dump's paths written as
// <case>, <cases>, <resources> and <work> inside its strings) serve parity record, goldens --check
// and certify too, as does knobRunner, which can drop each run's dumps but the kept boundaries.

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from 'node:fs'
import { basename, join } from 'node:path'
import type { Platform } from '../env.ts'
import type { OrcaLock } from '../lock.ts'
import type { ParityPaths } from '../paths.ts'
import { StepError } from '../run.ts'
import { SMOKE_VERSION, type SmokeCase, type SmokePresetSets } from '../smoke.ts'
import { portableText } from './classes.ts'
import type { Mask, OutputStatus } from './masks.ts'
import { ORACLE_KNOBS, type OracleUnderTest } from './oracle.ts'
import { compareCases, runEngineCases, runHidden, type CaseRecord, type CaseReport, type EngineUnderTest, type RunProcess, type SameEngineInfo } from './same.ts'

export const NEUTRAL_FORMAT = 'sketch-orca-parity-neutral/1'

/** The time (b) and (c) pin: 2026-01-01 00:00:00 UTC, in seconds since 1970. */
export const NEUTRAL_TIME = '1767225600'

/** The seed (b) and (c) pin. */
export const NEUTRAL_SEED = '1'

/** The thread counts (c) compares. */
export const NEUTRAL_THREADS = [1, 8] as const

/** The knobs (b) and (c) set. */
export const PINNED_KNOBS: Record<string, string> = { [ORACLE_KNOBS.time]: NEUTRAL_TIME, [ORACLE_KNOBS.seed]: NEUTRAL_SEED }

// ---------------------------------------------------------------- SOP1 (parity/SOP1.md)

export const SOP1_MAGIC = 'SOP1'
export const SOP1_VERSION = 1

export interface Sop1Record {
  /** The four-letter kind, as written ('RUN ' and 'END ' end in a space). */
  kind: string
  payload: Buffer
}

export interface Sop1File {
  version: number
  /** Every record before END. */
  records: Sop1Record[]
}

/** Reads an SOP1 container: the magic, the version, the records and the END record that counts them. Throws on anything else. */
export function readSop1(bytes: Buffer): Sop1File {
  if (bytes.length < 8 || bytes.toString('latin1', 0, 4) !== SOP1_MAGIC) throw new Error('not an SOP1 file (no SOP1 magic)')
  const version = bytes.readUInt32LE(4)
  if (version !== SOP1_VERSION) throw new Error(`SOP1 version ${version}, this reader knows ${SOP1_VERSION}`)
  const records: Sop1Record[] = []
  let at = 8
  for (;;) {
    if (at + 12 > bytes.length) throw new Error(`the file ends inside a record header at byte ${at} (no END record)`)
    const kind = bytes.toString('latin1', at, at + 4)
    const length = bytes.readBigUInt64LE(at + 4)
    const start = at + 12
    if (length > BigInt(bytes.length - start)) throw new Error(`the ${JSON.stringify(kind)} record at byte ${at} is longer than the file`)
    const payload = bytes.subarray(start, start + Number(length))
    at = start + Number(length)
    if (kind === 'END ') {
      if (payload.length !== 8) throw new Error('the END record is not a u64')
      const count = payload.readBigUInt64LE(0)
      if (count !== BigInt(records.length)) throw new Error(`the END record counts ${count} records, the file has ${records.length}`)
      if (at !== bytes.length) throw new Error(`${bytes.length - at} bytes after the END record`)
      return { version, records }
    }
    records.push({ kind, payload })
  }
}

/** A cursor over a payload, reading as Payload in OracleDump.cpp writes. */
export class Sop1Reader {
  private at = 0
  private readonly buf: Buffer
  constructor(buf: Buffer) {
    this.buf = buf
  }
  private need(n: number): number {
    if (this.at + n > this.buf.length) throw new Error(`the payload ends at byte ${this.buf.length}, ${n} more were needed at ${this.at}`)
    const at = this.at
    this.at += n
    return at
  }
  u8(): number {
    return this.buf.readUInt8(this.need(1))
  }
  u32(): number {
    return this.buf.readUInt32LE(this.need(4))
  }
  i32(): number {
    return this.buf.readInt32LE(this.need(4))
  }
  u64(): bigint {
    return this.buf.readBigUInt64LE(this.need(8))
  }
  i64(): bigint {
    return this.buf.readBigInt64LE(this.need(8))
  }
  bool(): boolean {
    const v = this.u8()
    if (v > 1) throw new Error(`a boolean of ${v}`)
    return v === 1
  }
  str(): string {
    const n = this.u32()
    return this.buf.toString('utf8', this.need(n), this.at)
  }
  done(): boolean {
    return this.at === this.buf.length
  }
}

export interface Sop1Head {
  source: string
  name: string
  object: bigint
  occurrence: number
}

export function parseHead(payload: Buffer): Sop1Head {
  const r = new Sop1Reader(payload)
  const head = { source: r.str(), name: r.str(), object: r.i64(), occurrence: r.u32() }
  if (!r.done()) throw new Error('the HEAD record is longer than its fields')
  return head
}

export interface Sop1Run {
  knobs: { name: string; set: boolean; value: string }[]
  /** tbb::this_task_arena::max_concurrency() at the boundary. */
  maxConcurrency: number
  /** tbb::global_control::active_value(max_allowed_parallelism) at the boundary. */
  maxAllowedParallelism: number
  /** _get_FMA3_enable(), or -1 where there is none. */
  fma3Enable: number
}

export function parseRun(payload: Buffer): Sop1Run {
  const r = new Sop1Reader(payload)
  const n = Number(r.u64())
  const knobs: Sop1Run['knobs'] = []
  for (let i = 0; i < n; i++) knobs.push({ name: r.str(), set: r.bool(), value: r.str() })
  const run = { knobs, maxConcurrency: r.i32(), maxAllowedParallelism: Number(r.u64()), fma3Enable: r.i32() }
  if (!r.done()) throw new Error('the RUN record is longer than its fields')
  return run
}

/** Where two dumps of one boundary differ, the RUN record left out (it names the knobs, which differ on purpose). */
export function sop1Differences(a: Sop1File, b: Sop1File): string[] {
  const keep = (f: Sop1File) => f.records.map((r, i) => ({ ...r, i })).filter((r) => r.kind !== 'RUN ')
  const x = keep(a)
  const y = keep(b)
  const out: string[] = []
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const p = x[i]
    const q = y[i]
    if (!p || !q) out.push(`record ${(p ?? q)!.i} (${(p ?? q)!.kind.trim()}) is in one dump only`)
    else if (p.kind !== q.kind) out.push(`record ${p.i} is ${p.kind.trim()} in one and ${q.kind.trim()} in the other`)
    else if (!p.payload.equals(q.payload)) out.push(`record ${p.i} (${p.kind.trim()})`)
  }
  return out
}

/** The fields of a record kind whose strings portableDump rewrites, in the order OracleDump.cpp writes them. */
type Sop1Field = 'u8' | 'bool' | 'u32' | 'i32' | 'f32' | 'u64' | 'i64' | 'f64' | 'str' | { count: Sop1Field[] } | { fixed: number; of: Sop1Field[] }

const MATRIX: Sop1Field = { fixed: 16, of: ['f64'] }

/**
 * The record kinds whose strings can hold a path (parity/SOP1.md): a config's values, an object's
 * or volume's name, the knobs' values, a custom G-code's colour and text (PCGC, and the tool
 * ordering's TLXT, oracle patch 0007), and a step's warnings (OWRN and PWRN, patch 0007). The other
 * kinds of front-objects-<n>.sop1 (oracle patch 0006, 2026-11-07: HRNG, BRIM, PNTV) hold no
 * string; they are listed so the whole file is read as its fields, and a record that does not read
 * leaves the dump as it is. Other kinds hold no string and are compared as they are.
 */
export const SOP1_STRING_FIELDS: Record<string, Sop1Field[]> = {
  HEAD: ['str', 'str', 'i64', 'u32'],
  'RUN ': [{ count: ['str', 'bool', 'str'] }, 'i32', 'u64', 'i32'],
  CONF: ['str', 'i64', 'i64', { count: ['str', 'u32', 'str', { count: ['f64'] }] }],
  MODL: ['i64', 'str', 'bool', { count: ['f64'] }, { count: [MATRIX, 'bool'] }, { count: ['str', 'i32', MATRIX, { count: ['f32', 'f32', 'f32'] }, { count: ['i32', 'i32', 'i32'] }] }],
  PCGC: ['i32', { count: ['i32', 'i32', { count: ['f64', 'i32', 'i32', 'str', 'str'] }] }],
  HRNG: ['i64', { count: ['f64', 'f64'] }],
  BRIM: ['i64', { count: ['f32', 'f32', 'f32', 'f32'] }],
  PNTV: ['i64', 'i64', { fixed: 4, of: [{ count: ['i32', 'i32'] }, 'u64', { count: ['u8'] }, { count: ['bool'] }] }],
  OWRN: ['i64', { count: ['u8', { count: ['u8', 'bool', 'str', 'i32'] }] }],
  PWRN: [{ count: ['u8', { count: ['u8', 'bool', 'str', 'i32'] }] }],
  TLXT: [{ count: [{ count: ['f64', 'i32', 'i32', 'str', 'str'] }] }],
}

const FIXED_SIZE: Record<string, number> = { u8: 1, bool: 1, u32: 4, i32: 4, f32: 4, u64: 8, i64: 8, f64: 8 }

/** Copies `fields` from `src` at `at` into `out`, each string through `str`; returns where it stopped. */
function rewriteFields(src: Buffer, at: number, fields: Sop1Field[], out: Buffer[], str: (s: Buffer) => Buffer): number {
  const need = (n: number) => {
    if (at + n > src.length) throw new Error('the payload ends inside a field')
  }
  for (const f of fields) {
    if (typeof f === 'string') {
      if (f === 'str') {
        need(4)
        const n = src.readUInt32LE(at)
        need(4 + n)
        const s = str(src.subarray(at + 4, at + 4 + n))
        const len = Buffer.alloc(4)
        len.writeUInt32LE(s.length)
        out.push(len, s)
        at += 4 + n
      } else {
        const n = FIXED_SIZE[f]!
        need(n)
        out.push(src.subarray(at, at + n))
        at += n
      }
    } else if ('count' in f) {
      need(8)
      const n = src.readBigUInt64LE(at)
      out.push(src.subarray(at, at + 8))
      at += 8
      for (let i = 0n; i < n; i++) at = rewriteFields(src, at, f.count, out, str)
    } else {
      for (let i = 0; i < f.fixed; i++) at = rewriteFields(src, at, f.of, out, str)
    }
  }
  return at
}

/**
 * A dump with its paths portable between folders: each of `roots` written as its name
 * (classes.ts portableText) in the strings of the kinds of SOP1_STRING_FIELDS, each string's
 * length and its record's length written again. The bytes as they are when no root appears in
 * them, or when they are not an SOP1 file of those fields (then a path in them makes them differ).
 */
export function portableDump(bytes: Buffer | undefined, roots: [string, string][]): Buffer | undefined {
  if (bytes === undefined) return undefined
  const latin = bytes.toString('latin1')
  const asBytes = (s: string) => Buffer.from(s, 'utf8').toString('latin1')
  const forms = roots.flatMap(([r]) => [r, r.replace(/\\/g, '/'), r.replace(/\\/g, '\\\\'), r.replace(/\//g, '\\\\')]).filter((f) => f !== '').map(asBytes)
  if (!forms.some((f) => latin.includes(f))) return bytes
  const latinRoots = roots.map(([r, n]): [string, string] => [asBytes(r), asBytes(n)])
  const str = (s: Buffer) => Buffer.from(portableText(s.toString('latin1'), latinRoots), 'latin1')
  try {
    readSop1(bytes)
    const out: Buffer[] = [bytes.subarray(0, 8)]
    let at = 8
    while (at < bytes.length) {
      const kind = bytes.toString('latin1', at, at + 4)
      const length = Number(bytes.readBigUInt64LE(at + 4))
      const payload = bytes.subarray(at + 12, at + 12 + length)
      const fields = SOP1_STRING_FIELDS[kind]
      let body = payload
      if (fields) {
        const parts: Buffer[] = []
        if (rewriteFields(payload, 0, fields, parts, str) !== payload.length) throw new Error(`the ${kind} record is longer than its fields`)
        body = Buffer.concat(parts)
      }
      const head = Buffer.alloc(12)
      head.write(kind, 0, 'latin1')
      head.writeBigUInt64LE(BigInt(body.length), 4)
      out.push(head, body)
      at += 12 + length
    }
    return Buffer.concat(out)
  } catch {
    return bytes
  }
}

// ---------------------------------------------------------------- running the oracle with knobs

/**
 * A RunProcess that runs the engine with `knobs` added to its environment and, with `dumps`, with
 * SKETCH_ORCA_ORACLE_DUMP set to a new folder per engine run: <dumps>/<case>/<n>, n counting the
 * case's runs from 0 as same.ts's run stores do. With `keep` (a dump's file name to whether it is
 * kept), every other file the run wrote there is removed as soon as the run ends, so a pass holds
 * only the boundaries it compares (record.ts KEPT_DUMPS) on disk. That one knob sets every dump:
 * oracle patch 0007's step boundaries (front-config's, the object and print records it adds, and
 * print-psConflictCheck) read no knob of their own, and none changes an output.
 */
export function knobRunner(knobs: Record<string, string>, dumps?: string, run: RunProcess = runHidden, keep?: (file: string) => boolean): RunProcess {
  const counts = new Map<string, number>()
  return async (exe, args, o) => {
    const env = { ...o.env, ...knobs }
    let folder: string | undefined
    if (dumps !== undefined) {
      const id = basename(o.cwd)
      const n = counts.get(id) ?? 0
      counts.set(id, n + 1)
      folder = join(dumps, id, String(n))
      rmSync(folder, { recursive: true, force: true })
      mkdirSync(folder, { recursive: true })
      env[ORACLE_KNOBS.dump] = folder
    }
    const outcome = await run(exe, args, { ...o, env })
    if (folder !== undefined && keep !== undefined && existsSync(folder)) for (const f of readdirSync(folder)) if (!keep(f)) rmSync(join(folder, f), { recursive: true, force: true })
    return outcome
  }
}

// ---------------------------------------------------------------- the report

export interface PairSummary {
  cases: number
  runs: number
  outputs: number
  identical: number
  masked: number
  different: number
}

export interface PairCase {
  id: string
  /** The smoke checks of each side, by its label. */
  smoke: Record<string, { ok: boolean; problems: string[] }>
  runs: CaseReport['runs']
}

export interface PairReport {
  /** The two sides: the first is compared against the second. */
  sides: [{ label: string; knobs: Record<string, string> }, { label: string; knobs: Record<string, string> }]
  /** The masks file, or null when no mask applies. */
  masks: string | null
  summary: PairSummary
  /** Every output that differs, as <case>/<run>/<output>. */
  different: string[]
  cases: PairCase[]
}

export interface ArenaReport {
  /** The thread count the knob asked for. */
  threads: number
  /** The dumps read. */
  dumps: number
  /** Each distinct (max_concurrency, max_allowed_parallelism, FMA3) that a RUN record gave. */
  seen: { maxConcurrency: number; maxAllowedParallelism: number; fma3Enable: number; dumps: number }[]
  /** Every RUN record gave max_concurrency() = the thread count. */
  reportsP: boolean
  /** Every RUN record gave global_control's max_allowed_parallelism = 1: TBB had no worker thread. */
  oneThread: boolean
}

export interface DumpComparison {
  /** Dump files compared (in both folders of a run). */
  compared: number
  identical: number
  /** <case>/<run>/<file>: where its records differ. */
  different: { file: string; records: string[] }[]
  /** <case>/<run>/<file> written in one folder only. */
  only: string[]
  /** Dumps that could not be read: <case>/<run>/<file>: why. */
  unreadable: string[]
}

export interface NeutralReport {
  format: typeof NEUTRAL_FORMAT
  createdAt: string
  smokeVersion: number
  engines: { head: SameEngineInfo; oracle: { dir: string; exeSha256: string; oraclePatches: { name: string; sha256: string }[]; sourceDigest: string } }
  masks: { file: string; sha256: string }
  /** (a) and (b) passed: parity neutral exits 0. */
  passed: boolean
  unset: PairReport & { passed: boolean }
  pinned: PairReport & { passed: boolean }
  threads: PairReport & {
    /** Every engine run of both ended by itself (none timed out). */
    finished: boolean
    /** <side>/<case>/<run> of each run that timed out. */
    unfinished: string[]
    /** The T candidates: each output and dump that differs between the thread counts. */
    candidates: string[]
    dumps: DumpComparison
    arena: ArenaReport[]
  }
}

export function pairReport(o: { a: CaseRecord[]; b: CaseRecord[]; stores: [string, string]; sides: PairReport['sides']; masks: Mask[]; masksFile: string | null }): PairReport {
  const [la, lb] = [o.sides[0].label, o.sides[1].label]
  const cases = compareCases(o.a, o.b, { reference: o.stores[0], head: o.stores[1] }, o.masks, { reference: la, head: lb })
  const summary: PairSummary = { cases: cases.length, runs: 0, outputs: 0, identical: 0, masked: 0, different: 0 }
  const different: string[] = []
  for (const c of cases) {
    for (const r of c.runs) {
      summary.runs++
      for (const out of r.outputs) {
        summary.outputs++
        summary[out.status as OutputStatus]++
        if (out.status === 'different') different.push(`${c.id}/${r.index}/${out.name}`)
      }
    }
  }
  return {
    sides: o.sides,
    masks: o.masksFile,
    summary,
    different,
    cases: cases.map((c) => ({ id: c.id, smoke: { [la]: c.smoke.reference, [lb]: c.smoke.head }, runs: c.runs })),
  }
}

/** The runs of `cases` that timed out, as <label>/<case>/<run>. */
export function unfinishedRuns(label: string, cases: CaseRecord[]): string[] {
  return cases.flatMap((c) => c.runs.filter((r) => r.timedOut).map((r) => `${label}/${c.id}/${r.index}`))
}

function sop1Files(dir: string): string[] {
  return existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith('.sop1')).sort() : []
}

/** Each run's dump folder under `root` (<root>/<case>/<n>), as <case>/<n>. */
function runFolders(root: string): string[] {
  if (!existsSync(root)) return []
  const out: string[] = []
  for (const c of readdirSync(root).sort()) for (const n of readdirSync(join(root, c)).sort((x, y) => Number(x) - Number(y))) out.push(`${c}/${n}`)
  return out
}

/** Compares the dumps of the same runs in two dump roots, file by file, record by record (sop1Differences). */
export function compareDumps(a: string, b: string): DumpComparison {
  const out: DumpComparison = { compared: 0, identical: 0, different: [], only: [], unreadable: [] }
  for (const run of [...new Set([...runFolders(a), ...runFolders(b)])].sort()) {
    const fa = sop1Files(join(a, run))
    const fb = sop1Files(join(b, run))
    for (const f of [...new Set([...fa, ...fb])].sort()) {
      const name = `${run}/${f}`
      if (!fa.includes(f) || !fb.includes(f)) {
        out.only.push(name)
        continue
      }
      out.compared++
      const x = readFileSync(join(a, run, f))
      const y = readFileSync(join(b, run, f))
      let records: string[]
      try {
        records = sop1Differences(readSop1(x), readSop1(y))
      } catch (err) {
        out.unreadable.push(`${name}: ${(err as Error).message}`)
        continue
      }
      if (records.length === 0) out.identical++
      else out.different.push({ file: name, records: records.slice(0, 20) })
    }
  }
  return out
}

/** What the RUN records of every dump under `root` say TBB and the CRT reported, for the thread count `threads`. */
export function arenaReport(root: string, threads: number): ArenaReport & { unreadable: string[] } {
  const seen = new Map<string, ArenaReport['seen'][number]>()
  const unreadable: string[] = []
  let dumps = 0
  for (const run of runFolders(root)) {
    for (const f of sop1Files(join(root, run))) {
      try {
        const file = readSop1(readFileSync(join(root, run, f)))
        const rec = file.records.find((r) => r.kind === 'RUN ')
        if (!rec) throw new Error('no RUN record')
        const r = parseRun(rec.payload)
        const key = `${r.maxConcurrency}/${r.maxAllowedParallelism}/${r.fma3Enable}`
        const s = seen.get(key) ?? { maxConcurrency: r.maxConcurrency, maxAllowedParallelism: r.maxAllowedParallelism, fma3Enable: r.fma3Enable, dumps: 0 }
        s.dumps++
        seen.set(key, s)
        dumps++
      } catch (err) {
        unreadable.push(`${run}/${f}: ${(err as Error).message}`)
      }
    }
  }
  const list = [...seen.values()]
  return {
    threads,
    dumps,
    seen: list,
    reportsP: dumps > 0 && list.every((s) => s.maxConcurrency === threads),
    oneThread: dumps > 0 && list.every((s) => s.maxAllowedParallelism === 1),
    unreadable,
  }
}

// ---------------------------------------------------------------- the gate

/** <work>/parity/neutral.json and the folders parity neutral runs in (<work>/parity/runs/neutral). */
export function neutralFiles(paths: Pick<ParityPaths, 'parity' | 'runs'>) {
  const base = join(paths.runs, 'neutral')
  return {
    report: join(paths.parity, 'neutral.json'),
    root: join(base, 'cases'),
    stores: {
      head: join(base, 'head'),
      oracle: join(base, 'oracle'),
      pinnedA: join(base, 'pinned-a'),
      pinnedB: join(base, 'pinned-b'),
      t1: join(base, 'threads-1'),
      t8: join(base, 'threads-8'),
    },
    dumps: { t1: join(base, 'dumps-t1'), t8: join(base, 'dumps-t8') },
  }
}

export interface NeutralOptions {
  head: EngineUnderTest
  headInfo: SameEngineInfo
  oracle: OracleUnderTest
  paths: Pick<ParityPaths, 'parity' | 'runs'>
  cases: SmokeCase[]
  sets: SmokePresetSets
  lock: OrcaLock
  orcaSource?: string
  baseEnv: NodeJS.ProcessEnv
  platform: Platform
  timeoutMs: number
  masks: Mask[]
  masksFile: { file: string; sha256: string }
  /** How engine runs start (runHidden; tests give their own). */
  run?: RunProcess
  log: (msg: string) => void
}

/** Runs (a), (b) and (c) and returns the report; the caller writes it. */
export async function runNeutral(o: NeutralOptions): Promise<NeutralReport> {
  const files = neutralFiles(o.paths)
  const base = o.run ?? runHidden
  const common = {
    root: files.root,
    cases: o.cases,
    sets: o.sets,
    lock: o.lock,
    ...(o.orcaSource ? { orcaSource: o.orcaSource } : {}),
    baseEnv: o.baseEnv,
    platform: o.platform,
    timeoutMs: o.timeoutMs,
    log: o.log,
  }
  const oracle = (label: string) => ({ label, exe: o.oracle.exe, resources: o.oracle.resources, libraryPath: o.oracle.libraryPath })
  const say = o.log

  say(`(a) unset: the head engine and the oracle with no knob set, compared after ${o.masksFile.file}`)
  const head = await runEngineCases(o.head, { ...common, store: files.stores.head, run: base })
  const plain = await runEngineCases(oracle('oracle'), { ...common, store: files.stores.oracle, run: base })
  const unset = pairReport({ a: head, b: plain, stores: [files.stores.head, files.stores.oracle], sides: [{ label: 'head', knobs: {} }, { label: 'oracle', knobs: {} }], masks: o.masks, masksFile: o.masksFile.file })

  say(`(b) pinned: the oracle twice with ${Object.entries(PINNED_KNOBS).map(([k, v]) => `${k}=${v}`).join(' ')}, compared byte for byte`)
  const a = await runEngineCases(oracle('oracle-pinned-a'), { ...common, store: files.stores.pinnedA, run: knobRunner(PINNED_KNOBS, undefined, base) })
  const b = await runEngineCases(oracle('oracle-pinned-b'), { ...common, store: files.stores.pinnedB, run: knobRunner(PINNED_KNOBS, undefined, base) })
  const pinned = pairReport({ a, b, stores: [files.stores.pinnedA, files.stores.pinnedB], sides: [{ label: 'oracle-pinned-a', knobs: PINNED_KNOBS }, { label: 'oracle-pinned-b', knobs: PINNED_KNOBS }], masks: [], masksFile: null })

  const [n1, n8] = NEUTRAL_THREADS
  const k1 = { ...PINNED_KNOBS, [ORACLE_KNOBS.threads]: String(n1) }
  const k8 = { ...PINNED_KNOBS, [ORACLE_KNOBS.threads]: String(n8) }
  say(`(c) threads: the oracle with ${ORACLE_KNOBS.threads}=${n1} and =${n8}, time and seed pinned, dumping each run`)
  for (const d of [files.dumps.t1, files.dumps.t8]) rmSync(d, { recursive: true, force: true })
  const t1 = await runEngineCases(oracle(`oracle-threads-${n1}`), { ...common, store: files.stores.t1, run: knobRunner(k1, files.dumps.t1, base) })
  const t8 = await runEngineCases(oracle(`oracle-threads-${n8}`), { ...common, store: files.stores.t8, run: knobRunner(k8, files.dumps.t8, base) })
  const threads = pairReport({ a: t1, b: t8, stores: [files.stores.t1, files.stores.t8], sides: [{ label: `oracle-threads-${n1}`, knobs: k1 }, { label: `oracle-threads-${n8}`, knobs: k8 }], masks: [], masksFile: null })
  const unfinished = [...unfinishedRuns(`threads-${n1}`, t1), ...unfinishedRuns(`threads-${n8}`, t8)]
  const dumps = compareDumps(files.dumps.t1, files.dumps.t8)
  const arena1 = arenaReport(files.dumps.t1, n1)
  const arena8 = arenaReport(files.dumps.t8, n8)
  dumps.unreadable.push(...arena1.unreadable.map((u) => `dumps-t1/${u}`), ...arena8.unreadable.map((u) => `dumps-t8/${u}`))
  const candidates = [...threads.different, ...dumps.different.map((d) => `dump/${d.file}`), ...dumps.only.map((f) => `dump/${f} (one thread count only)`)]
  const strip = ({ unreadable: _u, ...rest }: ArenaReport & { unreadable: string[] }): ArenaReport => rest

  const unsetPassed = unset.summary.different === 0
  const pinnedPassed = pinned.summary.different === 0 && pinned.summary.identical === pinned.summary.outputs
  return {
    format: NEUTRAL_FORMAT,
    createdAt: new Date().toISOString(),
    smokeVersion: SMOKE_VERSION,
    engines: {
      head: o.headInfo,
      oracle: { dir: o.oracle.dir, exeSha256: o.oracle.manifest.exe.sha256, oraclePatches: o.oracle.manifest.oraclePatches, sourceDigest: o.oracle.manifest.sourceDigest },
    },
    masks: o.masksFile,
    passed: unsetPassed && pinnedPassed,
    unset: { ...unset, passed: unsetPassed },
    pinned: { ...pinned, passed: pinnedPassed },
    threads: { ...threads, finished: unfinished.length === 0, unfinished, candidates, dumps, arena: [strip(arena1), strip(arena8)] },
  }
}

/** Refuses a head engine or oracle staged for another platform. */
export function checkPlatforms(head: EngineUnderTest, oracle: OracleUnderTest, platform: Platform): void {
  if (head.manifest.platform !== platform) throw new StepError(`the head engine is for ${head.manifest.platform}, this is ${platform}`)
  if (oracle.manifest.platform !== platform) throw new StepError(`the oracle is for ${oracle.manifest.platform}, this is ${platform}`)
}
