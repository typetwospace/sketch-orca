// parity certify (scripts/lib/parity/certify.ts, cmd-certify.ts): sketch-orca-rs's runs against the
// goldens. The engine block is made alike (version, implementation); a result is compared as a JSON
// value; an output classed R passes within the envelope of the engine's own variance and nowhere
// else; a decline (not-ported, -8) passes for the front families alone, and only for a job the C++
// got past reading; a family is certified when every run of every case passes; parity.json's flags
// are raised and never lowered, and a certified family that fails is a regression; every corpus
// job that reaches an uncertified family must be declined naming it; goldens count only when
// parity neutral passed with the staged oracle and they were recorded with it. Hermetic: the runs
// are written to a temporary folder.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  compareCase,
  declineGaps,
  declineProblems,
  familyVerdicts,
  FRONT_FAMILIES,
  isResult,
  normaliseEngine,
  oracleChainProblems,
  progressStages,
  raiseCertified,
  rustProgramPath,
  rustSources,
  sameJsonValue,
  staleSources,
  withinEnvelope,
  type CaseVerdict,
  type Side,
} from '../scripts/lib/parity/certify.ts'
import { checkParityModule } from '../scripts/lib/parity/dispatch.ts'
import type { OutputEnvelope } from '../scripts/lib/parity/envelope.ts'
import type { FamilyEntry, ParityDocument } from '../scripts/lib/parity/record.ts'
import type { CaseRecord, RecordedRun } from '../scripts/lib/parity/same.ts'
import * as cmd from '../scripts/lib/parity/cmd-certify.ts'

let work = ''
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-certify-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

function side(name: string): Side {
  return { store: join(work, name, 'store'), root: join(work, name, 'cases'), resources: join(work, 'resources'), work: join(work, name) }
}

/** Keeps a run of `caseId` in `s`'s store as recordingExec does, and returns its record. */
function keep(s: Side, caseId: string, index: number, o: { code: number; files?: Record<string, string>; stdout?: string; stderr?: string; gone?: string[]; args?: string[] }): RecordedRun {
  const dir = join(s.store, caseId, String(index))
  mkdirSync(join(dir, 'files'), { recursive: true })
  writeFileSync(join(dir, 'stdout'), o.stdout ?? '')
  writeFileSync(join(dir, 'stderr'), o.stderr ?? '')
  for (const [rel, text] of Object.entries(o.files ?? {})) {
    mkdirSync(join(dir, 'files', ...rel.split('/').slice(0, -1)), { recursive: true })
    writeFileSync(join(dir, 'files', ...rel.split('/')), text.split('<case>').join(join(s.root, caseId)))
  }
  return { index, args: o.args ?? ['run', '--job', '<cases>/c/job.json'], extraEnv: [], code: o.code, timedOut: false, files: Object.keys(o.files ?? {}).sort(), gone: o.gone ?? [] }
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
const PROGRESS_PAST = PROGRESS_JOB + '{"stage":"presets","percent":2.0,"message":"loading the presets"}\n{"stage":"failed","percent":2.0,"message":"x"}\n'

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
    expect(v.runs).toEqual([{ run: 0, passed: true, declined: false, problems: [] }])
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

  function declined(o: { goldenProgress?: string; rustFiles?: Record<string, string>; code?: number; stderr?: string; families?: FamilyEntry[] }) {
    const g = side('golden')
    const r = side('rust')
    const cpp = keep(g, 'c', 0, { code: -3, files: { 'progress.ndjson': o.goldenProgress ?? PROGRESS_PAST, 'result.json': result('  "ok": false,\n  "code": "file-not-found",\n  "message": "x",\n  "exitCode": -3', CPP_ENGINE) } })
    const rust = keep(r, 'c', 0, { code: o.code ?? -8, stderr: o.stderr ?? '', files: o.rustFiles ?? { 'progress.ndjson': PROGRESS_JOB, 'result.json': result(NOT_PORTED_FIELDS, RUST_ENGINE) } })
    return { g, r, cpp, rust, problems: declineProblems({ caseId: 'c', golden: { side: g, run: cpp }, rust: { side: r, run: rust }, families: o.families ?? families }) }
  }

  test('of a job the C++ got past reading counts as passing for a front family', () => {
    const d = declined({})
    expect(d.problems).toEqual([])
    const v = compareCase({ caseId: 'c', golden: { side: d.g, record: caseRecord('c', [d.cpp]) }, rust: { side: d.r, record: caseRecord('c', [d.rust]) }, masks: [], families, classOf: NO_CLASS, envelopeOf: NO_CLASS })
    expect(v.runs).toEqual([{ run: 0, passed: false, declined: true, problems: [] }])
    const verdicts = familyVerdicts(parityOf(families), new Map([['c', v]]))
    expect(verdicts.find((f) => f.name === 'intake')).toMatchObject({ passed: true, declined: 1, certified: true })
  })

  test('the exit code -8 is seen as its low byte, 248, too', () => {
    expect(declined({ code: 248 }).problems).toEqual([])
  })

  test('of a job the C++ refused while reading it does not', () => {
    expect(declined({ goldenProgress: PROGRESS_JOB + '{"stage":"failed","percent":1.0,"message":"x"}\n' }).problems.join()).toContain('should have been refused')
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

  test('never passes a family after the front end', () => {
    const d = declined({})
    const v: CaseVerdict = { id: 'm', runs: [{ run: 0, passed: false, declined: true, problems: [] }] }
    const verdicts = familyVerdicts(parityOf(families), new Map([['m', v]]))
    expect(verdicts.find((f) => f.name === 'model-load')).toMatchObject({ passed: false, certified: false })
    expect(d.problems).toEqual([])
    expect(FRONT_FAMILIES).toEqual(['version', 'config-def', 'intake'])
  })

  test('progress stages are read line by line', () => {
    expect(progressStages(Buffer.from(PROGRESS_PAST.replace(/\n/g, '\r\n')))).toEqual(['job', 'presets', 'failed'])
    expect(progressStages(undefined)).toEqual([])
  })
})

describe('families and parity.json', () => {
  const pass: CaseVerdict = { id: 'a', runs: [{ run: 0, passed: true, declined: false, problems: [] }] }
  const fail: CaseVerdict = { id: 'b', runs: [{ run: 0, passed: true, declined: false, problems: [] }, { run: 1, passed: false, declined: false, problems: ['exit: 1 against 2'] }] }

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

describe('the decline against the corpus jobs', () => {
  const families = [family('intake', []), family('model-load', ['m'], true), family('supports', ['m']), family('raft', ['m'])]
  const declining = (missing: string[], code = -8) => {
    const r = side('rust')
    const fields = NOT_PORTED_FIELDS.replace('[\n    "model-load"\n  ]', JSON.stringify(missing))
    return { side: r, record: caseRecord('m', [keep(r, 'm', 0, { code, files: { 'progress.ndjson': PROGRESS_JOB, 'result.json': result(fields, RUST_ENGINE) } })]) }
  }

  test('every uncertified family a job reaches leads to not-ported, naming it', () => {
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports', 'raft'], families, rust: declining(['supports', 'raft']) })).toEqual([])
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports', 'raft'], families, rust: declining(['supports']) })).toEqual(['m/0: its decline does not name raft, which the job reaches and parity.json does not certify'])
    // The low byte of -8 is a decline too.
    expect(declineGaps({ caseId: 'm', reaches: ['supports'], families, rust: declining(['supports'], 248) })).toEqual([])
  })

  test('a run that is not declined, or not run, is a gap; a job of certified families alone is not held', () => {
    const r = side('rust')
    const ran = { side: r, record: caseRecord('m', [keep(r, 'm', 0, { code: 0, files: { 'result.json': result('  "ok": true', RUST_ENGINE) } })]) }
    expect(declineGaps({ caseId: 'm', reaches: ['model-load', 'supports'], families, rust: ran }).join()).toContain('was not declined')
    expect(declineGaps({ caseId: 'm', reaches: ['supports'], families, rust: { side: r, record: undefined } })).toEqual(['m: the Rust program did not run it'])
    expect(declineGaps({ caseId: 'm', reaches: ['intake', 'model-load'], families, rust: ran })).toEqual([])
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
