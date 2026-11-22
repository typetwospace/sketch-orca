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
// A family is certified when every run of every case of it passes, its outputs the golden's. A run
// sketch-orca-rs declines (exit -8, the low byte 248; result not-ported with the families it is
// missing) never passes, in any family, the front families (FRONT_FAMILIES) too: the decline is
// information (RunVerdict.declined), so a certified family is the C++ engine's outputs in every
// run. A decline is itself wrong, a problem of its run, when the C++ run ended within what the
// program does itself (endedWithinRust: its progress shows no stage past RUST_STAGES (the job, the
// presets, the model and arrange), ending with its failed or done line when it stopped in model or arrange), as
// the program should then have done what the C++ run did; when it names a missing family that is
// a front family, certified, or not in parity.json; when it wrote a file the C++ run did not; and
// when it wrote to stdout or stderr anything but what the C++ run wrote there up to that point
// (streamPrefixProblem): the decline's stream must be, line by line, the first lines of the C++
// run's, each line whole with its end, each side's paths portable and the masks that name the
// stream applied. The C++ run went on past the decline and may have written more; the decline may
// not write a byte it did not. The rule judges the two streams only: a decline's exit code and
// progress, which differ from a C++ run that went on, stay with the rules here and below. A stream
// the decline left empty always passes the rule, so a decline that passes a family by its boundary
// is not held to the lines the C++ wrote to stderr before the boundary; the runs the program
// answers whole (every info case) and the rs-* smoke cases of stage hold those lines. A
// decline that names preset-conditions (a compatibility condition the program cannot work out yet,
// in the presets stage) needs the C++ run to have reached the presets. parity.json's flags are
// raised, never lowered; --check writes no parity.json and fails on a regression (a family
// parity.json certifies that does not pass now). Before any of it, the
// oracle staged in the work folder must be the one this checkout makes (cli/ included), parity
// neutral must have passed with it and the goldens must have been recorded with it
// (oracleChainProblems), so a change to cli/ cannot reach certify without parity oracle, neutral
// and record run again. Apart from the families, the decline itself is held to the corpus
// (declineGaps): every run of a corpus job, example or refusal (DECLINE_HELD_KINDS; a smoke case's
// families are not worked out for each of its runs) that reaches a family parity.json does not
// certify (the corpus's families for it, corpus.ts) must be declined, naming each such family
// (crates/orca-cli/src/decline.rs), unless its C++ run ended within what the program does itself,
// so the program never does a step that is not certified; a gap fails certify and certify --check.
// The families are held against parity.json as this very run raises it, so a family certified here
// needs no decline in the same run.
//
// The dumps (record.ts KEPT_DUMPS). The program runs with the golden pass's knobs (time and seed
// pinned, one thread) and SKETCH_ORCA_ORACLE_DUMP set to a fresh folder per run, as the oracle did,
// and each dump it keeps is compared with the golden run's byte for byte, each side's paths portable
// inside its strings (neutral.ts portableDump). A run that is answered must write the golden's dumps,
// each the same, and no other. A declined run must write no dump the C++ run did not, nor one that
// differs from it, and the C++ run's dumps after its decline are not compared (it never got there).
// The boundaries are in the order Print::process() writes them (BOUNDARY_FAMILIES: front-objects,
// front-config, the object steps and the print steps up to the end of process(),
// print-psConflictCheck). Where it wrote every dump the C++ run wrote of a boundary and of every
// boundary before it, each the same (boundariesReached), and its progress lines (its final failed
// line aside) are the C++ run's first lines, the decline passes the families wholly before that
// boundary (front-objects passes preset-conditions, model-load and placement; the end of process()
// every family process() runs in domain A, but layers, which no boundary holds), in
// RunVerdict.boundaries and the families' boundaryPasses. It passes no other family. A golden dump
// the goldens keep as its SHA-256 alone (record.ts keepGoldens) is compared by that hash. Goldens
// recorded before dumps were kept (their runs name no dumps) are compared without them, and no
// decline passes a family against them.
//
// What the declines wrote (J11). A decline's empty stream proves no line the C++ writes before its
// boundary, so the report shows what the declines wrote (declineStreams: each declined run's stderr
// and stdout as both empty, the program's empty, a line prefix of the C++ run's, or not a prefix) and
// which answered runs hold each family's lines (preBoundaryHolders). Neither changes a verdict or
// the exit status.
//
// The runs after a decline (goldenCarry). A smoke case may make a later run from an earlier run's
// outputs: pocket-pause slices at the print Z its layers run gave, imported-settings imports the
// .gcode.3mf its export run wrote. A declined run made none of them, so the case would stop there
// and its later runs would be the C++ program's alone. certify runs the cases with goldenCarry
// instead: once a run the program declined (exit -8, not timed out) is kept, and its golden run had
// the same command line and added variables, the case's folder is given the golden run's outputs in
// place of the decline's (each file the C++ run wrote, its golden paths swapped for this pass's,
// rebaseBytes; the files it removed removed; a file only the decline made removed), and the case is
// told the C++ run's exit code and stdout. The case goes on as it did for the C++ program, and its
// later runs get the jobs and the inputs the C++ program got. Nothing is passed by this: the declined
// run is judged as it was kept, before the swap, and each later run is the program's own, judged as
// any other (its outputs, its dumps and the decline rules). A run that is not declined is never
// carried, and a decline whose golden run differs is not either. The report lists the carried runs
// (CertifyReport.carried).

import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { listFiles, nativePath, sha256Bytes } from '../files.ts'
import type { Platform } from '../env.ts'
import { lineShape, type EnvelopeReport, type OutputEnvelope } from './envelope.ts'
import { portableBytes, type OutputClass } from './classes.ts'
import { compareOutput, masksFor, type Mask, type OutputStatus } from './masks.ts'
import type { NeutralReport } from './neutral.ts'
import { readSop1, sop1Differences } from './neutral.ts'
import { dumpBoundary, dumpNames, dumpSha256, maskName, outputBytes, outputNames, portableDumpBytes, portableRoots, type FamilyEntry, type GoldensIndex, type ParityDocument } from './record.ts'
import { type AfterRun, type CaseRecord, type RecordedRun } from './same.ts'
import { endedWithinRust, lowByte, RUST_STAGES } from '../smoke.ts'

export const CERTIFY_FORMAT = 'sketch-orca-parity-certify/1'

/** The Rust program's name (crates/orca-cli's binary). */
export const RUST_PROGRAM = 'sketch-orca-rs'

/**
 * The front families: what the program does itself before any step it can decline (the version,
 * the config-def, and intake: the command line, the job, the presets stage and the model's file
 * check). A decline cannot name one of them; like every family, they are certified only by runs
 * that pass, never by declines.
 */
export const FRONT_FAMILIES = ['version', 'config-def', 'intake'] as const

// What the program does itself and where a C++ run ended within it (RUST_STAGES, endedWithinRust)
// are smoke.ts's, which the stage's rs-intake case holds the program to as well.
export { endedWithinRust, RUST_STAGES }

// The families wholly before each boundary, each list holding the one before it.
const FRONT_OBJECTS = ['preset-conditions', 'model-load', 'placement']
const SLICED = [...FRONT_OBJECTS, 'slice-mesh']
const WALLED = [...SLICED, 'perimeters-classic', 'perimeters-arachne']
// The curled extrusions run for every object after the walls and before the infill; their lines are in each layer's LEXT.
const SURFACED = [...WALLED, 'step-estimate-curled-extrusions', 'step-prepare-infill']
const FILLED = [...SURFACED, 'infill-monotonic', 'infill-monotonicline', 'infill-rectilinear', 'infill-crosshatch', 'infill-concentric', 'infill-concentricinternal']
const SUPPORT_CHECKED = [...FILLED, 'support-check']
const LIFTED = [...SUPPORT_CHECKED, 'step-detect-overhangs-for-lift']
const TOOL_ORDERED = [...LIFTED, 'tool-ordering']
const BRIMMED = [...TOOL_ORDERED, 'step-skirt-brim']
// posSimplifyWall is never set done: the walls are simplified with the paths (PrintObject::simplify_extrusion_path).
const PATHS_SIMPLIFIED = [...BRIMMED, 'step-simplify-path', 'step-simplify-wall']
const INFILL_SIMPLIFIED = [...PATHS_SIMPLIFIED, 'step-simplify-infill']
const PROCESSED = [...INFILL_SIMPLIFIED, 'step-conflict-check']

/**
 * The families a declined run passes when it reached a boundary (see the top of this file): those
 * wholly before the boundary's dump, one list per boundary of record.ts KEPT_DUMPS, in the order
 * Print::process() writes them (boundariesReached reads this order). Each list holds the one
 * before it. The end of process(), print-psConflictCheck, holds every family process() runs in
 * domain A (corpus.ts O3_FAMILIES but layers); no boundary holds layers, which cli/run.cpp writes
 * after process() returns, nor a marker of a job outside domain A (corpus.ts MARKER_FAMILIES),
 * which is declined at the placement stop.
 */
export const BOUNDARY_FAMILIES: Record<string, readonly string[]> = {
  'front-objects': FRONT_OBJECTS,
  'front-config': FRONT_OBJECTS,
  'object-posSlice': SLICED,
  'object-posPerimeters': WALLED,
  'object-posEstimateCurledExtrusions': WALLED,
  'object-posPrepareInfill': SURFACED,
  'object-posInfill': FILLED,
  'object-posIroning': FILLED,
  'object-posContouring': FILLED,
  'object-posSupportMaterial': SUPPORT_CHECKED,
  'object-posDetectOverhangsForLift': LIFTED,
  'print-psWipeTower': TOOL_ORDERED,
  'print-psSkirtBrim': BRIMMED,
  'object-posSimplifyPath': PATHS_SIMPLIFIED,
  'object-posSimplifyInfill': INFILL_SIMPLIFIED,
  'object-posSimplifySupportPath': INFILL_SIMPLIFIED,
  'print-psConflictCheck': PROCESSED,
}

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

/** Where a run ended: the stages its progress file names (none without one), and its result's code. */
export function runEnd(side: Side, caseId: string, run: RecordedRun): { stages: string[]; code: string | undefined } {
  const progress = run.files.find((f) => /(^|\/)progress\.ndjson$/.test(f))
  const stages = progress === undefined ? [] : progressStages(outputBytes(side.store, caseId, run, `files/${progress}`))
  let code: string | undefined
  for (const f of run.files) {
    const v = jsonOf(outputBytes(side.store, caseId, run, `files/${f}`))
    if (isRecord(v) && v.format === 'sketch-orca-result/1' && typeof v.code === 'string') code = v.code
  }
  return { stages, code }
}

/** A text's lines, each with its end (LF, or CR LF), a last line without one as it is. */
function linesWithEnds(text: string): string[] {
  return text.match(/[^\n]*\n|[^\n]+$/g) ?? []
}

/** `text` with the text masks that name `key` applied, as compareOutput applies them (masks.ts). */
function maskedText(text: string, masks: Mask[], key: string): string {
  let out = text
  for (const m of masksFor(masks, key)) if (m.kind === 'text') out = out.replace(new RegExp(m.pattern, 'gm'), m.replace)
  return out
}

/**
 * Why what a declined run wrote to a stream (stdout or stderr) is not what the C++ run wrote there
 * up to the decline; undefined when it is (see the top of this file). `golden` and `rust` are the
 * two streams, each side's paths portable. The masks that name the stream are applied to both;
 * then each line of the Rust stream, whole and with its end, must be the C++ stream's line at the
 * same place. A stream the decline left empty always is.
 */
export function streamPrefixProblem(name: 'stdout' | 'stderr', golden: Buffer | undefined, rust: Buffer | undefined, masks: Mask[]): string | undefined {
  const lines = (b: Buffer | undefined) => linesWithEnds(maskedText((b ?? Buffer.alloc(0)).toString('latin1'), masks, name))
  const r = lines(rust)
  const g = lines(golden)
  const show = (s: string | undefined) => (s === undefined ? 'no line' : JSON.stringify(s.slice(0, 160)))
  for (let i = 0; i < r.length; i++) {
    if (r[i] !== g[i]) return `it wrote to ${name} what the C++ run did not write there (line ${i + 1}: the C++ run's ${show(g[i])}, the Rust run's ${show(r[i])})`
  }
  return undefined
}

/**
 * Why a decline of the Rust program is wrong; empty when it is right (see the top of this file).
 * A right decline is information, never a pass. `families` is parity.json's; `masks` are those
 * the streams are compared with.
 */
export function declineProblems(o: { caseId: string; golden: { side: Side; run: RecordedRun | undefined }; rust: { side: Side; run: RecordedRun | undefined }; families: FamilyEntry[]; masks: Mask[] }): string[] {
  const g = o.golden.run
  const r = o.rust.run
  if (!g || !r) return [`only the ${g ? 'C++' : 'Rust'} program ran it`]
  const problems: string[] = []
  const exitByte = r.code === null ? null : ((r.code % 256) + 256) % 256
  if (r.timedOut || exitByte !== ((NOT_PORTED_EXIT % 256) + 256) % 256) problems.push(`it exited with ${r.timedOut ? 'a timeout' : r.code}, not ${NOT_PORTED_EXIT}`)
  const results = r.files.filter((f) => isResult(outputBytes(o.rust.side.store, o.caseId, r, `files/${f}`)))
  if (results.length !== 1) problems.push(`it wrote ${results.length} results`)
  const named: string[] = []
  for (const f of results) {
    const v = jsonOf(outputBytes(o.rust.side.store, o.caseId, r, `files/${f}`)) as Record<string, unknown>
    if (v.code !== NOT_PORTED || v.ok !== false || v.exitCode !== NOT_PORTED_EXIT) problems.push(`its result is ${JSON.stringify(v.code)} with exit code ${JSON.stringify(v.exitCode)}, not ${NOT_PORTED}`)
    const missing = Array.isArray(v.missing) ? v.missing : []
    if (missing.length === 0 || !missing.every((m) => typeof m === 'string')) problems.push('its result names no missing families')
    for (const m of missing) {
      named.push(String(m))
      const f2 = o.families.find((x) => x.name === m)
      if (!f2) problems.push(`it is missing ${String(m)}, which parity.json does not list`)
      else if (f2.certified) problems.push(`it is missing ${f2.name}, which parity.json certifies`)
      if ((FRONT_FAMILIES as readonly string[]).includes(String(m))) problems.push(`it is missing ${String(m)}, a front family`)
    }
  }
  for (const f of r.files) if (!g.files.includes(f)) problems.push(`it wrote ${f}, which the C++ run did not`)
  for (const f of r.gone) if (!g.gone.includes(f)) problems.push(`it removed ${f}, which the C++ run did not`)
  // The streams: only what the C++ run wrote there up to the decline (streamPrefixProblem).
  for (const s of ['stdout', 'stderr'] as const) {
    const why = streamPrefixProblem(s, sideBytes(o.golden.side, o.caseId, g, s), sideBytes(o.rust.side, o.caseId, r, s), o.masks)
    if (why) problems.push(why)
  }
  const end = runEnd(o.golden.side, o.caseId, g)
  const where = `the C++ run's progress shows ${end.stages.length === 0 ? 'no stage' : end.stages.join(', ')}${end.code === undefined ? '' : `, its result ${end.code}`}`
  if (named.includes('preset-conditions')) {
    if (!end.stages.includes('presets')) problems.push(`it names preset-conditions, a step of the presets, which the C++ run did not reach (${where})`)
  } else if (endedWithinRust(end.stages, end.code)) {
    problems.push(`the C++ run ended within what ${RUST_PROGRAM} does itself (${where}), so it should have done the same, not declined`)
  }
  return problems
}

export interface RunVerdict {
  run: number
  passed: boolean
  /**
   * The Rust program declined it (not-ported) and declineProblems found nothing wrong with the
   * decline. A declined run passes no family but those of the boundaries it reached (`boundaries`).
   */
  declined: boolean
  /**
   * The boundaries a declined run reached: it wrote every dump of the boundary the C++ run wrote,
   * each the same, and its progress lines are the C++ run's first lines. It passes the families of
   * each (BOUNDARY_FAMILIES). Empty for a run that is not declined.
   */
  boundaries: string[]
  problems: string[]
}

/**
 * The kept dumps of a run against the golden run's, each side's paths portable: those that are the
 * same, those the golden run wrote and the Rust run did not, and those the Rust run wrote that the
 * golden run did not or that differ from it (with the records that differ, RUN included).
 */
export function dumpComparison(o: { caseId: string; golden: { side: Side; run: RecordedRun }; rust: { side: Side; run: RecordedRun } }): { same: string[]; notWritten: string[]; wrong: string[] } {
  const out = { same: [] as string[], notWritten: [] as string[], wrong: [] as string[] }
  for (const dump of dumpNames([o.golden.run, o.rust.run])) {
    const a = portableDumpBytes(o.golden.side.store, o.caseId, o.golden.run, dump, o.golden.side)
    const b = portableDumpBytes(o.rust.side.store, o.caseId, o.rust.run, dump, o.rust.side)
    // A golden dump kept as its SHA-256 alone (record.ts keepGoldens, past GOLDENS_MAX_BYTES) is compared by its hash.
    const hash = a === undefined ? dumpSha256(o.golden.side.store, o.caseId, o.golden.run, dump) : undefined
    if (hash !== undefined) {
      if (b === undefined) out.notWritten.push(dump)
      else if (sha256Bytes(b) === hash) out.same.push(dump)
      else out.wrong.push(`dumps/${dump}: differs from the C++ run's (by its SHA-256, which the goldens keep alone)`)
      continue
    }
    if (a !== undefined && b !== undefined && a.equals(b)) out.same.push(dump)
    else if (b === undefined) out.notWritten.push(dump)
    else if (a === undefined) out.wrong.push(`dumps/${dump}: written, which the C++ run did not`)
    else {
      let detail: string
      try {
        const x = readSop1(a)
        const y = readSop1(b)
        const runA = x.records.find((r) => r.kind === 'RUN ')
        const runB = y.records.find((r) => r.kind === 'RUN ')
        const d = sop1Differences(x, y)
        if (runA && runB && !runA.payload.equals(runB.payload)) d.unshift('the RUN record')
        detail = d.length > 0 ? d.slice(0, 5).join('; ') : 'its bytes'
      } catch (err) {
        detail = `unreadable: ${(err as Error).message}`
      }
      out.wrong.push(`dumps/${dump}: differs from the C++ run's (${detail})`)
    }
  }
  return out
}

/** Whether the Rust run's progress lines, its final failed line aside, are the golden run's first lines. */
export function progressIsPrefix(golden: Buffer | undefined, rust: Buffer | undefined): boolean {
  const lines = (b: Buffer | undefined) => (b === undefined ? [] : b.toString('latin1').split('\n').filter((l, i, all) => !(i === all.length - 1 && l === '')))
  const g = lines(golden)
  const r = lines(rust)
  const last = r[r.length - 1]
  if (last !== undefined && progressStages(Buffer.from(last, 'latin1'))[0] === 'failed') r.pop()
  return r.length <= g.length && r.every((l, i) => l === g[i])
}

/**
 * The boundaries a declined run reached (BOUNDARY_FAMILIES' keys, in their order): each boundary of
 * which the golden run wrote a dump, when every golden dump of it and of every boundary before it
 * is among those the Rust run wrote the same (`same`). So a run reaches the end of process() only
 * when every kept dump up to it is the same, the front dumps and every object and print dump of
 * process(); a boundary the golden run wrote no dump of is passed over.
 */
export function boundariesReached(goldenDumps: string[], same: string[]): string[] {
  const out: string[] = []
  for (const b of Object.keys(BOUNDARY_FAMILIES)) {
    const of = goldenDumps.filter((d) => dumpBoundary(d) === b)
    if (!of.every((d) => same.includes(d))) break
    if (of.length > 0) out.push(b)
  }
  return out
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
  if (!o.golden.record) return { id: o.caseId, runs: [{ run: -1, passed: false, declined: false, boundaries: [], problems: ['the goldens do not have this case; run parity record'] }] }
  if (!o.rust.record) return { id: o.caseId, runs: [{ run: -1, passed: false, declined: false, boundaries: [], problems: ['the Rust program did not run this case'] }] }
  const n = Math.max(o.golden.record.runs.length, o.rust.record.runs.length)
  for (let i = 0; i < n; i++) {
    const g = { side: o.golden.side, run: o.golden.record.runs[i] }
    const r = { side: o.rust.side, run: o.rust.record.runs[i] }
    if (!g.run || !r.run) {
      runs.push({ run: i, passed: false, declined: false, boundaries: [], problems: [`only the ${g.run ? 'C++' : 'Rust'} program ran it`] })
      continue
    }
    const args = (x: RecordedRun) => JSON.stringify([x.args, x.extraEnv])
    if (args(g.run) !== args(r.run)) {
      runs.push({ run: i, passed: false, declined: false, boundaries: [], problems: [`the runs differ: ${args(g.run)} against ${args(r.run)}`] })
      continue
    }
    const verdicts = compareWithGolden({ caseId: o.caseId, golden: g, rust: r, masks: o.masks, classOf: (out) => o.classOf(i, out), envelopeOf: (out) => o.envelopeOf(i, out) })
    const bad = verdicts.filter((v) => v.status === 'different')
    // The dumps, when the goldens kept them (a golden run recorded before has no `dumps`).
    const withDumps = g.run.dumps !== undefined
    const dumps = withDumps ? dumpComparison({ caseId: o.caseId, golden: { side: g.side, run: g.run }, rust: { side: r.side, run: r.run } }) : { same: [], notWritten: [], wrong: [] }
    const dumpProblems = [...dumps.notWritten.map((d) => `dumps/${d}: not written, which the C++ run did`), ...dumps.wrong]
    if (bad.length === 0 && dumpProblems.length === 0) {
      runs.push({ run: i, passed: true, declined: false, boundaries: [], problems: [] })
      continue
    }
    const declined = r.run.code !== null && ((r.run.code % 256) + 256) % 256 === ((NOT_PORTED_EXIT % 256) + 256) % 256
    const dp = declined ? [...declineProblems({ caseId: o.caseId, golden: g, rust: r, families: o.families, masks: o.masks }), ...dumps.wrong] : []
    if (declined && dp.length === 0) {
      const progress = (x: { side: Side; run: RecordedRun }) => {
        const f = x.run.files.find((p) => /(^|\/)progress\.ndjson$/.test(p))
        return f === undefined ? undefined : sideBytes(x.side, o.caseId, x.run, `files/${f}`)
      }
      const prefix = progressIsPrefix(progress({ side: g.side, run: g.run }), progress({ side: r.side, run: r.run }))
      const boundaries = withDumps && prefix ? boundariesReached(g.run.dumps ?? [], dumps.same) : []
      runs.push({ run: i, passed: false, declined: true, boundaries, problems: [] })
    } else runs.push({ run: i, passed: false, declined: false, boundaries: [], problems: [...dp, ...(declined ? [] : dumpProblems), ...bad.map((v) => `${v.output}: ${v.detail ?? 'differs'}`)] })
  }
  return { id: o.caseId, runs }
}

/**
 * Bytes written in the golden's cases' folder made bytes of this pass's: each path of `pairs` (a
 * root of the golden's side and the same root of this side) swapped for the other wherever it
 * appears as written, with forward slashes, or JSON-escaped (the forms classes.ts portableText
 * finds), the longest first, in one pass. A zip (a .3mf) or other binary output is left as it is,
 * as portableBytes leaves it.
 */
export function rebaseBytes(b: Buffer, pairs: [string, string][]): Buffer {
  if ((b.length >= 4 && b.readUInt32LE(0) === 0x04034b50) || b.includes(0)) return b
  const asBytes = (s: string) => Buffer.from(s, 'utf8').toString('latin1')
  const forms = (p: string) => [p, p.replace(/\\/g, '/'), p.replace(/\\/g, '\\\\'), p.replace(/\//g, '\\\\')]
  const swap = new Map<string, string>()
  for (const [from, to] of [...pairs].sort((x, y) => y[0].length - x[0].length)) {
    const t = forms(asBytes(to))
    forms(asBytes(from)).forEach((f, i) => {
      if (f !== '' && !swap.has(f)) swap.set(f, t[i]!)
    })
  }
  // Every root stays in the pattern, one swapped for itself too, so a longer one is never cut by a shorter.
  const keys = [...swap.keys()].sort((x, y) => y.length - x.length)
  if (keys.every((k) => swap.get(k) === k)) return b
  const re = new RegExp(keys.map((k) => k.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'), 'g')
  return Buffer.from(b.toString('latin1').replace(re, (m) => swap.get(m)!), 'latin1')
}

/**
 * Where certify goes on past a run the Rust program declined (see the top of this file): the
 * hook runEngineCases calls once each run is kept (same.ts AfterRun). For a decline (the exit code
 * -8, not timed out) whose golden run had the same command line and added variables, it gives the
 * case's folder the golden run's outputs in place of the decline's and answers the case with the
 * C++ run's exit code and stdout, its paths this pass's. For any other run it does nothing.
 * `golden` and `rust` are the two sides; `goldens` the golden pass's cases.
 */
export function goldenCarry(o: { golden: Side; goldens: CaseRecord[]; rust: Side }): AfterRun {
  return ({ caseId, caseDir, run, before }) => {
    if (run.timedOut || lowByte(run.code) !== lowByte(NOT_PORTED_EXIT)) return undefined
    const g = o.goldens.find((c) => c.id === caseId)?.runs[run.index]
    if (!g || JSON.stringify([g.args, g.extraEnv]) !== JSON.stringify([run.args, run.extraEnv])) return undefined
    const roots = (s: Side) => portableRoots({ root: s.root, caseId, resources: s.resources, work: s.work }).map(([p]) => p)
    const to = roots(o.rust)
    const pairs = roots(o.golden).map((p, i): [string, string] => [p, to[i]!])
    // A file only the decline made goes; one it changed that the C++ run did not stays (a problem of the decline already).
    for (const f of run.files) if (!g.files.includes(f) && !before.has(f)) rmSync(nativePath(caseDir, f), { force: true })
    for (const f of g.gone) rmSync(nativePath(caseDir, f), { force: true })
    for (const f of g.files) {
      const at = nativePath(caseDir, f)
      const bytes = outputBytes(o.golden.store, caseId, g, `files/${f}`)!
      rmSync(at, { force: true })
      mkdirSync(dirname(at), { recursive: true })
      writeFileSync(at, rebaseBytes(bytes, pairs))
    }
    const stdout = rebaseBytes(outputBytes(o.golden.store, caseId, g, 'stdout') ?? Buffer.alloc(0), pairs)
    return { code: g.code, timedOut: g.timedOut, stdout: stdout.toString('utf8') }
  }
}

/**
 * The kinds of corpus case the decline is held to (declineGaps): those whose families are worked
 * out for their one job (corpus.ts: a job's from its settings and presets, a refusal's from the
 * steps the C++ engine reaches before it refuses, an example's intake). A smoke case's families
 * are named by hand or counted as a generic slice for the whole case, not for each of its runs
 * (a run on another preset set, or a layers run in a slice case, reaches others), so its runs
 * are held to the goldens alone.
 */
export const DECLINE_HELD_KINDS = ['job', 'example', 'refusal'] as const

/**
 * Where the decline lets a corpus case through: each run of the Rust program, of a case whose
 * corpus families (`reaches`) include one parity.json does not certify (front families aside),
 * that is not a not-ported result naming each such family, as `<case>/<run>: what`. A run that is
 * not declined is no gap when its C++ run (`golden`) ended within what the program does itself
 * (endedWithinRust): the job never reached those families. Empty when every run is so.
 */
export function declineGaps(o: {
  caseId: string
  reaches: string[]
  families: FamilyEntry[]
  rust: { side: Side; record: CaseRecord | undefined }
  golden: { side: Side; record: CaseRecord | undefined }
}): string[] {
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
      const g = o.golden.record?.runs[i]
      if (g) {
        const end = runEnd(o.golden.side, o.caseId, g)
        if (endedWithinRust(end.stages, end.code)) return
      }
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
  /** Its runs the Rust program declined, rightly (those that reached a boundary of it are in boundaryPasses too). */
  declined: number
  /** Its declined runs that passed it by reaching a boundary after it (BOUNDARY_FAMILIES). */
  boundaryPasses: number
  certifiedBefore: boolean
  certified: boolean
  /** The first problems, as <case>/<run>: what. */
  problems: string[]
}

/**
 * Each family's verdict from its cases': it passes when every run of every case of it passed. A
 * declined run is no pass, in any family, the front families too, unless it reached a boundary
 * whose families (BOUNDARY_FAMILIES) include this one; `declined` counts them and `boundaryPasses`
 * those that passed so.
 */
export function familyVerdicts(parity: ParityDocument, cases: Map<string, CaseVerdict>, maxProblems = 20): FamilyVerdict[] {
  return parity.families.map((f) => {
    let runs = 0
    let declined = 0
    let boundaryPasses = 0
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
        const byBoundary = r.declined && (r.boundaries ?? []).some((b) => (BOUNDARY_FAMILIES[b] ?? []).includes(f.name))
        if (byBoundary) boundaryPasses++
        const ok = r.passed || byBoundary
        if (!ok) {
          passed = false
          if (problems.length < maxProblems) problems.push(`${id}/${r.run}: ${r.declined ? 'declined (not-ported)' : r.problems.slice(0, 3).join('; ')}`)
        }
      }
    }
    return { name: f.name, cases: f.cases.length, runs, passed, declined, boundaryPasses, certifiedBefore: f.certified, certified: f.certified || passed, problems }
  })
}

/** parity.json with the families that passed certified (never one lowered), and what changed. */
export function raiseCertified(parity: ParityDocument, verdicts: FamilyVerdict[]): { parity: ParityDocument; raised: string[]; regressions: string[] } {
  const by = new Map(verdicts.map((v) => [v.name, v]))
  const raised = verdicts.filter((v) => v.passed && !v.certifiedBefore).map((v) => v.name)
  const regressions = verdicts.filter((v) => v.certifiedBefore && !v.passed).map((v) => v.name)
  return { parity: { ...parity, families: parity.families.map((f) => ({ ...f, certified: f.certified || (by.get(f.name)?.passed ?? false) })) }, raised, regressions }
}

// ---------------------------------------------------------------- what the declines wrote (J11)

/**
 * What a declined run wrote to one stream against the golden run's, each side's case folder made
 * <case> (and its other roots portable, portableRoots): both empty; the program's empty where the
 * C++ wrote some; the program's, line by line with each line's end, the first lines of the C++
 * run's (a prefix); or not a prefix. A decline's empty stream proves nothing of the lines the C++
 * writes before its boundary; the answered runs hold those (preBoundaryHolders).
 */
export type StreamClass = 'both empty' | 'empty' | 'a prefix' | 'not a prefix'

export function declineStreamClass(golden: Buffer | undefined, rust: Buffer | undefined): StreamClass {
  const g = (golden ?? Buffer.alloc(0)).toString('latin1')
  const r = (rust ?? Buffer.alloc(0)).toString('latin1')
  if (g === '' && r === '') return 'both empty'
  if (r === '') return 'empty'
  const gl = linesWithEnds(g)
  return linesWithEnds(r).every((l, i) => l === gl[i]) ? 'a prefix' : 'not a prefix'
}

export interface StreamCounts {
  bothEmpty: number
  empty: number
  prefix: number
  notPrefix: number
}

/** CertifyReport.declineStreams: every run the program declined, each of its two streams by its class. */
export interface DeclineStreams {
  declines: number
  stderr: StreamCounts
  stdout: StreamCounts
  /** The runs and streams that are not a prefix, as <case>/<run> <stream>. */
  notPrefix: string[]
}

/** Whether a run of the program declined: it wrote a result whose code is not-ported. */
export function isNotPorted(side: Side, caseId: string, run: RecordedRun): boolean {
  return run.files.some((f) => {
    const v = jsonOf(outputBytes(side.store, caseId, run, `files/${f}`))
    return isRecord(v) && v.format === 'sketch-orca-result/1' && v.code === NOT_PORTED
  })
}

/**
 * The report's decline streams (see StreamClass): for every run of the program whose result is
 * not-ported, its stderr and stdout against the golden run's (an empty stream where the golden
 * case has no such run), each side's paths portable. It changes no verdict and no exit status.
 */
export function declineStreams(cases: { caseId: string; golden: { side: Side; record: CaseRecord | undefined }; rust: { side: Side; record: CaseRecord | undefined } }[]): DeclineStreams {
  const zero = (): StreamCounts => ({ bothEmpty: 0, empty: 0, prefix: 0, notPrefix: 0 })
  const out: DeclineStreams = { declines: 0, stderr: zero(), stdout: zero(), notPrefix: [] }
  const key: Record<StreamClass, keyof StreamCounts> = { 'both empty': 'bothEmpty', empty: 'empty', 'a prefix': 'prefix', 'not a prefix': 'notPrefix' }
  for (const c of cases) {
    for (const r of c.rust.record?.runs ?? []) {
      if (!isNotPorted(c.rust.side, c.caseId, r)) continue
      out.declines++
      const g = c.golden.record?.runs.find((x) => x.index === r.index)
      for (const s of ['stderr', 'stdout'] as const) {
        const portable = (side: Side, run: RecordedRun | undefined) => portableBytes(outputBytes(side.store, c.caseId, run, s), portableRoots({ root: side.root, caseId: c.caseId, resources: side.resources, work: side.work }))
        const cls = declineStreamClass(portable(c.golden.side, g), portable(c.rust.side, r))
        out[s][key[cls]]++
        if (cls === 'not a prefix') out.notPrefix.push(`${c.caseId}/${r.index} ${s}`)
      }
    }
  }
  return out
}

/** cmd-certify's line for the decline streams. */
export function declineStreamsLine(d: DeclineStreams): string {
  const part = (s: StreamCounts) => `${s.bothEmpty} both empty, ${s.empty} empty, ${s.prefix} a prefix, ${s.notPrefix} not a prefix`
  return `decline streams: ${d.declines} declines; stderr ${part(d.stderr)}; stdout ${part(d.stdout)}`
}

/**
 * The report's pre-boundary holders: for each family of parity.json, the answered runs (passed:
 * not declined, every output and dump the golden's) of the cases that reach it, as <case>/<run>.
 * They hold the lines the family's C++ writes before its boundary, which a decline's empty stream
 * does not. It changes no verdict and no exit status.
 */
export function preBoundaryHolders(parity: Pick<ParityDocument, 'families'>, cases: Map<string, CaseVerdict>): { family: string; runs: string[] }[] {
  return parity.families.map((f) => ({ family: f.name, runs: f.cases.flatMap((id) => (cases.get(id)?.runs ?? []).filter((r) => r.passed).map((r) => `${id}/${r.run}`)) }))
}

/** cmd-certify's line for a family's pre-boundary holders, with the first five runs. */
export function preBoundaryLine(h: { family: string; runs: string[] }): string {
  return `pre-boundary holders of ${h.family}: ${h.runs.length} answered runs${h.runs.length > 0 ? ` (${h.runs.slice(0, 5).join(', ')}${h.runs.length > 5 ? ', ...' : ''})` : ''}`
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
  /** Where the decline let a corpus case through (declineGaps); certify fails on any. */
  declineGaps: string[]
  /**
   * The declined runs, as <case>/<run>, after which the case went on from the golden run's outputs
   * (goldenCarry); the runs after them in their case are the program's, made from what the C++
   * program had.
   */
  carried: string[]
  /** What the declines wrote to stderr and stdout against the C++ runs (declineStreams); information only. */
  declineStreams: DeclineStreams
  /** Each family's answered runs (preBoundaryHolders); information only. */
  preBoundaryHolders: { family: string; runs: string[] }[]
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

export function readJsonIf<T>(path: string): T | undefined {
  return existsSync(path) ? (JSON.parse(readFileSync(path, 'utf8')) as T) : undefined
}
