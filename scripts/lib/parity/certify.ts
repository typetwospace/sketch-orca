// parity certify: whether sketch-orca-rs, the Rust translation's program (crates/orca-cli), does
// what the C++ engine does on the corpus, family by family (parity.json). It runs every corpus case
// with the Rust program in the environment a client gives an engine (same.ts runEngineCases, the
// cases' folder <work>/parity/runs/certify/cases), and compares each output of each run with the
// goldens (<work>/parity/goldens, the oracle's one-thread pass), each side's paths written as
// <case>, <cases>, <resources> and <work> (record.ts portableRoots), and the engine block written
// alike (normaliseEngine: the program's version and its `implementation` are not compared):
//   - a result (a file the golden run wrote as sketch-orca-result/1) as a JSON value: the same
//     members in the same order with the same values, the masks (masks.json) applied when not;
//   - every other output byte for byte, the masks applied (masks.ts compareOutput);
//   - an output record.json classes R (the shipped engine differs between its own runs) within
//     the envelope of that variance (<work>/parity/envelope/envelope.json): its other lines the
//     same, and each number of a varying line within the least and greatest of the five runs.
// A run sketch-orca-rs declines (exit -8, the low byte 248; result not-ported with the families it
// is missing) counts for the front families alone (FRONT_FAMILIES: they are decided before a job's
// work starts), and only when the C++ run got past reading the job (its progress has a stage after
// `job`), the program wrote nothing the C++ run did not, and no family it misses is a front family
// or certified. A family is certified when every run of every case of it passes. parity.json's
// flags are raised, never lowered; --check writes no parity.json and fails on a regression (a
// family parity.json certifies that does not pass now). Before any of it, the oracle staged in the
// work folder must be the one this checkout makes (cli/ included), parity neutral must have passed
// with it and the goldens must have been recorded with it (oracleChainProblems), so a change to
// cli/ cannot reach certify without parity oracle, neutral and record run again. Apart from the
// families, the decline itself is held to the corpus (declineGaps): every run of a corpus job that
// reaches a family parity.json does not certify (the corpus's families for it, corpus.ts
// familiesFor) must be declined, naming each such family (crates/orca-cli/src/decline.rs), so the
// program never does a step that is not certified; a gap fails certify and certify --check.

import { existsSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { listFiles, nativePath } from '../files.ts'
import type { Platform } from '../env.ts'
import { lineShape, type EnvelopeReport, type OutputEnvelope } from './envelope.ts'
import { portableBytes, type OutputClass } from './classes.ts'
import { compareOutput, type Mask, type OutputStatus } from './masks.ts'
import type { NeutralReport } from './neutral.ts'
import { maskName, outputBytes, outputNames, portableRoots, type FamilyEntry, type GoldensIndex, type ParityDocument } from './record.ts'
import { runStore, type CaseRecord, type RecordedRun } from './same.ts'

export const CERTIFY_FORMAT = 'sketch-orca-parity-certify/1'

/** The Rust program's name (crates/orca-cli's binary). */
export const RUST_PROGRAM = 'sketch-orca-rs'

/** The families decided before a job's work starts: a decline cannot be one of theirs. */
export const FRONT_FAMILIES = ['version', 'config-def', 'intake'] as const

/** The code and exit code of a decline (crates/orca-cli/src/decline.rs; protocol/README.md). */
export const NOT_PORTED = 'not-ported'
export const NOT_PORTED_EXIT = -8

/** The program `bun run rs:build` makes: <repository>/target/debug/sketch-orca-rs(.exe). */
export function rustProgramPath(engineDir: string, platform: Platform): string {
  return join(engineDir, 'target', 'debug', platform === 'win32' ? `${RUST_PROGRAM}.exe` : RUST_PROGRAM)
}

/**
 * What the program is built from: each crate's Cargo.toml, build.rs and src/, the workspace's
 * Cargo files and the toolchain's pins (a crate's tests/ and README are not).
 */
export function rustSources(engineDir: string): string[] {
  const out = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', join('.cargo', 'config.toml')].map((f) => join(engineDir, f)).filter((f) => existsSync(f))
  const crates = join(engineDir, 'crates')
  const built = (rel: string) => /^[^/]+\/(Cargo\.toml|build\.rs|src\/.+)$/.test(rel)
  if (existsSync(crates)) for (const rel of listFiles(crates, (r, isDir) => !isDir || /^[^/]+(\/src(\/.*)?)?$/.test(r))) if (built(rel)) out.push(nativePath(crates, rel))
  return out
}

/** The sources changed after the program was built (none when it is up to date). parity.json is not one: certify itself writes it. */
export function staleSources(exe: string, sources: string[]): string[] {
  const built = statSync(exe).mtimeMs
  return sources.filter((f) => statSync(f).mtimeMs > built)
}

/** The engine block made alike: its version a placeholder, the Rust program's `implementation` gone. */
export function normaliseEngine(text: string): string {
  return text.replace(/,\s*"implementation"\s*:\s*"[^"\\]*"/g, '').replace(/("engineVersion"\s*:\s*)"[^"\\]*"/g, '$1"<engineVersion>"')
}

function normaliseBytes(b: Buffer | undefined): Buffer | undefined {
  if (b === undefined || b.includes(0) || (b.length >= 4 && b.readUInt32LE(0) === 0x04034b50)) return b
  return Buffer.from(normaliseEngine(b.toString('latin1')), 'latin1')
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

/** The value of a JSON text, or undefined when it is not JSON. */
function jsonOf(b: Buffer | undefined): unknown {
  if (b === undefined) return undefined
  try {
    return JSON.parse(b.toString('utf8')) as unknown
  } catch {
    return undefined
  }
}

/** Whether the bytes are a result (sketch-orca-result/1). */
export function isResult(b: Buffer | undefined): boolean {
  const v = jsonOf(b)
  return isRecord(v) && v.format === 'sketch-orca-result/1'
}

/** Two JSON values the same: the same members in the same order, the same items, the same values. */
export function sameJsonValue(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((x, i) => sameJsonValue(x, b[i]))
  if (isRecord(a) || isRecord(b)) {
    if (!isRecord(a) || !isRecord(b)) return false
    const ka = Object.keys(a)
    const kb = Object.keys(b)
    return ka.length === kb.length && ka.every((k, i) => k === kb[i] && sameJsonValue(a[k], b[k]))
  }
  return a === b
}

/** Whether `rust` is within the run-to-run variance `env` recorded for the golden's output. */
export function withinEnvelope(golden: Buffer, rust: Buffer, env: OutputEnvelope | undefined): { ok: boolean; detail?: string } {
  if (golden.equals(rust)) return { ok: true }
  if (!env) return { ok: false, detail: 'classed R, but the envelope has no entry for it; run parity envelope' }
  if (env.kind !== 'text') return { ok: false, detail: `classed R, but its envelope is ${env.kind}: only text is compared within an envelope` }
  if ((env.varyingLines ?? 0) > (env.lines?.length ?? 0)) return { ok: false, detail: `its envelope holds ${env.lines?.length ?? 0} of ${env.varyingLines} varying lines` }
  const g = golden.toString('utf8').split(/\r?\n/)
  const r = rust.toString('utf8').split(/\r?\n/)
  if (!(env.lineCounts ?? [g.length]).includes(r.length)) return { ok: false, detail: `${r.length} lines, the runs had ${[...new Set(env.lineCounts)].join(' or ')}` }
  const varying = new Map((env.lines ?? []).map((l) => [l.line, l]))
  for (let i = 0; i < r.length; i++) {
    if (g[i] === r[i]) continue
    const v = varying.get(i + 1)
    if (!v) return { ok: false, detail: `line ${i + 1} differs, and it does not vary between the engine's runs` }
    if (v.texts) continue
    const gs = lineShape(g[i] ?? '')
    const rs = lineShape(r[i] ?? '')
    if (gs.skeleton !== rs.skeleton || gs.numbers.length !== rs.numbers.length) return { ok: false, detail: `line ${i + 1}: the text around its numbers differs` }
    for (let f = 0; f < rs.numbers.length; f++) {
      const spread = v.numbers?.find((n) => n.field === f)
      const x = rs.numbers[f]!
      if (spread ? x < spread.min || x > spread.max : x !== gs.numbers[f]) {
        return { ok: false, detail: `line ${i + 1}, number ${f + 1}: ${x} is outside ${spread ? `${spread.min}..${spread.max}` : `the steady ${gs.numbers[f]}`}` }
      }
    }
  }
  return { ok: true }
}

/** One side of a comparison: where its runs are kept, and the paths its outputs are written with. */
export interface Side {
  store: string
  root: string
  resources: string
  work: string
}

export interface OutputVerdict {
  output: string
  status: OutputStatus | 'within-envelope'
  detail?: string
}

/** The bytes of an output of run `run` of case `caseId`, its paths portable and its engine block alike. */
function sideBytes(side: Side, caseId: string, run: RecordedRun | undefined, name: string): Buffer | undefined {
  return normaliseBytes(portableBytes(outputBytes(side.store, caseId, run, name), portableRoots({ root: side.root, caseId, resources: side.resources, work: side.work })))
}

/** Every output of a run of the Rust program against the golden run (see the top of this file). */
export function compareWithGolden(o: {
  caseId: string
  golden: { side: Side; run: RecordedRun | undefined }
  rust: { side: Side; run: RecordedRun | undefined }
  masks: Mask[]
  classOf: (output: string) => OutputClass | undefined
  envelopeOf: (output: string) => OutputEnvelope | undefined
}): OutputVerdict[] {
  const out: OutputVerdict[] = []
  for (const name of outputNames([o.golden.run, o.rust.run])) {
    const a = sideBytes(o.golden.side, o.caseId, o.golden.run, name)
    const b = sideBytes(o.rust.side, o.caseId, o.rust.run, name)
    if (a === undefined && b === undefined) continue
    if (a !== undefined && b !== undefined && a.equals(b)) {
      out.push({ output: name, status: 'identical' })
      continue
    }
    if (a !== undefined && b !== undefined && isResult(a) && sameJsonValue(jsonOf(a), jsonOf(b))) {
      out.push({ output: name, status: 'identical' })
      continue
    }
    const c = compareOutput(maskName(name), a, b, o.masks, ['C++', 'Rust'])
    if (c.status !== 'different' || a === undefined || b === undefined || o.classOf(name) !== 'R') {
      out.push({ output: name, status: c.status, ...(c.detail ? { detail: c.detail } : {}) })
      continue
    }
    const e = withinEnvelope(a, b, o.envelopeOf(name))
    out.push(e.ok ? { output: name, status: 'within-envelope' } : { output: name, status: 'different', detail: e.detail ?? 'outside the envelope' })
  }
  return out
}

/** The stages a progress file names, in its order. */
export function progressStages(b: Buffer | undefined): string[] {
  if (b === undefined) return []
  const out: string[] = []
  for (const line of b.toString('utf8').split(/\r?\n/)) {
    try {
      const v = JSON.parse(line) as unknown
      if (isRecord(v) && typeof v.stage === 'string') out.push(v.stage)
    } catch {
      // Not a line of JSON.
    }
  }
  return out
}

/**
 * Why a run the Rust program declined does not count as passing for the front families; empty
 * when it does (see the top of this file). `families` is parity.json's.
 */
export function declineProblems(o: { caseId: string; golden: { side: Side; run: RecordedRun | undefined }; rust: { side: Side; run: RecordedRun | undefined }; families: FamilyEntry[] }): string[] {
  const g = o.golden.run
  const r = o.rust.run
  if (!g || !r) return [`only the ${g ? 'C++' : 'Rust'} program ran it`]
  const problems: string[] = []
  const exitByte = r.code === null ? null : ((r.code % 256) + 256) % 256
  if (r.timedOut || exitByte !== ((NOT_PORTED_EXIT % 256) + 256) % 256) problems.push(`it exited with ${r.timedOut ? 'a timeout' : r.code}, not ${NOT_PORTED_EXIT}`)
  const results = r.files.filter((f) => isResult(outputBytes(o.rust.side.store, o.caseId, r, `files/${f}`)))
  if (results.length !== 1) problems.push(`it wrote ${results.length} results`)
  for (const f of results) {
    const v = jsonOf(outputBytes(o.rust.side.store, o.caseId, r, `files/${f}`)) as Record<string, unknown>
    if (v.code !== NOT_PORTED || v.ok !== false || v.exitCode !== NOT_PORTED_EXIT) problems.push(`its result is ${JSON.stringify(v.code)} with exit code ${JSON.stringify(v.exitCode)}, not ${NOT_PORTED}`)
    const missing = Array.isArray(v.missing) ? v.missing : []
    if (missing.length === 0 || !missing.every((m) => typeof m === 'string')) problems.push('its result names no missing families')
    for (const m of missing) {
      const f2 = o.families.find((x) => x.name === m)
      if (!f2) problems.push(`it is missing ${String(m)}, which parity.json does not list`)
      else if (f2.certified) problems.push(`it is missing ${f2.name}, which parity.json certifies`)
      if ((FRONT_FAMILIES as readonly string[]).includes(String(m))) problems.push(`it is missing ${String(m)}, a front family`)
    }
  }
  for (const f of r.files) if (!g.files.includes(f)) problems.push(`it wrote ${f}, which the C++ run did not`)
  for (const f of r.gone) if (!g.gone.includes(f)) problems.push(`it removed ${f}, which the C++ run did not`)
  for (const s of ['stdout', 'stderr']) if ((outputBytes(o.rust.side.store, o.caseId, r, s)?.length ?? 0) > 0) problems.push(`it wrote to ${s}`)
  const progress = g.files.filter((f) => /(^|\/)progress\.ndjson$/.test(f))
  const past = progress.some((f) => progressStages(outputBytes(o.golden.side.store, o.caseId, g, `files/${f}`)).some((s) => !['job', 'failed', 'done'].includes(s)))
  if (!past) problems.push('the C++ run shows no stage past reading the job (or no progress file), so the job should have been refused, not declined')
  return problems
}

export interface RunVerdict {
  run: number
  passed: boolean
  /** The Rust program declined it, and the decline is a pass for the front families. */
  declined: boolean
  problems: string[]
}

export interface CaseVerdict {
  id: string
  runs: RunVerdict[]
}

/** Every run of a case, the golden's against the Rust program's. */
export function compareCase(o: {
  caseId: string
  golden: { side: Side; record: CaseRecord | undefined }
  rust: { side: Side; record: CaseRecord | undefined }
  masks: Mask[]
  families: FamilyEntry[]
  classOf: (run: number, output: string) => OutputClass | undefined
  envelopeOf: (run: number, output: string) => OutputEnvelope | undefined
}): CaseVerdict {
  const runs: RunVerdict[] = []
  if (!o.golden.record) return { id: o.caseId, runs: [{ run: -1, passed: false, declined: false, problems: ['the goldens do not have this case; run parity record'] }] }
  if (!o.rust.record) return { id: o.caseId, runs: [{ run: -1, passed: false, declined: false, problems: ['the Rust program did not run this case'] }] }
  const n = Math.max(o.golden.record.runs.length, o.rust.record.runs.length)
  for (let i = 0; i < n; i++) {
    const g = { side: o.golden.side, run: o.golden.record.runs[i] }
    const r = { side: o.rust.side, run: o.rust.record.runs[i] }
    if (!g.run || !r.run) {
      runs.push({ run: i, passed: false, declined: false, problems: [`only the ${g.run ? 'C++' : 'Rust'} program ran it`] })
      continue
    }
    const args = (x: RecordedRun) => JSON.stringify([x.args, x.extraEnv])
    if (args(g.run) !== args(r.run)) {
      runs.push({ run: i, passed: false, declined: false, problems: [`the runs differ: ${args(g.run)} against ${args(r.run)}`] })
      continue
    }
    const verdicts = compareWithGolden({ caseId: o.caseId, golden: g, rust: r, masks: o.masks, classOf: (out) => o.classOf(i, out), envelopeOf: (out) => o.envelopeOf(i, out) })
    const bad = verdicts.filter((v) => v.status === 'different')
    if (bad.length === 0) {
      runs.push({ run: i, passed: true, declined: false, problems: [] })
      continue
    }
    const declined = r.run.code !== null && ((r.run.code % 256) + 256) % 256 === ((NOT_PORTED_EXIT % 256) + 256) % 256
    const dp = declined ? declineProblems({ caseId: o.caseId, golden: g, rust: r, families: o.families }) : []
    if (declined && dp.length === 0) runs.push({ run: i, passed: false, declined: true, problems: [] })
    else runs.push({ run: i, passed: false, declined: false, problems: [...dp, ...bad.map((v) => `${v.output}: ${v.detail ?? 'differs'}`)] })
  }
  return { id: o.caseId, runs }
}

/**
 * Where the decline lets a corpus job through: each run of the Rust program, of a job whose corpus
 * families (`reaches`) include one parity.json does not certify (front families aside), that is
 * not a not-ported result naming each such family, as `<case>/<run>: what`. Empty when every run
 * is declined so.
 */
export function declineGaps(o: { caseId: string; reaches: string[]; families: FamilyEntry[]; rust: { side: Side; record: CaseRecord | undefined } }): string[] {
  const front = FRONT_FAMILIES as readonly string[]
  const uncertified = o.reaches.filter((f) => !front.includes(f) && o.families.find((x) => x.name === f)?.certified !== true)
  if (uncertified.length === 0) return []
  if (!o.rust.record || o.rust.record.runs.length === 0) return [`${o.caseId}: the Rust program did not run it`]
  const out: string[] = []
  o.rust.record.runs.forEach((r, i) => {
    const exitByte = r.code === null ? null : ((r.code % 256) + 256) % 256
    const results = r.files.map((f) => outputBytes(o.rust.side.store, o.caseId, r, `files/${f}`)).filter((b) => isResult(b))
    const v = results.length === 1 ? (jsonOf(results[0]) as Record<string, unknown>) : undefined
    if (r.timedOut || exitByte !== ((NOT_PORTED_EXIT % 256) + 256) % 256 || !v || v.code !== NOT_PORTED) {
      out.push(`${o.caseId}/${i}: it reaches ${uncertified.join(', ')}, which parity.json does not certify, and was not declined (exit ${r.timedOut ? 'timeout' : String(r.code)}, result ${JSON.stringify(v?.code ?? null)})`)
      return
    }
    const missing = Array.isArray(v.missing) ? v.missing.map(String) : []
    const unnamed = uncertified.filter((f) => !missing.includes(f))
    if (unnamed.length > 0) out.push(`${o.caseId}/${i}: its decline does not name ${unnamed.join(', ')}, which the job reaches and parity.json does not certify`)
  })
  return out
}

export interface FamilyVerdict {
  name: string
  cases: number
  runs: number
  passed: boolean
  declined: number
  certifiedBefore: boolean
  certified: boolean
  /** The first problems, as <case>/<run>: what. */
  problems: string[]
}

/** Each family's verdict from its cases': a front family counts the runs declined as passing. */
export function familyVerdicts(parity: ParityDocument, cases: Map<string, CaseVerdict>, maxProblems = 20): FamilyVerdict[] {
  return parity.families.map((f) => {
    const front = (FRONT_FAMILIES as readonly string[]).includes(f.name)
    let runs = 0
    let declined = 0
    const problems: string[] = []
    let passed = f.cases.length > 0
    for (const id of f.cases) {
      const c = cases.get(id)
      if (!c) {
        passed = false
        if (problems.length < maxProblems) problems.push(`${id}: not run`)
        continue
      }
      for (const r of c.runs) {
        runs++
        if (r.declined) declined++
        const ok = r.passed || (r.declined && front)
        if (!ok) {
          passed = false
          if (problems.length < maxProblems) problems.push(`${id}/${r.run}: ${r.declined ? 'declined (not-ported)' : r.problems.slice(0, 3).join('; ')}`)
        }
      }
    }
    return { name: f.name, cases: f.cases.length, runs, passed, declined, certifiedBefore: f.certified, certified: f.certified || passed, problems }
  })
}

/** parity.json with the families that passed certified (never one lowered), and what changed. */
export function raiseCertified(parity: ParityDocument, verdicts: FamilyVerdict[]): { parity: ParityDocument; raised: string[]; regressions: string[] } {
  const by = new Map(verdicts.map((v) => [v.name, v]))
  const raised = verdicts.filter((v) => v.passed && !v.certifiedBefore).map((v) => v.name)
  const regressions = verdicts.filter((v) => v.certifiedBefore && !v.passed).map((v) => v.name)
  return { parity: { ...parity, families: parity.families.map((f) => ({ ...f, certified: f.certified || (by.get(f.name)?.passed ?? false) })) }, raised, regressions }
}

export interface CertifyReport {
  format: typeof CERTIFY_FORMAT
  createdAt: string
  check: boolean
  program: { path: string; sha256: string }
  corpusSha256: string
  goldens: string
  summary: { families: number; passed: number; certified: number; raised: string[]; regressions: string[] }
  families: FamilyVerdict[]
  /** Where the decline let a corpus job through (declineGaps); certify fails on any. */
  declineGaps: string[]
}

/**
 * Why the goldens cannot be held against this checkout's cli/: the chain CONTRIBUTING.md asks for
 * after a change to cli/ or the patches (parity oracle, then parity neutral, then parity record)
 * must have run for the oracle staged now. The caller has already checked that oracle against this
 * checkout (oracle.ts oracleUnderTest: the commit, patches/, the oracle patches and cli/); here
 * parity neutral must have passed with that very oracle, and the goldens must have been recorded
 * with it. Empty when they were.
 */
export function oracleChainProblems(o: { oracle: { exeSha256: string; cli: string }; neutral: Pick<NeutralReport, 'passed' | 'engines'> | undefined; goldens: Pick<GoldensIndex, 'oracle'> }): string[] {
  const out: string[] = []
  if (!o.neutral) out.push('parity neutral has not run (no neutral.json); run parity neutral')
  else if (o.neutral.engines.oracle.exeSha256 !== o.oracle.exeSha256) out.push('parity neutral last ran with another oracle than the one staged; run parity neutral')
  else if (!o.neutral.passed) out.push('parity neutral did not pass with the staged oracle')
  if (!o.goldens.oracle) out.push('the goldens do not name the oracle they were recorded with (recorded before goldens.json named it); run parity record')
  else if (o.goldens.oracle.exeSha256 !== o.oracle.exeSha256 || o.goldens.oracle.cli !== o.oracle.cli) out.push('the goldens were recorded with another oracle than the one staged (another cli/, patches or build); run parity record')
  return out
}

/** <work>/parity/runs/certify (the cases' folder and the Rust program's runs) and the report. */
export function certifyFiles(paths: { parity: string; runs: string }, check: boolean): { root: string; store: string; report: string } {
  const dir = join(paths.runs, check ? 'certify-check' : 'certify')
  return { root: join(dir, 'cases'), store: join(dir, 'rust'), report: join(paths.parity, check ? 'certify-check.json' : 'certify.json') }
}

/** record.json's class of each output, by case, run and output. */
export function classIndex(outputs: { case: string; run: number; output: string; class: OutputClass }[]): (caseId: string, run: number, output: string) => OutputClass | undefined {
  const m = new Map(outputs.map((o) => [`${o.case}\u0000${o.run}\u0000${o.output}`, o.class]))
  return (c, r, out) => m.get(`${c}\u0000${r}\u0000${out}`)
}

/** The envelope's entry of each output, by case, run and output. */
export function envelopeIndex(report: EnvelopeReport | undefined): (caseId: string, run: number, output: string) => OutputEnvelope | undefined {
  const m = new Map((report?.outputs ?? []).map((o) => [`${o.case}\u0000${o.run}\u0000${o.output}`, o]))
  return (c, r, out) => m.get(`${c}\u0000${r}\u0000${out}`)
}

/** The runs of the store as a pass keeps them (runStore), for a reader that wants the folder. */
export function runFolder(side: Side, caseId: string, run: number): string {
  return runStore(join(side.store, caseId), run).dir
}

export function readJsonIf<T>(path: string): T | undefined {
  return existsSync(path) ? (JSON.parse(readFileSync(path, 'utf8')) as T) : undefined
}
