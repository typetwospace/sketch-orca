// parity certify (scripts/lib/parity/certify.ts, cmd-certify.ts): sketch-orca-rs's runs against the
// goldens. The engine block is made alike (version, implementation); a result is compared as a JSON
// value; an output classed R passes within the envelope of the engine's own variance and nowhere
// else; a decline (not-ported, -8) passes no family, the front ones included, and is itself wrong
// when the C++ run ended within what the program does itself (reading the job and the presets, the
// model's reading and the placement, each with its end line); a family is certified when every run of every case passes; parity.json's
// flags are raised and never lowered, and a certified family that fails is a regression; every run
// of a corpus job, example or refusal that reaches an uncertified family must be declined naming it,
// unless its C++ run ended within what the program does, the families held as the same run raises
// them; goldens count only when parity neutral passed with the staged oracle and they were recorded
// with it. The dumps: an answered run writes the golden's dumps, the same; a right decline that wrote
// every dump of a boundary and of every boundary before it the same, its progress the C++ run's
// first lines, passes the families before that boundary and no other; the end of process()
// (print-psConflictCheck) passes every family process() runs and no boundary passes layers; a dump
// that differs makes the decline wrong; a golden dump kept as its hash is compared by it; goldens of
// before dumps were kept compare none. A decline's streams: the C++ run's first lines there, paths
// portable and masks applied, and not one byte more; the report counts what every decline wrote
// there and names each family's answered runs (J11). The runs after a decline (goldenCarry): the case goes on
// from the golden run's outputs, its paths swapped for this folder's, so a later run is made from
// what the C++ program had, and that run is judged in full. Hermetic: the runs are written to a
// temporary folder.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  BOUNDARY_FAMILIES,
  boundariesReached,
  compareCase,
  DECLINE_HELD_KINDS,
  declineGaps,
  declineProblems,
  declineStreamClass,
  declineStreams,
  declineStreamsLine,
  endedWithinRust,
  familyVerdicts,
  FRONT_FAMILIES,
  goldenCarry,
  isResult,
  normaliseEngine,
  oracleChainProblems,
  preBoundaryHolders,
  preBoundaryLine,
  progressIsPrefix,
  progressStages,
  raiseCertified,
  rebaseBytes,
  rustProgramPath,
  rustSources,
  sameJsonValue,
  staleSources,
  withinEnvelope,
  type CaseVerdict,
  type Side,
} from '../scripts/lib/parity/certify.ts'
import { sha256Bytes } from '../scripts/lib/files.ts'
import { FAMILIES, FRONT_OBJECTS_FAMILIES, MARKER_FAMILIES, O3_FAMILIES } from '../scripts/lib/parity/corpus.ts'
import { checkParityModule } from '../scripts/lib/parity/dispatch.ts'
import type { OutputEnvelope } from '../scripts/lib/parity/envelope.ts'
import type { Mask } from '../scripts/lib/parity/masks.ts'
import { KEPT_DUMPS, type FamilyEntry, type ParityDocument } from '../scripts/lib/parity/record.ts'
import { recordingExec, runEngineCases, runStore, type CaseRecord, type RecordedRun, type RunProcess } from '../scripts/lib/parity/same.ts'
import * as cmd from '../scripts/lib/parity/cmd-certify.ts'
import type { FlatPreset, PresetSet } from '../scripts/lib/profiles.ts'
import { SMOKE_PRESETS, type SmokeCase, type SmokePresetSets } from '../scripts/lib/smoke.ts'

let work = ''
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-certify-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

function side(name: string): Side {
  return { store: join(work, name, 'store'), root: join(work, name, 'cases'), resources: join(work, 'resources'), work: join(work, name) }
}

/** Keeps a run of `caseId` in `s`'s store as recordingExec does (its dumps as collectDumps does), and returns its record. */
function keep(s: Side, caseId: string, index: number, o: { code: number; files?: Record<string, string>; stdout?: string; stderr?: string; gone?: string[]; args?: string[]; dumps?: Record<string, Buffer> }): RecordedRun {
  const dir = join(s.store, caseId, String(index))
  mkdirSync(join(dir, 'files'), { recursive: true })
  if (o.dumps) {
    mkdirSync(join(dir, 'dumps'), { recursive: true })
    for (const [f, b] of Object.entries(o.dumps)) writeFileSync(join(dir, 'dumps', f), b)
  }
  writeFileSync(join(dir, 'stdout'), (o.stdout ?? '').split('<case>').join(join(s.root, caseId)))
  writeFileSync(join(dir, 'stderr'), (o.stderr ?? '').split('<case>').join(join(s.root, caseId)))
  for (const [rel, text] of Object.entries(o.files ?? {})) {
    mkdirSync(join(dir, 'files', ...rel.split('/').slice(0, -1)), { recursive: true })
    writeFileSync(join(dir, 'files', ...rel.split('/')), text.split('<case>').join(join(s.root, caseId)))
  }
  return { index, args: o.args ?? ['run', '--job', '<cases>/c/job.json'], extraEnv: [], code: o.code, timedOut: false, files: Object.keys(o.files ?? {}).sort(), gone: o.gone ?? [], ...(o.dumps ? { dumps: Object.keys(o.dumps).sort() } : {}) }
}

function caseRecord(id: string, runs: RecordedRun[]): CaseRecord {
  return { id, ok: true, problems: [], warnings: [], runs }
}

function family(name: string, cases: string[], certified = false): FamilyEntry {
  return { name, step: '', what: '', cases, classes: ['B'], certified }
}

function parityOf(families: FamilyEntry[]): ParityDocument {
  return { format: 'sketch-orca-parity/1', corpus: { file: 'parity/corpus.json', sha256: 'x' }, threads: [1], engineRuns: 5, families, hSites: [] }
}

const CPP_ENGINE = '"engine": {\n    "engine": "sketch-orca",\n    "engineVersion": "0.1.0",\n    "orcaVersion": "2.4.2"\n  }'
const RUST_ENGINE = '"engine": {\n    "engine": "sketch-orca",\n    "engineVersion": "0.2.0",\n    "orcaVersion": "2.4.2",\n    "implementation": "rust"\n  }'

function result(fields: string, engine: string): string {
  return `{\n  "format": "sketch-orca-result/1",\n${fields},\n  ${engine}\n}\n`
}

const NOT_PORTED_FIELDS = '  "ok": false,\n  "code": "not-ported",\n  "message": "m",\n  "missing": [\n    "model-load"\n  ],\n  "exitCode": -8'
const PROGRESS_JOB = '{"stage":"job","percent":1.0,"message":"reading the job"}\n'
const PROGRESS_PRESETS = PROGRESS_JOB + '{"stage":"presets","percent":3.0,"message":"reading the presets"}\n'
const PROGRESS_MODEL = PROGRESS_PRESETS + '{"stage":"model","percent":8.0,"message":"reading the model"}\n'
const FAILED = '{"stage":"failed","percent":8.0,"message":"x"}\n'
const PROGRESS_ARRANGE = PROGRESS_MODEL + '{"stage":"arrange","percent":12.0,"message":"arranging"}\n'
/** A C++ run past what the program does: it placed the objects and failed while slicing (empty-plate). */
const PROGRESS_PAST = PROGRESS_ARRANGE + '{"stage":"slice","percent":14.0,"message":"slicing"}\n' + FAILED

/** A failed result's fields with `code`. */
function failedFields(code: string): string {
  return `  "ok": false,\n  "code": "${code}",\n  "message": "x",\n  "exitCode": -2`
}

const NO_CLASS = () => undefined

describe('the engine block', () => {
  test("the version and the Rust program's implementation are not compared", () => {
    expect(normaliseEngine(result('  "ok": true', RUST_ENGINE))).toBe(normaliseEngine(result('  "ok": true', CPP_ENGINE)))
    expect(normaliseEngine('{\r\n  "engineVersion": "0.2.0",\r\n  "orcaTag": "v2.4.2",\r\n  "implementation": "rust"\r\n}\r\n')).toBe('{\r\n  "engineVersion": "<engineVersion>",\r\n  "orcaTag": "v2.4.2"\r\n}\r\n')
    // Anything else in it is.
    expect(normaliseEngine(result('  "ok": true', RUST_ENGINE.replace('2.4.2', '2.4.3')))).not.toBe(normaliseEngine(result('  "ok": true', CPP_ENGINE)))
  })
})

describe('results as JSON values', () => {
  test('the same members in the same order with the same values', () => {
    expect(sameJsonValue({ a: 1, b: [1, { c: 'x' }] }, JSON.parse('{ "a": 1.0, "b": [1, {"c": "x"}] }'))).toBe(true)
    expect(sameJsonValue({ a: 1, b: 2 }, { b: 2, a: 1 })).toBe(false)
    expect(sameJsonValue({ a: 1 }, { a: 1, b: 2 })).toBe(false)
    expect(sameJsonValue([1, 2], [2, 1])).toBe(false)
    expect(sameJsonValue({ a: null }, { a: 0 })).toBe(false)
  })

  test('a result is known by its format', () => {
    expect(isResult(Buffer.from(result('  "ok": true', CPP_ENGINE)))).toBe(true)
    expect(isResult(Buffer.from('{"format":"sketch-orca-job/1"}'))).toBe(false)
    expect(isResult(Buffer.from('not json'))).toBe(false)
  })

  test('a result written differently but of the same value passes; one of another value does not', () => {
    const g = side('golden')
    const r = side('rust')
    const cpp = keep(g, 'c', 0, { code: 0, files: { 'result.json': result('  "ok": true,\n  "outputs": [\n    "<case>/a.gcode"\n  ]', CPP_ENGINE) } })
    const same = keep(r, 'c', 0, { code: 0, files: { 'result.json': result('  "ok": true,\n  "outputs": ["<case>/a.gcode"]', RUST_ENGINE) } })
    const v = compareCase({ caseId: 'c', golden: { side: g, record: caseRecord('c', [cpp]) }, rust: { side: r, record: caseRecord('c', [same]) }, masks: [], families: [], classOf: NO_CLASS, envelopeOf: NO_CLASS })
    expect(v.runs).toEqual([{ run: 0, passed: true, declined: false, boundaries: [], problems: [] }])
    const other = side('other')
    const differs = keep(other, 'c', 0, { code: 0, files: { 'result.json': result('  "ok": true,\n  "outputs": ["<case>/b.gcode"]', RUST_ENGINE) } })
    const w = compareCase({ caseId: 'c', golden: { side: g, record: caseRecord('c', [cpp]) }, rust: { side: other, record: caseRecord('c', [differs]) }, masks: [], families: [], classOf: NO_CLASS, envelopeOf: NO_CLASS })
    expect(w.runs[0]!.passed).toBe(false)
    expect(w.runs[0]!.problems.join()).toContain('files/result.json')
  })

  test('every other output byte for byte, its paths portable', () => {
    const g = side('golden')
    const r = side('rust')
    const cpp = keep(g, 'c', 0, { code: 0, files: { 'a.gcode': 'G1 X1\n; <case>\n' }, stdout: 'done\r\n' })
    const rust = keep(r, 'c', 0, { code: 0, files: { 'a.gcode': 'G1 X1\n; <case>\n' }, stdout: 'done\n' })
    const v = compareCase({ caseId: 'c', golden: { side: g, record: caseRecord('c', [cpp]) }, rust: { side: r, record: caseRecord('c', [rust]) }, masks: [], families: [], classOf: NO_CLASS, envelopeOf: NO_CLASS })
    expect(v.runs[0]!.passed).toBe(false)
    expect(v.runs[0]!.problems.join()).toContain('stdout')
    expect(v.runs[0]!.problems.join()).not.toContain('a.gcode')
  })
})

describe('R outputs within the envelope', () => {
  const env: OutputEnvelope = { case: 'c', run: 0, output: 'files/a.gcode', runs: 5, kind: 'text', lineCounts: [3, 3, 3, 3, 3], varyingLines: 1, lines: [{ line: 2, distinct: 3, numbers: [{ field: 2, min: 1.5, max: 1.7, spread: 0.2 }] }] }
  const golden = Buffer.from('G1 X1\nG1 X2 E1.6\n')

  test('a varying number within the least and greatest of the runs passes', () => {
    expect(withinEnvelope(golden, Buffer.from('G1 X1\nG1 X2 E1.7\n'), env).ok).toBe(true)
    expect(withinEnvelope(golden, golden, undefined).ok).toBe(true)
  })

  test('outside it, on a steady number or line, or with another line count, it fails', () => {
    expect(withinEnvelope(golden, Buffer.from('G1 X1\nG1 X2 E1.8\n'), env).detail).toContain('outside 1.5..1.7')
    expect(withinEnvelope(golden, Buffer.from('G1 X1\nG1 X3 E1.6\n'), env).detail).toContain('the steady 2')
    expect(withinEnvelope(golden, Buffer.from('G1 X9\nG1 X2 E1.6\n'), env).detail).toContain('does not vary')
    expect(withinEnvelope(golden, Buffer.from('G1 X1\nG1 X2 E1.6\n\n'), env).detail).toContain('4 lines')
    expect(withinEnvelope(golden, Buffer.from('G1 X1\nG1 X2 E1.7\n'), undefined).detail).toContain('no entry')
    expect(withinEnvelope(golden, Buffer.from('G1 X1\nG1 X2 E1.7\n'), { ...env, kind: 'binary' }).ok).toBe(false)
  })

  test('only an output record.json classes R gets the envelope', () => {
    const g = side('golden')
    const r = side('rust')
    const cpp = keep(g, 'c', 0, { code: 0, files: { 'a.gcode': golden.toString() } })
    const rust = keep(r, 'c', 0, { code: 0, files: { 'a.gcode': 'G1 X1\nG1 X2 E1.7\n' } })
    const run = (cls: 'R' | 'B') => compareCase({ caseId: 'c', golden: { side: g, record: caseRecord('c', [cpp]) }, rust: { side: r, record: caseRecord('c', [rust]) }, masks: [], families: [], classOf: () => cls, envelopeOf: () => env }).runs[0]!.passed
    expect(run('R')).toBe(true)
    expect(run('B')).toBe(false)
  })
})

describe('a decline', () => {
  const families = [family('version', ['v']), family('intake', ['c']), family('model-load', ['m'])]

  function declined(o: {
    goldenProgress?: string | null
    goldenCode?: string
    rustFiles?: Record<string, string>
    missing?: string[]
    code?: number
    stdout?: string
    stderr?: string
    goldenStdout?: string
    goldenStderr?: string
    masks?: Mask[]
    families?: FamilyEntry[]
  }) {
    const g = side('golden')
    const r = side('rust')
    const goldenFiles: Record<string, string> = { 'result.json': result(failedFields(o.goldenCode ?? 'empty-plate'), CPP_ENGINE) }
    if (o.goldenProgress !== null) goldenFiles['progress.ndjson'] = o.goldenProgress ?? PROGRESS_PAST
    const cpp = keep(g, 'c', 0, { code: -50, files: goldenFiles, stdout: o.goldenStdout ?? '', stderr: o.goldenStderr ?? '' })
    const fields = o.missing ? NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', JSON.stringify(o.missing)) : NOT_PORTED_FIELDS
    const rust = keep(r, 'c', 0, { code: o.code ?? -8, stdout: o.stdout ?? '', stderr: o.stderr ?? '', files: o.rustFiles ?? { 'progress.ndjson': PROGRESS_MODEL + FAILED, 'result.json': result(fields, RUST_ENGINE) } })
    return { g, r, cpp, rust, problems: declineProblems({ caseId: 'c', golden: { side: g, run: cpp }, rust: { side: r, run: rust }, families: o.families ?? families, masks: o.masks ?? [] }) }
  }

  test('that is right is information: it passes no family, a front family neither', () => {
    const d = declined({})
    expect(d.problems).toEqual([])
    const v = compareCase({ caseId: 'c', golden: { side: d.g, record: caseRecord('c', [d.cpp]) }, rust: { side: d.r, record: caseRecord('c', [d.rust]) }, masks: [], families, classOf: NO_CLASS, envelopeOf: NO_CLASS })
    expect(v.runs).toEqual([{ run: 0, passed: false, declined: true, boundaries: [], problems: [] }])
    const verdicts = familyVerdicts(parityOf(families), new Map([['c', v]]))
    expect(verdicts.find((f) => f.name === 'intake')).toMatchObject({ passed: false, declined: 1, certified: false })
    expect(verdicts.find((f) => f.name === 'intake')!.problems).toEqual(['c/0: declined (not-ported)'])
    // A family certified before that now has a declined run is a regression.
    const before = parityOf([family('intake', ['c'], true)])
    expect(raiseCertified(before, familyVerdicts(before, new Map([['c', v]]))).regressions).toEqual(['intake'])
  })

  test('the exit code -8 is seen as its low byte, 248, too', () => {
    expect(declined({ code: 248 }).problems).toEqual([])
  })

  test('is wrong when the C++ run ended within what the program does itself', () => {
    const within = 'ended within what sketch-orca-rs does itself'
    // Refused while reading the job, in the presets, at the model's file, or with no progress at all.
    expect(declined({ goldenProgress: PROGRESS_JOB + FAILED, goldenCode: 'job-invalid' }).problems.join()).toContain(within)
    expect(declined({ goldenProgress: PROGRESS_PRESETS + FAILED, goldenCode: 'setting-invalid' }).problems.join()).toContain(within)
    expect(declined({ goldenProgress: PROGRESS_MODEL + FAILED, goldenCode: 'file-not-found' }).problems.join()).toContain(within)
    expect(declined({ goldenProgress: null, goldenCode: 'invalid-params' }).problems.join()).toContain(within)
    // A crash at the stage model comes before its line: the C++ run shows the presets alone.
    expect(declined({ goldenProgress: PROGRESS_PRESETS, goldenCode: 'internal-error' }).problems.join()).toContain(within)
    // From 0.3.0 the model's reading and the placement: refused there, or an info job answered.
    expect(declined({ goldenProgress: PROGRESS_MODEL + FAILED, goldenCode: 'model-unreadable' }).problems.join()).toContain(within)
    expect(declined({ goldenProgress: PROGRESS_MODEL + FAILED, goldenCode: 'config-invalid' }).problems.join()).toContain(within)
    expect(declined({ goldenProgress: PROGRESS_ARRANGE + FAILED, goldenCode: 'arrange-failed' }).problems.join()).toContain(within)
  })

  test('is right when the C++ run went past it: a crash before the next line, or the slice', () => {
    // A crash in the model or the placement, before its end's line, may be past the build volume check.
    expect(declined({ goldenProgress: PROGRESS_MODEL, goldenCode: 'internal-error' }).problems).toEqual([])
    expect(declined({ goldenProgress: PROGRESS_ARRANGE, goldenCode: 'internal-error' }).problems).toEqual([])
    expect(declined({ goldenProgress: PROGRESS_PAST }).problems).toEqual([])
  })

  test('naming preset-conditions is right once the C++ run reached the presets', () => {
    const conditions = ['preset-conditions', 'model-load']
    const fams = [...families, family('preset-conditions', ['c'])]
    expect(declined({ missing: conditions, families: fams, goldenProgress: PROGRESS_PRESETS + FAILED, goldenCode: 'process-not-compatible' }).problems).toEqual([])
    expect(declined({ missing: conditions, families: fams, goldenProgress: PROGRESS_PAST }).problems).toEqual([])
    expect(declined({ missing: conditions, families: fams, goldenProgress: PROGRESS_JOB + FAILED, goldenCode: 'job-invalid' }).problems.join()).toContain('did not reach')
  })

  test('where a C++ run ended: within the job, the presets, the model and the placement, with its end line', () => {
    expect(endedWithinRust([], undefined)).toBe(true)
    expect(endedWithinRust(['job', 'failed'], 'job-invalid')).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'failed'], 'config-invalid')).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'model', 'failed'], 'file-not-found')).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'model', 'failed'], 'model-unreadable')).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'model', 'done'], undefined)).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'model'], 'internal-error')).toBe(false)
    expect(endedWithinRust(['job', 'presets', 'model', 'arrange', 'failed'], 'arrange-failed')).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'model', 'arrange', 'done'], undefined)).toBe(true)
    expect(endedWithinRust(['job', 'presets', 'model', 'arrange'], 'internal-error')).toBe(false)
    expect(endedWithinRust(['job', 'presets', 'model', 'arrange', 'slice', 'failed'], 'empty-plate')).toBe(false)
    expect(endedWithinRust(['job', 'presets', 'model', 'orient', 'failed'], 'internal-error')).toBe(false)
  })

  test('that wrote what the C++ did not, wrote to a stream, or misses a certified or front family does not', () => {
    expect(declined({ rustFiles: { 'progress.ndjson': PROGRESS_JOB, 'result.json': result(NOT_PORTED_FIELDS, RUST_ENGINE), 'out.gcode': '' } }).problems.join()).toContain('out.gcode')
    expect(declined({ stderr: 'x' }).problems.join()).toContain('stderr')
    expect(declined({ families: [family('intake', ['c']), family('model-load', ['m'], true)] }).problems.join()).toContain('which parity.json certifies')
    const front = NOT_PORTED_FIELDS.replace('"model-load"', '"intake"')
    expect(declined({ rustFiles: { 'result.json': result(front, RUST_ENGINE) } }).problems.join()).toContain('a front family')
    const empty = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', '[]')
    expect(declined({ rustFiles: { 'result.json': result(empty, RUST_ENGINE) } }).problems.join()).toContain('no missing families')
    expect(declined({ code: -2 }).problems.join()).toContain('not -8')
  })

  const MTL_LINE = '[error] load_obj: failed to load mtl_path:<case>/missing.mtl\r\n'

  test("may write to a stream the C++ run's first lines there, its paths portable: the C++ run went on and wrote more", () => {
    // The C++ run's line, written in each side's own case folder.
    expect(declined({ goldenStderr: MTL_LINE, stderr: MTL_LINE }).problems).toEqual([])
    expect(declined({ goldenStderr: MTL_LINE + '[warning] past placement\r\n', stderr: MTL_LINE }).problems).toEqual([])
    expect(declined({ goldenStdout: 'a\nb\n', goldenStderr: MTL_LINE, stdout: 'a\n', stderr: MTL_LINE }).problems).toEqual([])
    // An empty stream always is.
    expect(declined({ goldenStderr: MTL_LINE }).problems).toEqual([])
  })

  test('that writes one byte to a stream the C++ run did not write there is wrong', () => {
    const wrong = (o: { stdout?: string; stderr?: string }) => declined({ goldenStderr: MTL_LINE + 'more\r\n', ...o }).problems.join()
    // One byte past the C++ run's stream, on a line of its own or on the next line.
    expect(wrong({ stderr: MTL_LINE + 'more\r\nx' })).toContain('it wrote to stderr what the C++ run did not write there (line 3: the C++ run\'s no line, the Rust run\'s "x")')
    expect(wrong({ stderr: MTL_LINE + 'x' })).toContain('(line 2: ')
    // A line cut short, one with another end, or one with another path.
    expect(wrong({ stderr: MTL_LINE.slice(0, -1) })).toContain('it wrote to stderr')
    expect(wrong({ stderr: MTL_LINE.replace('\r\n', '\n') })).toContain('it wrote to stderr')
    expect(wrong({ stderr: MTL_LINE.replace('<case>', 'C:/elsewhere') })).toContain('it wrote to stderr')
    // The other stream, which the C++ run left empty.
    expect(wrong({ stderr: MTL_LINE, stdout: 'x' })).toContain('it wrote to stdout what the C++ run did not write there (line 1')
  })

  test('a stream is compared with the masks that name it applied, and with no other', () => {
    const mask: Mask = { id: 'stderr-time', kind: 'text', files: ['stderr'], pattern: 'at \\d\\d:\\d\\d', replace: 'at <time>', source: 's', why: 'w' }
    expect(declined({ goldenStderr: 'started at 10:00\nmore\n', stderr: 'started at 11:30\n', masks: [mask] }).problems).toEqual([])
    expect(declined({ goldenStderr: 'started at 10:00\nmore\n', stderr: 'started at 11:30\n' }).problems.join()).toContain('it wrote to stderr')
    expect(declined({ goldenStderr: 'started at 10:00\n', stderr: 'started at 11:30\n', masks: [{ ...mask, files: ['*.gcode'] }] }).problems.join()).toContain('it wrote to stderr')
  })

  test('the streams are judged alone: the exit code and progress of a decline stay with the decline rules', () => {
    // The C++ run went on to slice and exit; the decline exited -8, its progress cut short: right, and information.
    const d = declined({ goldenStderr: MTL_LINE + 'later\r\n', stderr: MTL_LINE })
    expect(d.problems).toEqual([])
    const v = compareCase({ caseId: 'c', golden: { side: d.g, record: caseRecord('c', [d.cpp]) }, rust: { side: d.r, record: caseRecord('c', [d.rust]) }, masks: [], families, classOf: NO_CLASS, envelopeOf: NO_CLASS })
    expect(v.runs).toEqual([{ run: 0, passed: false, declined: true, boundaries: [], problems: [] }])
    // A C++ run that ended within what the program does still makes the decline wrong, the stream the C++ run's.
    expect(declined({ goldenStderr: MTL_LINE, stderr: MTL_LINE, goldenProgress: PROGRESS_MODEL + FAILED, goldenCode: 'file-not-found' }).problems.join()).toContain('ended within what sketch-orca-rs does itself')
    // A decline's own exit code is held as before.
    expect(declined({ goldenStderr: MTL_LINE, stderr: MTL_LINE, code: -2 }).problems.join()).toContain('not -8')
  })

  test('never passes a family after the front end either', () => {
    const d = declined({})
    const v: CaseVerdict = { id: 'm', runs: [{ run: 0, passed: false, declined: true, boundaries: [], problems: [] }] }
    const verdicts = familyVerdicts(parityOf(families), new Map([['m', v]]))
    expect(verdicts.find((f) => f.name === 'model-load')).toMatchObject({ passed: false, declined: 1, certified: false })
    expect(d.problems).toEqual([])
    expect(FRONT_FAMILIES).toEqual(['version', 'config-def', 'intake'])
  })

  test('progress stages are read line by line', () => {
    expect(progressStages(Buffer.from(PROGRESS_PAST.replace(/\n/g, '\r\n')))).toEqual(['job', 'presets', 'model', 'arrange', 'slice', 'failed'])
    expect(progressStages(undefined)).toEqual([])
  })
})

describe('families and parity.json', () => {
  const pass: CaseVerdict = { id: 'a', runs: [{ run: 0, passed: true, declined: false, boundaries: [], problems: [] }] }
  const fail: CaseVerdict = { id: 'b', runs: [{ run: 0, passed: true, declined: false, boundaries: [], problems: [] }, { run: 1, passed: false, declined: false, boundaries: [], problems: ['exit: 1 against 2'] }] }

  test('a family passes when every run of every case of it passes, and not when a case was not run', () => {
    const p = parityOf([family('version', ['a']), family('intake', ['a', 'b']), family('placement', ['z']), family('info', [])])
    const v = familyVerdicts(p, new Map([['a', pass], ['b', fail]]))
    expect(v.map((f) => [f.name, f.passed, f.runs])).toEqual([
      ['version', true, 1],
      ['intake', false, 3],
      ['placement', false, 0],
      ['info', false, 0],
    ])
    expect(v[1]!.problems).toEqual(['b/1: exit: 1 against 2'])
    expect(v[2]!.problems).toEqual(['z: not run'])
  })

  test('flags are raised and never lowered; a certified family that fails is a regression', () => {
    const p = parityOf([family('version', ['a']), family('intake', ['b'], true), family('config-def', ['a'], true)])
    const before = JSON.stringify(p)
    const { parity, raised, regressions } = raiseCertified(p, familyVerdicts(p, new Map([['a', pass], ['b', fail]])))
    expect(parity.families.map((f) => [f.name, f.certified])).toEqual([
      ['version', true],
      ['intake', true],
      ['config-def', true],
    ])
    expect(raised).toEqual(['version'])
    expect(regressions).toEqual(['intake'])
    expect(JSON.stringify(p)).toBe(before)
  })
})

const u32 = (v: number) => {
  const b = Buffer.alloc(4)
  b.writeUInt32LE(v)
  return b
}
const u64 = (v: number) => {
  const b = Buffer.alloc(8)
  b.writeBigUInt64LE(BigInt(v))
  return b
}
const i64 = (v: number) => {
  const b = Buffer.alloc(8)
  b.writeBigInt64LE(BigInt(v))
  return b
}
const str = (t: string) => Buffer.concat([u32(Buffer.byteLength(t)), Buffer.from(t, 'utf8')])
/** A front-objects dump whose one CONF value is `value`. */
const dump = (value: string) => {
  const records: [string, Buffer][] = [
    ['HEAD', Buffer.concat([str('front'), str('objects'), i64(-1), u32(1)])],
    ['CONF', Buffer.concat([str('run'), i64(-1), i64(-1), u64(1), str('post_process'), u32(5), str(value), u64(0)])],
  ]
  const parts: Buffer[] = [Buffer.from('SOP1', 'latin1'), u32(1)]
  for (const [kind, payload] of [...records, ['END ', u64(records.length)] as [string, Buffer]]) parts.push(Buffer.from(kind, 'latin1'), u64(payload.length), payload)
  return Buffer.concat(parts)
}

describe('the dumps', () => {
  const families = [family('intake', ['c']), family('preset-conditions', ['c']), family('model-load', ['c']), family('placement', ['c']), family('slice-mesh', ['c'])]
  const GOLDEN_DUMPS = { 'front-objects-1.sop1': dump('<case>/post.py') }

  /** Case c: the C++ run went past placement and wrote GOLDEN_DUMPS (or, with `before`, no dumps named) and `stderr`; the Rust run as given. */
  function verdict(rust: { code: number; files: Record<string, string>; dumps?: Record<string, Buffer>; stderr?: string }, before = false, stderr = '') {
    const g = side('golden')
    const r = side('rust')
    const cpp = keep(g, 'c', 0, { code: 0, stderr, files: { 'progress.ndjson': PROGRESS_PAST, 'result.json': result(failedFields('empty-plate'), CPP_ENGINE) }, ...(before ? {} : { dumps: GOLDEN_DUMPS }) })
    const run = keep(r, 'c', 0, rust)
    const v = compareCase({ caseId: 'c', golden: { side: g, record: caseRecord('c', [cpp]) }, rust: { side: r, record: caseRecord('c', [run]) }, masks: [], families, classOf: NO_CLASS, envelopeOf: NO_CLASS })
    return { v, verdicts: familyVerdicts(parityOf(families), new Map([['c', v]])) }
  }
  const answered = { 'progress.ndjson': PROGRESS_PAST, 'result.json': result(failedFields('empty-plate'), RUST_ENGINE) }
  const NOT_PORTED_SLICE = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', '["slice-mesh"]')
  const declinedFiles = (progress: string) => ({ 'progress.ndjson': progress, 'result.json': result(NOT_PORTED_SLICE, RUST_ENGINE) })

  test('a boundary passes the families wholly before it: front-objects, the presets, the model and its placement', () => {
    expect(BOUNDARY_FAMILIES['front-objects']).toEqual(['preset-conditions', 'model-load', 'placement'])
    expect(boundariesReached(['front-objects-1.sop1', 'front-objects-2.sop1'], ['front-objects-1.sop1', 'front-objects-2.sop1'])).toEqual(['front-objects'])
    expect(boundariesReached(['front-objects-1.sop1', 'front-objects-2.sop1'], ['front-objects-1.sop1'])).toEqual([])
    expect(boundariesReached([], [])).toEqual([])
  })

  test("every kept boundary has its families, in process()'s order, each list holding the one before; the end of process() holds every family process() runs, and none holds layers or a marker", () => {
    expect(Object.keys(BOUNDARY_FAMILIES)).toEqual([...KEPT_DUMPS])
    const lists = Object.values(BOUNDARY_FAMILIES)
    lists.forEach((l, i) => {
      if (i > 0) for (const f of lists[i - 1]!) expect({ i, f, held: l.includes(f) }).toEqual({ i, f, held: true })
      expect(l).not.toContain('layers')
      for (const m of MARKER_FAMILIES) expect(l).not.toContain(m)
      // A family the corpus does not list is none.
      for (const f of l) expect(FAMILIES.some((x) => x.name === f)).toBe(true)
    })
    const end = BOUNDARY_FAMILIES['print-psConflictCheck']!
    expect([...end].sort()).toEqual([...FRONT_OBJECTS_FAMILIES, ...O3_FAMILIES.filter((f) => f !== 'layers')].sort())
    // Each family of process() first appears at the boundary of the step it belongs to.
    const first = (f: string) => Object.keys(BOUNDARY_FAMILIES).find((b) => BOUNDARY_FAMILIES[b]!.includes(f))
    expect(first('slice-mesh')).toBe('object-posSlice')
    expect(first('perimeters-arachne')).toBe('object-posPerimeters')
    expect(first('step-estimate-curled-extrusions')).toBe('object-posPrepareInfill')
    expect(first('infill-concentricinternal')).toBe('object-posInfill')
    expect(first('support-check')).toBe('object-posSupportMaterial')
    expect(first('step-detect-overhangs-for-lift')).toBe('object-posDetectOverhangsForLift')
    expect(first('tool-ordering')).toBe('print-psWipeTower')
    expect(first('step-skirt-brim')).toBe('print-psSkirtBrim')
    expect(first('step-simplify-path')).toBe('object-posSimplifyPath')
    expect(first('step-simplify-infill')).toBe('object-posSimplifyInfill')
    expect(first('step-conflict-check')).toBe('print-psConflictCheck')
  })

  test('a boundary is reached only when every golden dump of it and of every boundary before it is the same; one with no golden dump is passed over', () => {
    const golden = ['front-objects-1.sop1', 'front-config-1.sop1', 'object0-posSlice-1.sop1', 'object1-posSlice-1.sop1', 'object0-posPerimeters-1.sop1', 'object1-posPerimeters-1.sop1']
    expect(boundariesReached(golden, golden)).toEqual(['front-objects', 'front-config', 'object-posSlice', 'object-posPerimeters'])
    // Object 1's slice differs: neither its step's boundary nor any after it is reached, object 0's walls the same or not.
    expect(boundariesReached(golden, golden.filter((d) => d !== 'object1-posSlice-1.sop1'))).toEqual(['front-objects', 'front-config'])
    // The front-config dump not written (the program's decline at the placement stop): front-objects alone.
    expect(boundariesReached(golden, ['front-objects-1.sop1'])).toEqual(['front-objects'])
    // A later boundary the same is not reached past one that is not.
    expect(boundariesReached(golden, golden.filter((d) => d !== 'front-config-1.sop1'))).toEqual(['front-objects'])
  })

  test("an answered run writes the golden's dumps, the same once each side's paths are portable", () => {
    // The Rust run's dump names its own case folder, which the golden's writes as <case> too.
    const same = verdict({ code: 0, files: answered, dumps: { 'front-objects-1.sop1': dump(`${join(side('rust').root, 'c')}/post.py`) } })
    expect(same.v.runs).toEqual([{ run: 0, passed: true, declined: false, boundaries: [], problems: [] }])
    const none = verdict({ code: 0, files: answered, dumps: {} })
    expect(none.v.runs[0]).toMatchObject({ passed: false, problems: ['dumps/front-objects-1.sop1: not written, which the C++ run did'] })
    const other = verdict({ code: 0, files: answered, dumps: { 'front-objects-1.sop1': dump('y') } })
    expect(other.v.runs[0]!.problems).toEqual(['dumps/front-objects-1.sop1: differs from the C++ run\'s (record 1 (CONF))'])
  })

  test("a right decline that wrote the boundary's dumps the same, its progress the C++ run's first lines, passes those families and no other", () => {
    const d = verdict({ code: -8, files: declinedFiles(PROGRESS_ARRANGE + FAILED), dumps: GOLDEN_DUMPS })
    expect(d.v.runs).toEqual([{ run: 0, passed: false, declined: true, boundaries: ['front-objects'], problems: [] }])
    const by = (n: string) => d.verdicts.find((f) => f.name === n)!
    for (const n of ['preset-conditions', 'model-load', 'placement']) expect(by(n)).toMatchObject({ passed: true, declined: 1, boundaryPasses: 1, certified: true })
    for (const n of ['intake', 'slice-mesh']) expect(by(n)).toMatchObject({ passed: false, declined: 1, boundaryPasses: 0 })
  })

  test("a decline at the boundary that wrote the C++ run's log line passes its families; with one byte more it passes none", () => {
    // model-obj-mtl-missing-layers: the C++ program logs the missing material library, places the objects and slices.
    const line = '[error] load_obj: failed to load mtl_path:<case>/missing.mtl\r\n'
    const d = verdict({ code: -8, files: declinedFiles(PROGRESS_ARRANGE + FAILED), dumps: GOLDEN_DUMPS, stderr: line }, false, line)
    expect(d.v.runs).toEqual([{ run: 0, passed: false, declined: true, boundaries: ['front-objects'], problems: [] }])
    for (const n of ['model-load', 'placement']) expect(d.verdicts.find((f) => f.name === n)).toMatchObject({ passed: true, boundaryPasses: 1 })
    const more = verdict({ code: -8, files: declinedFiles(PROGRESS_ARRANGE + FAILED), dumps: GOLDEN_DUMPS, stderr: line + 'x' }, false, line)
    expect(more.v.runs[0]).toMatchObject({ passed: false, declined: false, boundaries: [] })
    expect(more.v.runs[0]!.problems.join()).toContain('it wrote to stderr what the C++ run did not write there')
    for (const n of ['preset-conditions', 'model-load', 'placement']) expect(more.verdicts.find((f) => f.name === n)).toMatchObject({ passed: false, boundaryPasses: 0 })
  })

  test('a decline passes no family when its progress is not the C++ run\'s first lines, it wrote no dump, or the goldens are of before', () => {
    const other = verdict({ code: -8, files: declinedFiles(PROGRESS_PRESETS + '{"stage":"model","percent":9.0,"message":"x"}\n' + FAILED), dumps: GOLDEN_DUMPS })
    expect(other.v.runs[0]).toMatchObject({ declined: true, boundaries: [] })
    const none = verdict({ code: -8, files: declinedFiles(PROGRESS_MODEL + FAILED), dumps: {} })
    expect(none.v.runs[0]).toMatchObject({ declined: true, boundaries: [] })
    expect(none.verdicts.find((f) => f.name === 'model-load')).toMatchObject({ passed: false, boundaryPasses: 0 })
    const before = verdict({ code: -8, files: declinedFiles(PROGRESS_MODEL + FAILED), dumps: GOLDEN_DUMPS }, true)
    expect(before.v.runs[0]).toMatchObject({ declined: true, boundaries: [] })
  })

  test('a decline that wrote a dump the C++ run did not, or one that differs, is wrong', () => {
    const wrong = verdict({ code: -8, files: declinedFiles(PROGRESS_MODEL + FAILED), dumps: { 'front-objects-1.sop1': dump('y') } })
    expect(wrong.v.runs[0]).toMatchObject({ declined: false, passed: false })
    expect(wrong.v.runs[0]!.problems.join()).toContain("dumps/front-objects-1.sop1: differs from the C++ run's")
    const extra = verdict({ code: -8, files: declinedFiles(PROGRESS_MODEL + FAILED), dumps: { ...GOLDEN_DUMPS, 'front-objects-2.sop1': dump('z') } })
    expect(extra.v.runs[0]!.problems.join()).toContain('dumps/front-objects-2.sop1: written, which the C++ run did not')
  })

  test("progress lines are the C++ run's first ones, the Rust run's final failed line aside", () => {
    const g = Buffer.from(PROGRESS_PAST)
    expect(progressIsPrefix(g, Buffer.from(PROGRESS_MODEL + FAILED))).toBe(true)
    expect(progressIsPrefix(g, Buffer.from(PROGRESS_MODEL))).toBe(true)
    expect(progressIsPrefix(g, Buffer.from(PROGRESS_JOB + '{"stage":"model","percent":8.0,"message":"reading the model"}\n'))).toBe(false)
    expect(progressIsPrefix(Buffer.from(PROGRESS_JOB), Buffer.from(PROGRESS_MODEL))).toBe(false)
    expect(progressIsPrefix(g, undefined)).toBe(true)
  })
})

/** A dump of `name` (front-config, object0-posSlice, print-psConflictCheck, ...) whose one CONF value is `value`. */
function stepDump(name: string, value = 'v'): Buffer {
  const records: [string, Buffer][] = [
    ['HEAD', Buffer.concat([str('step'), str(name), i64(-1), u32(1)])],
    ['CONF', Buffer.concat([str('job'), i64(-1), i64(-1), u64(1), str('k'), u32(5), str(value), u64(0)])],
  ]
  const parts: Buffer[] = [Buffer.from('SOP1', 'latin1'), u32(1)]
  for (const [kind, payload] of [...records, ['END ', u64(records.length)] as [string, Buffer]]) parts.push(Buffer.from(kind, 'latin1'), u64(payload.length), payload)
  return Buffer.concat(parts)
}

describe('the end of process()', () => {
  // A domain-A slice job of two objects: every boundary of process() the C++ run wrote.
  const BOUNDARIES = [
    'front-objects-1',
    'front-config-1',
    ...['posSlice', 'posPerimeters', 'posPrepareInfill', 'posInfill', 'posIroning', 'posContouring', 'posSupportMaterial', 'posDetectOverhangsForLift'].flatMap((s) => [`object0-${s}-1`, `object1-${s}-1`]),
    'print-psWipeTower-1',
    'print-psSkirtBrim-1',
    ...['posSimplifyPath', 'posSimplifyInfill', 'posSimplifySupportPath'].flatMap((s) => [`object0-${s}-1`, `object1-${s}-1`]),
    'print-psConflictCheck-1',
  ]
  const GOLDEN = Object.fromEntries(BOUNDARIES.map((b) => [`${b}.sop1`, stepDump(b)]))
  const processFamilies = O3_FAMILIES.filter((f) => f !== 'layers')
  const families = [...['intake', 'preset-conditions', 'model-load', 'placement', ...O3_FAMILIES].map((n) => family(n, ['s'])), family('gcode-export', ['s'])]
  const SLICE = '{"stage":"slice","percent":14.0,"message":"slicing"}\n'
  const NOT_PORTED_GCODE = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', '["gcode-export"]')

  /** The C++ run sliced and exported; the program declined right after process(), writing `dumps`. */
  function slice(dumps: Record<string, Buffer>, hashed: string[] = []) {
    const g = side('golden')
    const r = side('rust')
    const cpp = keep(g, 's', 0, { code: 0, files: { 'progress.ndjson': PROGRESS_ARRANGE + SLICE + '{"stage":"export","percent":90.0,"message":"x"}\n', 'result.json': result('  "ok": true', CPP_ENGINE), 'out.gcode': 'G1' }, dumps: GOLDEN })
    // A dump the goldens keep as its SHA-256 alone (record.ts keepGoldens).
    for (const h of hashed) {
      const at = join(runStore(join(g.store, 's'), 0).dumps, h)
      writeFileSync(`${at}.sha256`, `${sha256Bytes(readFileSync(at))}\n`)
      rmSync(at)
    }
    const run = keep(r, 's', 0, { code: -8, files: { 'progress.ndjson': PROGRESS_ARRANGE + SLICE + FAILED, 'result.json': result(NOT_PORTED_GCODE, RUST_ENGINE) }, dumps })
    const v = compareCase({ caseId: 's', golden: { side: g, record: caseRecord('s', [cpp]) }, rust: { side: r, record: caseRecord('s', [run]) }, masks: [], families, classOf: NO_CLASS, envelopeOf: NO_CLASS })
    return { v, verdicts: familyVerdicts(parityOf(families), new Map([['s', v]])) }
  }
  const by = (d: ReturnType<typeof slice>, n: string) => d.verdicts.find((f) => f.name === n)!

  test('a declined slice run that wrote every dump the same passes every family of process(), and not layers', () => {
    const d = slice(GOLDEN)
    expect(d.v.runs[0]).toMatchObject({ passed: false, declined: true, problems: [] })
    expect(d.v.runs[0]!.boundaries.at(-1)).toBe('print-psConflictCheck')
    // Every boundary but posEstimateCurledExtrusions, of which this run has no dump.
    expect(d.v.runs[0]!.boundaries).toEqual([...KEPT_DUMPS].filter((b) => b !== 'object-posEstimateCurledExtrusions'))
    for (const n of ['preset-conditions', 'model-load', 'placement', ...processFamilies]) expect({ n, ...by(d, n) }).toMatchObject({ n, passed: true, declined: 1, boundaryPasses: 1 })
    for (const n of ['layers', 'intake', 'gcode-export']) expect({ n, ...by(d, n) }).toMatchObject({ n, passed: false, boundaryPasses: 0 })
  })

  test('one with an object dump not written passes only the families before it; one with an object dump that differs is wrong and passes none', () => {
    const missing = { ...GOLDEN }
    delete missing['object1-posInfill-1.sop1']
    const d = slice(missing)
    expect(d.v.runs[0]).toMatchObject({ declined: true, problems: [] })
    expect(d.v.runs[0]!.boundaries.at(-1)).toBe('object-posPrepareInfill')
    for (const n of ['slice-mesh', 'perimeters-arachne', 'step-prepare-infill']) expect(by(d, n)).toMatchObject({ passed: true, boundaryPasses: 1 })
    for (const n of ['infill-monotonic', 'support-check', 'tool-ordering', 'step-conflict-check', 'layers']) expect({ n, ...by(d, n) }).toMatchObject({ n, passed: false, boundaryPasses: 0 })
    // The end of process() alone not written: every family but the conflict check's.
    const noEnd = { ...GOLDEN }
    delete noEnd['print-psConflictCheck-1.sop1']
    const e = slice(noEnd)
    expect(e.v.runs[0]!.boundaries.at(-1)).toBe('object-posSimplifySupportPath')
    expect(by(e, 'step-simplify-infill')).toMatchObject({ passed: true })
    expect(by(e, 'step-conflict-check')).toMatchObject({ passed: false })
    // An object dump that differs makes the decline wrong.
    const w = slice({ ...GOLDEN, 'object0-posPerimeters-1.sop1': stepDump('object0-posPerimeters-1', 'other') })
    expect(w.v.runs[0]).toMatchObject({ declined: false, passed: false, boundaries: [] })
    expect(w.v.runs[0]!.problems.join()).toContain("dumps/object0-posPerimeters-1.sop1: differs from the C++ run's")
    for (const n of ['slice-mesh', 'step-conflict-check', 'layers']) expect(by(w, n)).toMatchObject({ passed: false, boundaryPasses: 0 })
  })

  test('a golden dump kept as its SHA-256 alone is compared by its hash', () => {
    const hashed = ['object0-posInfill-1.sop1', 'print-psConflictCheck-1.sop1']
    const d = slice(GOLDEN, hashed)
    expect(d.v.runs[0]!.boundaries.at(-1)).toBe('print-psConflictCheck')
    const w = slice({ ...GOLDEN, 'object0-posInfill-1.sop1': stepDump('object0-posInfill-1', 'other') }, hashed)
    expect(w.v.runs[0]!.problems.join()).toContain('dumps/object0-posInfill-1.sop1: differs from the C++ run\'s (by its SHA-256')
    const missing = { ...GOLDEN }
    delete missing['print-psConflictCheck-1.sop1']
    expect(slice(missing, hashed).v.runs[0]!.boundaries.at(-1)).toBe('object-posSimplifySupportPath')
  })
})

describe('what the declines wrote (J11)', () => {
  const LINE = '[error] load_obj: failed to load mtl_path:<case>/missing.mtl\r\n'
  const NOT_PORTED_SLICE = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', '["slice-mesh"]')

  /** Case `id`: the C++ run wrote `golden` to stderr (and `goldenOut` to stdout); the program declined, writing `stderr` and `stdout`, or answered with `answered`. */
  function run(id: string, o: { golden?: string; goldenOut?: string; stderr?: string; stdout?: string; answered?: boolean }) {
    const g = side('golden')
    const r = side('rust')
    const files = { 'progress.ndjson': PROGRESS_PAST, 'result.json': result(failedFields('empty-plate'), CPP_ENGINE) }
    const cpp = keep(g, id, 0, { code: -50, files, stdout: o.goldenOut ?? '', stderr: o.golden ?? '' })
    const rust = o.answered
      ? keep(r, id, 0, { code: -50, files: { 'progress.ndjson': PROGRESS_PAST, 'result.json': result(failedFields('empty-plate'), RUST_ENGINE) }, stdout: o.goldenOut ?? '', stderr: o.golden ?? '' })
      : keep(r, id, 0, { code: -8, files: { 'progress.ndjson': PROGRESS_ARRANGE + FAILED, 'result.json': result(NOT_PORTED_SLICE, RUST_ENGINE) }, stdout: o.stdout ?? '', stderr: o.stderr ?? '' })
    return { caseId: id, golden: { side: g, record: caseRecord(id, [cpp]) }, rust: { side: r, record: caseRecord(id, [rust]) } }
  }

  test("each stream of a decline is both empty, the program's empty, a prefix of the C++ run's, or not a prefix", () => {
    expect(declineStreamClass(undefined, undefined)).toBe('both empty')
    expect(declineStreamClass(Buffer.from('a\n'), Buffer.alloc(0))).toBe('empty')
    expect(declineStreamClass(Buffer.from('a\nb\n'), Buffer.from('a\n'))).toBe('a prefix')
    expect(declineStreamClass(Buffer.from('a\nb\n'), Buffer.from('a\nb\n'))).toBe('a prefix')
    // A line cut short, one the C++ never wrote, or anything where the C++ wrote nothing.
    expect(declineStreamClass(Buffer.from('ab\n'), Buffer.from('a'))).toBe('not a prefix')
    expect(declineStreamClass(Buffer.from('a\n'), Buffer.from('x\n'))).toBe('not a prefix')
    expect(declineStreamClass(Buffer.alloc(0), Buffer.from('x\n'))).toBe('not a prefix')
  })

  test('the report counts every decline: one that wrote nothing, one that wrote a prefix, one that wrote a line the C++ never wrote; an answered run is no decline', () => {
    const cases = [
      run('nothing', { golden: LINE }),
      run('prefix', { golden: LINE + 'later\r\n', stderr: LINE, goldenOut: 'a\nb\n', stdout: 'a\n' }),
      run('stray', { golden: LINE, stderr: LINE + 'never\r\n' }),
      run('quiet', {}),
      run('answered', { golden: LINE, answered: true }),
    ]
    const d = declineStreams(cases)
    expect(d).toEqual({
      declines: 4,
      stderr: { bothEmpty: 1, empty: 1, prefix: 1, notPrefix: 1 },
      stdout: { bothEmpty: 3, empty: 0, prefix: 1, notPrefix: 0 },
      notPrefix: ['stray/0 stderr'],
    })
    // Each side's case folder is <case>: the same line written in two folders is a prefix.
    expect(cases[1]!.golden.side.root).not.toBe(cases[1]!.rust.side.root)
    expect(declineStreamsLine(d)).toBe('decline streams: 4 declines; stderr 1 both empty, 1 empty, 1 a prefix, 1 not a prefix; stdout 3 both empty, 0 empty, 1 a prefix, 0 not a prefix')
  })

  test("each family's pre-boundary holders are the answered runs of its cases, which hold the lines its C++ writes before its boundary", () => {
    const cases = [run('nothing', { golden: LINE }), run('answered', { golden: LINE, answered: true })]
    const fams = [family('placement', ['nothing', 'answered']), family('slice-mesh', ['nothing']), family('info', [])]
    const verdicts = new Map(
      cases.map((c) => [c.caseId, compareCase({ caseId: c.caseId, golden: c.golden, rust: c.rust, masks: [], families: fams, classOf: NO_CLASS, envelopeOf: NO_CLASS })]),
    )
    expect(verdicts.get('answered')!.runs[0]).toMatchObject({ passed: true })
    expect(verdicts.get('nothing')!.runs[0]).toMatchObject({ declined: true })
    const holders = preBoundaryHolders(parityOf(fams), verdicts)
    expect(holders).toEqual([
      { family: 'placement', runs: ['answered/0'] },
      { family: 'slice-mesh', runs: [] },
      { family: 'info', runs: [] },
    ])
    expect(holders.map(preBoundaryLine)).toEqual(['pre-boundary holders of placement: 1 answered runs (answered/0)', 'pre-boundary holders of slice-mesh: 0 answered runs', 'pre-boundary holders of info: 0 answered runs'])
    expect(preBoundaryLine({ family: 'x', runs: ['a/0', 'b/0', 'c/0', 'd/0', 'e/0', 'f/0'] })).toBe('pre-boundary holders of x: 6 answered runs (a/0, b/0, c/0, d/0, e/0, ...)')
  })
})

describe('the runs after a decline', () => {
  test("rebaseBytes swaps the golden's roots for this pass's, as written, with forward slashes or JSON-escaped, the longest first", () => {
    const from = 'C:\\w\\runs\\record\\cases'
    const to = 'C:\\w\\runs\\certify\\cases'
    // The work folder holds both and stays: a root inside it is still swapped.
    const pairs: [string, string][] = [['C:\\w', 'C:\\w'], [from, to], [`${from}\\c`, `${to}\\c`]]
    const text = (r: string) => `a ${r}\\c\\x.stl b ${r.replace(/\\/g, '/')}/c/y c ${JSON.stringify(`${r}\\c\\out.gcode`)} d C:\\w\\other\n`
    expect(rebaseBytes(Buffer.from(text(from)), pairs).toString()).toBe(text(to))
    expect(rebaseBytes(Buffer.from('no path here\n'), pairs).toString()).toBe('no path here\n')
    // A zip or a binary output is left as it is.
    const zip = Buffer.concat([Buffer.from([0x50, 0x4b, 0x03, 0x04]), Buffer.from(from)])
    expect(rebaseBytes(zip, pairs).equals(zip)).toBe(true)
    const bin = Buffer.concat([Buffer.from(from), Buffer.from([0])])
    expect(rebaseBytes(bin, pairs).equals(bin)).toBe(true)
  })

  const JOB = join('<cases>', 'c', 'job.json')

  /** Case c's folder of the Rust side, with a file of an earlier step (keep.txt) and one the C++ run removes (old.txt). */
  function rustFolder() {
    const r = side('rust')
    const caseDir = join(r.root, 'c')
    mkdirSync(caseDir, { recursive: true })
    writeFileSync(join(caseDir, 'keep.txt'), 'before')
    writeFileSync(join(caseDir, 'old.txt'), 'old')
    return { r, caseDir }
  }

  /** The golden run 0 of case c: it wrote result.json and out/z.txt naming its folder, removed old.txt, and said so on stdout. */
  function goldenRun(o: { args?: string[] } = {}) {
    const g = side('golden')
    const run = keep(g, 'c', 0, { code: 0, args: o.args ?? ['run', '--job', JOB], stdout: 'sliced <case>\n', gone: ['old.txt'], files: { 'result.json': 'ok 6.2 <case>\n', 'out/z.txt': '6.2\n' } })
    return { g, cases: [caseRecord('c', [run])] }
  }

  /** The Rust program as a process: it writes result.json and progress.ndjson, changes keep.txt when told, and exits with `code`. */
  const program =
    (code: number, o: { timedOut?: boolean; touch?: boolean } = {}): RunProcess =>
    async (_exe, _args, p) => {
      writeFileSync(join(p.cwd, 'result.json'), 'not-ported\n')
      writeFileSync(join(p.cwd, 'progress.ndjson'), PROGRESS_MODEL + FAILED)
      if (o.touch) writeFileSync(join(p.cwd, 'keep.txt'), 'changed by the decline')
      return { code, timedOut: o.timedOut ?? false, stdout: Buffer.from('rust says\n'), stderr: Buffer.alloc(0) }
    }

  test("a declined run is followed by the golden run's outputs, its paths this pass's, and the case is told the C++ run's exit code and stdout", async () => {
    const { r, caseDir } = rustFolder()
    const { g, cases } = goldenRun()
    const runs: RecordedRun[] = []
    const carry = goldenCarry({ golden: g, goldens: cases, rust: r })
    const exec = recordingExec({ exe: 'rs', caseDir, root: r.root, store: join(r.store, 'c'), env: { PATH: 'x' }, timeoutMs: 1000, runs, run: program(-8, { touch: true }), after: (x) => carry({ caseId: 'c', ...x }) })
    expect(await exec(['run', '--job', join(caseDir, 'job.json')])).toEqual({ code: 0, timedOut: false, stdout: `sliced ${caseDir}\n` })
    // The folder holds what the C++ run left: its files, written for this folder, and old.txt gone.
    expect(readFileSync(join(caseDir, 'result.json'), 'utf8')).toBe(`ok 6.2 ${caseDir}\n`)
    expect(readFileSync(join(caseDir, 'out', 'z.txt'), 'utf8')).toBe('6.2\n')
    expect(existsSync(join(caseDir, 'old.txt'))).toBe(false)
    // A file only the decline made is gone; one it changed that was there before stays (its decline is wrong for it).
    expect(existsSync(join(caseDir, 'progress.ndjson'))).toBe(false)
    expect(readFileSync(join(caseDir, 'keep.txt'), 'utf8')).toBe('changed by the decline')
    // What is kept of the run, and judged, is the decline's own.
    expect(runs).toEqual([{ index: 0, args: ['run', '--job', JOB], extraEnv: [], code: -8, timedOut: false, files: ['keep.txt', 'progress.ndjson', 'result.json'], gone: [], carried: true }])
    expect(readFileSync(join(runStore(join(r.store, 'c'), 0).files, 'result.json'), 'utf8')).toBe('not-ported\n')
    expect(readFileSync(runStore(join(r.store, 'c'), 0).stdout, 'utf8')).toBe('rust says\n')
  })

  test('a run that is not declined, a decline that timed out, and one whose golden run differs or is missing are not carried', async () => {
    const cases: [string, RunProcess, ReturnType<typeof goldenRun>][] = [
      ['answered', program(0), goldenRun()],
      ['ended -2', program(-2), goldenRun()],
      ['timed out', program(-8, { timedOut: true }), goldenRun()],
      ['other command line', program(-8), goldenRun({ args: ['run', '--job', join('<cases>', 'c', 'other.json')] })],
      ['no golden run', program(-8), { g: side('golden'), cases: [] }],
    ]
    for (const [, run, golden] of cases) {
      rmSync(join(work, 'rust'), { recursive: true, force: true })
      const { r, caseDir } = rustFolder()
      const runs: RecordedRun[] = []
      const carry = goldenCarry({ golden: golden.g, goldens: golden.cases, rust: r })
      const exec = recordingExec({ exe: 'rs', caseDir, root: r.root, store: join(r.store, 'c'), env: { PATH: 'x' }, timeoutMs: 1000, runs, run, after: (x) => carry({ caseId: 'c', ...x }) })
      const answer = await exec(['run', '--job', join(caseDir, 'job.json')])
      expect(answer.stdout).toBe('rust says\n')
      expect(readFileSync(join(caseDir, 'result.json'), 'utf8')).toBe('not-ported\n')
      expect(existsSync(join(caseDir, 'old.txt'))).toBe(true)
      expect(runs[0]!.carried).toBeUndefined()
    }
  })

  // A smoke case like pocket-pause: its layers run gives a print Z and the folder, and its slice run is made from them.
  const twoStep: SmokeCase = {
    id: 'c',
    title: 'a slice at the print Z its layers run gave',
    async run(env) {
      mkdirSync(join(env.dir, 'layers'), { recursive: true })
      await env.exec(['run', '--job', join(env.dir, 'layers', 'job.json')])
      const plan = JSON.parse(readFileSync(join(env.dir, 'layers', 'result.json'), 'utf8')) as { ok: boolean; printZ?: number; dir?: string }
      if (!plan.ok) return { problems: ['the layers run failed'], warnings: [] }
      writeFileSync(join(env.dir, 'job.json'), JSON.stringify({ command: 'slice', printZ: plan.printZ, from: plan.dir }))
      await env.exec(['run', '--job', join(env.dir, 'job.json')])
      return { problems: [], warnings: [] }
    },
  }
  const PAST_DONE = PROGRESS_ARRANGE + '{"stage":"slice","percent":14.0,"message":"slicing"}\n{"stage":"done","percent":100.0,"message":"done"}\n'
  const DECLINE_SLICE = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', '["slice-mesh"]')
  const isLayers = (args: string[]) => args[2]!.endsWith(join('layers', 'job.json'))
  const ran = { code: 0, timedOut: false, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0) }
  /** The slice of the job in the case's folder: its result, its progress and out.txt, `dz` above the job's print Z. */
  const slice = (cwd: string, engine: string, dz = 0) => {
    const job = JSON.parse(readFileSync(join(cwd, 'job.json'), 'utf8')) as { printZ: number; from: string }
    writeFileSync(join(cwd, 'result.json'), result('  "ok": true', engine))
    writeFileSync(join(cwd, 'progress.ndjson'), PAST_DONE)
    writeFileSync(join(cwd, 'out.txt'), `sliced at ${job.printZ + dz} from ${job.from}\n`)
  }
  const cpp: RunProcess = async (_exe, args, o) => {
    if (isLayers(args)) {
      writeFileSync(join(o.cwd, 'layers', 'result.json'), result(`  "ok": true,\n  "printZ": 6.2,\n  "dir": ${JSON.stringify(o.cwd)}`, CPP_ENGINE))
      writeFileSync(join(o.cwd, 'layers', 'progress.ndjson'), PAST_DONE)
    } else slice(o.cwd, CPP_ENGINE)
    return ran
  }
  /** The Rust program: it declines the layers run at placement, and the slice run as `then` says. */
  const rs =
    (then: 'same' | 'other' | 'decline' | 'decline-and-log'): RunProcess =>
    async (_exe, args, o) => {
      const at = isLayers(args) ? join(o.cwd, 'layers') : o.cwd
      if (isLayers(args) || then.startsWith('decline')) {
        writeFileSync(join(at, 'result.json'), result(DECLINE_SLICE, RUST_ENGINE))
        writeFileSync(join(at, 'progress.ndjson'), PROGRESS_ARRANGE + FAILED)
        return { ...ran, code: -8, stderr: Buffer.from(then === 'decline-and-log' && !isLayers(args) ? 'x' : '') }
      }
      slice(o.cwd, RUST_ENGINE, then === 'other' ? 0.1 : 0)
      return ran
    }
  const sets = (() => {
    const preset = (type: 'machine' | 'process' | 'filament', name: string, more: Record<string, unknown> = {}): FlatPreset => ({ type, name, from: 'system', ...more })
    const set: PresetSet = { machine: preset('machine', 'M'), process: preset('process', 'P', { compatible_printers: ['M'] }), filaments: [preset('filament', 'F')] }
    return Object.fromEntries(Object.keys(SMOKE_PRESETS).map((k) => [k, set])) as SmokePresetSets
  })()
  const families = [family('preset-conditions', ['c']), family('model-load', ['c']), family('placement', ['c']), family('slice-mesh', ['c'])]
  /** Both passes of twoStep, the Rust one carried on past its declines unless `carry` is false; each run's front-objects dump as `dumps` gives it. */
  async function passes(then: Parameters<typeof rs>[0], o: { carry?: boolean; dumps?: (string | undefined)[] } = {}) {
    const g = side('golden')
    const r = side('rust')
    const common = { cases: [twoStep], sets, lock: {} as never, baseEnv: {}, platform: 'linux' as const, timeoutMs: 1000, log: () => {} }
    const goldens = await runEngineCases({ label: 'oracle', exe: 'cpp', resources: g.resources, libraryPath: 'lib' }, { ...common, root: g.root, store: g.store, run: cpp })
    const rust = await runEngineCases({ label: 'rust', exe: 'rs', resources: r.resources, libraryPath: 'lib' }, { ...common, root: r.root, store: r.store, run: rs(then), ...(o.carry === false ? {} : { after: goldenCarry({ golden: g, goldens, rust: r }) }) })
    // The front-objects dumps: the C++ runs' name their case folder, and the Rust runs' theirs or what `dumps` gives.
    const put = (s: Side, record: CaseRecord, i: number, value: string) => {
      const d = runStore(join(s.store, 'c'), i).dumps
      mkdirSync(d, { recursive: true })
      writeFileSync(join(d, 'front-objects-1.sop1'), dump(value))
      record.runs[i]!.dumps = ['front-objects-1.sop1']
    }
    for (const i of [0, 1]) put(g, goldens[0]!, i, `${join(g.root, 'c')}/post.py`)
    rust[0]!.runs.forEach((_, i) => {
      const value = o.dumps ? o.dumps[i] : `${join(r.root, 'c')}/post.py`
      if (value !== undefined) put(r, rust[0]!, i, value)
    })
    const v = compareCase({ caseId: 'c', golden: { side: g, record: goldens[0] }, rust: { side: r, record: rust[0] }, masks: [], families, classOf: NO_CLASS, envelopeOf: NO_CLASS })
    return { g, r, goldens, rust, v, verdicts: familyVerdicts(parityOf(families), new Map([['c', v]])) }
  }
  const passed = (p: Awaited<ReturnType<typeof passes>>, name: string) => p.verdicts.find((f) => f.name === name)!.passed

  test('without it, a case whose earlier run is declined never makes its later run', async () => {
    const p = await passes('same', { carry: false })
    expect(p.rust[0]!.runs).toHaveLength(1)
    expect(p.v.runs[1]).toEqual({ run: 1, passed: false, declined: false, boundaries: [], problems: ['only the C++ program ran it'] })
  })

  test('with it, the later run is made from what the C++ program had, and is the program\'s own, judged in full', async () => {
    const p = await passes('same')
    expect(p.rust[0]!.runs.map((x) => x.carried)).toEqual([true, undefined])
    // The slice job is the C++ run's, in this pass's folder: the print Z and the folder the layers run gave.
    const job = JSON.parse(readFileSync(join(p.r.root, 'c', 'job.json'), 'utf8')) as Record<string, unknown>
    expect(job).toEqual({ command: 'slice', printZ: 6.2, from: join(p.r.root, 'c') })
    // The declined run is information; the later one passes on its own outputs.
    expect(p.v.runs).toEqual([
      { run: 0, passed: false, declined: true, boundaries: ['front-objects'], problems: [] },
      { run: 1, passed: true, declined: false, boundaries: [], problems: [] },
    ])
    for (const n of ['model-load', 'placement']) expect(passed(p, n)).toBe(true)
    expect(passed(p, 'slice-mesh')).toBe(false)
  })

  test('a later run made so is never passed for being there: an output that differs, a wrong dump or a wrong decline fails it', async () => {
    const other = await passes('other')
    expect(other.v.runs[1]).toMatchObject({ passed: false, declined: false })
    expect(other.v.runs[1]!.problems.join()).toContain('files/out.txt')
    for (const n of ['model-load', 'placement']) expect(passed(other, n)).toBe(false)
    // A right decline of the later run passes the boundary's families when it wrote the dump the C++ run did.
    const declined = await passes('decline')
    expect(declined.v.runs[1]).toEqual({ run: 1, passed: false, declined: true, boundaries: ['front-objects'], problems: [] })
    for (const n of ['model-load', 'placement']) expect(passed(declined, n)).toBe(true)
    expect(passed(declined, 'slice-mesh')).toBe(false)
    // Its dump missing, the decline passes nothing; its dump different, or a byte on stderr, it is wrong.
    const noDump = await passes('decline', { dumps: [`${join(side('rust').root, 'c')}/post.py`, undefined] })
    expect(noDump.v.runs[1]).toMatchObject({ declined: true, boundaries: [] })
    expect(passed(noDump, 'model-load')).toBe(false)
    const wrongDump = await passes('decline', { dumps: [`${join(side('rust').root, 'c')}/post.py`, 'elsewhere/post.py'] })
    expect(wrongDump.v.runs[1]).toMatchObject({ passed: false, declined: false })
    expect(wrongDump.v.runs[1]!.problems.join()).toContain("dumps/front-objects-1.sop1: differs from the C++ run's")
    expect(passed(wrongDump, 'placement')).toBe(false)
    const logged = await passes('decline-and-log')
    expect(logged.v.runs[1]).toMatchObject({ passed: false, declined: false })
    expect(logged.v.runs[1]!.problems.join()).toContain('it wrote to stderr what the C++ run did not write there')
    expect(passed(logged, 'model-load')).toBe(false)
  })
})

describe('the program', () => {
  test('is what bun run rs:build makes', () => {
    expect(rustProgramPath(join('x', 'repo'), 'win32')).toBe(join('x', 'repo', 'target', 'debug', 'sketch-orca-rs.exe'))
    expect(rustProgramPath(join('x', 'repo'), 'linux')).toBe(join('x', 'repo', 'target', 'debug', 'sketch-orca-rs'))
  })

  test("is refused when what it is built from is newer than it, a crate's tests and parity.json aside", () => {
    const repo = join(work, 'repo')
    const files = ['Cargo.toml', 'Cargo.lock', 'crates/a/Cargo.toml', 'crates/a/src/lib.rs', 'crates/a/src/data/x.json', 'crates/a/tests/t.rs', 'crates/a/README.md', 'parity/parity.json', 'target/debug/sketch-orca-rs.exe']
    for (const f of files) {
      mkdirSync(join(repo, ...f.split('/').slice(0, -1)), { recursive: true })
      writeFileSync(join(repo, ...f.split('/')), '')
    }
    const exe = join(repo, 'target', 'debug', 'sketch-orca-rs.exe')
    const sources = rustSources(repo)
    expect(sources.map((s) => s.slice(repo.length + 1).split('\\').join('/')).sort()).toEqual(['Cargo.lock', 'Cargo.toml', 'crates/a/Cargo.toml', 'crates/a/src/data/x.json', 'crates/a/src/lib.rs'])
    const old = new Date(Date.UTC(2026, 0, 1))
    const built = new Date(Date.UTC(2026, 0, 2))
    for (const f of files) utimesSync(join(repo, ...f.split('/')), old, old)
    utimesSync(exe, built, built)
    expect(staleSources(exe, sources)).toEqual([])
    const later = new Date(Date.UTC(2026, 0, 3))
    for (const f of ['crates/a/src/lib.rs', 'crates/a/tests/t.rs', 'parity/parity.json']) utimesSync(join(repo, ...f.split('/')), later, later)
    expect(staleSources(exe, sources)).toEqual([join(repo, 'crates', 'a', 'src', 'lib.rs')])
  })
})

describe('the decline against the corpus cases', () => {
  const families = [family('intake', []), family('model-load', ['m'], true), family('supports', ['m']), family('raft', ['m'])]
  const declining = (missing: string[], code = -8) => {
    const r = side('rust')
    const fields = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', JSON.stringify(missing))
    return { side: r, record: caseRecord('m', [keep(r, 'm', 0, { code, files: { 'progress.ndjson': PROGRESS_MODEL + FAILED, 'result.json': result(fields, RUST_ENGINE) } })]) }
  }
  /** The C++ run of case m, its progress `progress`, its result `code`. */
  const golden = (progress: string, code: string) => {
    const g = side('golden')
    return { side: g, record: caseRecord('m', [keep(g, 'm', 0, { code: -2, files: { 'progress.ndjson': progress, 'result.json': result(failedFields(code), CPP_ENGINE) } })]) }
  }
  const past = () => golden(PROGRESS_PAST, 'empty-plate')

  test('every uncertified family a case reaches leads to not-ported, naming it', () => {
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports', 'raft'], families, rust: declining(['supports', 'raft']), golden: past() })).toEqual([])
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports', 'raft'], families, rust: declining(['supports']), golden: past() })).toEqual(['m/0: its decline does not name raft, which the job reaches and parity.json does not certify'])
    // The low byte of -8 is a decline too.
    expect(declineGaps({ caseId: 'm', reaches: ['supports'], families, rust: declining(['supports'], 248), golden: past() })).toEqual([])
  })

  test('a run that is not declined, or not run, is a gap; a case of certified families alone is not held', () => {
    const r = side('rust')
    const ran = { side: r, record: caseRecord('m', [keep(r, 'm', 0, { code: 0, files: { 'result.json': result('  "ok": true', RUST_ENGINE) } })]) }
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: ran, golden: past() }).join()).toContain('was not declined')
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: ran, golden: { side: side('golden'), record: undefined } }).join()).toContain('was not declined')
    expect(declineGaps({ caseId: 'm', reaches: ['supports'], families, rust: { side: r, record: undefined }, golden: past() })).toEqual(['m: the Rust program did not run it'])
    expect(declineGaps({ caseId: 'm', reaches: ['intake', 'model-load'], families, rust: ran, golden: past() })).toEqual([])
  })

  test('a run that is not declined is no gap when its C++ run ended within what the program does', () => {
    const r = side('rust')
    const refused = { side: r, record: caseRecord('m', [keep(r, 'm', 0, { code: -18, files: { 'progress.ndjson': PROGRESS_PRESETS + FAILED, 'result.json': result(failedFields('config-invalid'), RUST_ENGINE) } })]) }
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: refused, golden: golden(PROGRESS_PRESETS + FAILED, 'config-invalid') })).toEqual([])
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: refused, golden: golden(PROGRESS_MODEL + FAILED, 'file-not-found') })).toEqual([])
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: refused, golden: golden(PROGRESS_MODEL + FAILED, 'model-unreadable') })).toEqual([])
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: refused, golden: golden(PROGRESS_ARRANGE + FAILED, 'not-printable') })).toEqual([])
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: refused, golden: golden(PROGRESS_PAST, 'empty-plate') }).join()).toContain('was not declined')
  })

  test('is held against parity.json as the same run raises it: a family certified now needs no decline', () => {
    const r = side('rust')
    const ran = { side: r, record: caseRecord('m', [keep(r, 'm', 0, { code: 0, files: { 'result.json': result('  "ok": true', RUST_ENGINE) } })]) }
    const p = parityOf(families)
    expect(declineGaps({ caseId: 'm', reaches: ['supports'], families: p.families, rust: ran, golden: past() }).join()).toContain('was not declined')
    const now: CaseVerdict = { id: 'm', runs: [{ run: 0, passed: true, declined: false, boundaries: [], problems: [] }] }
    const raised = raiseCertified(p, familyVerdicts(p, new Map([['m', now]])))
    expect(raised.raised).toEqual(['supports', 'raft'])
    expect(declineGaps({ caseId: 'm', reaches: ['supports'], families: raised.parity.families, rust: ran, golden: past() })).toEqual([])
  })

  test('is held for jobs, examples and refusals, whose families are worked out for their one job, and not for smoke cases', () => {
    expect([...DECLINE_HELD_KINDS]).toEqual(['job', 'example', 'refusal'])
  })
})

describe('the oracle behind the goldens', () => {
  const oracle = { exeSha256: 'a'.repeat(64), cli: 'c'.repeat(64) }
  const neutral = (exeSha256: string, passed = true) => ({ passed, engines: { head: {} as never, oracle: { dir: '', exeSha256, oraclePatches: [], sourceDigest: '' } } })
  test('passes when parity neutral passed with the staged oracle and the goldens were recorded with it', () => {
    expect(oracleChainProblems({ oracle, neutral: neutral(oracle.exeSha256), goldens: { oracle } })).toEqual([])
  })

  test('refuses goldens of an oracle built from another cli/, or that do not name their oracle', () => {
    expect(oracleChainProblems({ oracle, neutral: neutral(oracle.exeSha256), goldens: { oracle: { ...oracle, cli: 'd'.repeat(64) } } }).join()).toContain('recorded with another oracle')
    expect(oracleChainProblems({ oracle, neutral: neutral(oracle.exeSha256), goldens: { oracle: { ...oracle, exeSha256: 'b'.repeat(64) } } }).join()).toContain('run parity record')
    expect(oracleChainProblems({ oracle, neutral: neutral(oracle.exeSha256), goldens: {} }).join()).toContain('do not name the oracle')
  })

  test('refuses when parity neutral did not run, ran with another oracle or failed', () => {
    expect(oracleChainProblems({ oracle, neutral: undefined, goldens: { oracle } }).join()).toContain('parity neutral has not run')
    expect(oracleChainProblems({ oracle, neutral: neutral('b'.repeat(64)), goldens: { oracle } }).join()).toContain('another oracle than the one staged; run parity neutral')
    expect(oracleChainProblems({ oracle, neutral: neutral(oracle.exeSha256, false), goldens: { oracle } }).join()).toContain('did not pass')
  })
})

describe('the command', () => {
  test('is a parity command that holds the work folder', () => {
    const c = checkParityModule(cmd, 'certify')
    expect(c.locked).toBe(true)
    expect(c.usage).toMatch(/^certify \[--check\] \[--program <exe>\]/)
  })
})
