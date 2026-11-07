// parity record's dumps (scripts/lib/parity/record.ts, neutral.ts): the oracle's passes run with
// SKETCH_ORCA_ORACLE_DUMP set to a fresh folder per engine run, keep the dumps of the boundaries in
// KEPT_DUMPS beside their runs (<pass>/<case>/<n>/dumps/) and name them in the run's record, remove
// every other dump as soon as the run ends, class each kept dump across the thread counts (B or T,
// the RUN record left out), and the goldens keep them and name their classes; a pass key changes
// with the boundaries kept; a dump's paths are made portable inside its strings. Hermetic:
// temporary folders, fake engine processes and hand-made SOP1 files only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { knobRunner, parseHead, portableDump, readSop1, Sop1Reader } from '../scripts/lib/parity/neutral.ts'
import {
  classifyDumps,
  collectDumps,
  dumpBoundary,
  dumpBytes,
  dumpNames,
  dumpsRoot,
  KEPT_DUMPS,
  keepGoldens,
  keptDump,
  passKey,
  portableDumpBytes,
  readGoldens,
  type Pass,
} from '../scripts/lib/parity/record.ts'
import { recordingExec, type CaseRecord, type RecordedRun, type RunProcess } from '../scripts/lib/parity/same.ts'

let work = ''
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-record-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

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
const i32 = (v: number) => {
  const b = Buffer.alloc(4)
  b.writeInt32LE(v)
  return b
}
const str = (s: string) => Buffer.concat([u32(Buffer.byteLength(s)), Buffer.from(s, 'utf8')])
const bool = (v: boolean) => Buffer.from([v ? 1 : 0])

/** An SOP1 file of `records`, as OracleDump.cpp writes one. */
function sop1(records: [string, Buffer][]): Buffer {
  const parts: Buffer[] = [Buffer.from('SOP1', 'latin1'), u32(1)]
  for (const [kind, payload] of [...records, ['END ', u64(records.length)] as [string, Buffer]]) parts.push(Buffer.from(kind, 'latin1'), u64(payload.length), payload)
  return Buffer.concat(parts)
}

function head(name: string, n: number): Buffer {
  return Buffer.concat([str('front'), str(name), i64(-1), u32(n)])
}

function runRecord(threads: string): Buffer {
  const knob = (k: string, v: string | undefined) => Buffer.concat([str(k), bool(v !== undefined), str(v ?? '')])
  return Buffer.concat([u64(2), knob('SKETCH_ORCA_ORACLE_TIME', '1767225600'), knob('SKETCH_ORCA_ORACLE_THREADS', threads), i32(Number(threads)), u64(Number(threads)), i32(1)])
}

/** A CONF record of one string option. */
function conf(scope: string, key: string, value: string): Buffer {
  return Buffer.concat([str(scope), i64(-1), i64(-1), u64(1), str(key), u32(5), str(value), u64(0)])
}

/** A front-objects dump: HEAD, RUN at `threads`, and a CONF whose one value is `value`. */
function frontObjects(threads: string, value = 'x'): Buffer {
  return sop1([
    ['HEAD', head('objects', 1)],
    ['RUN ', runRecord(threads)],
    ['CONF', conf('run', 'post_process', value)],
  ])
}

const ok = { code: 0, timedOut: false, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0) }

describe('the dumps a pass keeps', () => {
  test('are front-objects in phase O2, known by the name part of their file', () => {
    expect([...KEPT_DUMPS]).toEqual(['front-objects'])
    expect(dumpBoundary('front-objects-1.sop1')).toBe('front-objects')
    expect(dumpBoundary('object0-posSlice-12.sop1')).toBe('object0-posSlice')
    expect(dumpBoundary('notes.txt')).toBeUndefined()
    expect(keptDump('front-objects-2.sop1')).toBe(true)
    expect(keptDump('front-config-1.sop1')).toBe(false)
    expect(keptDump('print-psWipeTower-1.sop1', ['print-psWipeTower'])).toBe(true)
  })

  test('a fresh folder per engine run, the other dumps removed as the run ends, the kept ones beside the run in the store', async () => {
    const dumps = dumpsRoot(work, 'oracle-p1')
    const store = join(work, 'oracle-p1')
    const caseDir = join(work, 'cases', 'cube')
    mkdirSync(caseDir, { recursive: true })
    const seen: string[] = []
    // The oracle writes every boundary's dump; the folder it is given is empty each time.
    const fake: RunProcess = async (_exe, _args, o) => {
      const folder = o.env.SKETCH_ORCA_ORACLE_DUMP!
      seen.push(folder)
      expect(readdirSync(folder)).toEqual([])
      writeFileSync(join(folder, 'front-objects-1.sop1'), frontObjects(o.env.SKETCH_ORCA_ORACLE_THREADS!))
      writeFileSync(join(folder, 'front-config-1.sop1'), 'c')
      writeFileSync(join(folder, 'object0-posSlice-1.sop1'), 's')
      writeFileSync(join(caseDir, 'result.json'), '{}')
      return ok
    }
    const runs: RecordedRun[] = []
    const run = knobRunner({ SKETCH_ORCA_ORACLE_THREADS: '1' }, dumps, fake, (f) => keptDump(f))
    const exec = recordingExec({ exe: 'oracle', caseDir, root: join(work, 'cases'), store: join(store, 'cube'), env: { PATH: 'x' }, timeoutMs: 1000, runs, run })
    await exec(['run'])
    // Only the kept boundary is left in the run's folder once it ended.
    expect(readdirSync(join(dumps, 'cube', '0'))).toEqual(['front-objects-1.sop1'])
    await exec(['run'])
    expect(seen).toEqual([join(dumps, 'cube', '0'), join(dumps, 'cube', '1')])
    const cases: CaseRecord[] = [{ id: 'cube', ok: true, problems: [], warnings: [], runs }]
    collectDumps(dumps, store, cases)
    expect(existsSync(dumps)).toBe(false)
    expect(runs.map((r) => r.dumps)).toEqual([['front-objects-1.sop1'], ['front-objects-1.sop1']])
    expect(readdirSync(join(store, 'cube', '0', 'dumps'))).toEqual(['front-objects-1.sop1'])
    expect(dumpBytes(store, 'cube', runs[1], 'front-objects-1.sop1')!.equals(frontObjects('1'))).toBe(true)
    expect(dumpBytes(store, 'cube', runs[1], 'front-config-1.sop1')).toBeUndefined()
    // The run's own outputs are kept as before.
    expect(runs[0]!.files).toEqual(['result.json'])
  })

  test('a run that wrote no kept dump names none', () => {
    const store = join(work, 's')
    const cases: CaseRecord[] = [{ id: 'v', ok: true, problems: [], warnings: [], runs: [{ index: 0, args: [], extraEnv: [], code: 0, timedOut: false, files: [], gone: [] }] }]
    collectDumps(join(work, 'none'), store, cases)
    expect(cases[0]!.runs[0]!.dumps).toEqual([])
    expect(existsSync(join(store, 'v', '0', 'dumps'))).toBe(false)
    expect(dumpNames([cases[0]!.runs[0], undefined])).toEqual([])
  })

  test("a pass's key changes with the boundaries it keeps, and not for a pass that keeps none", () => {
    const base = { exeSha256: 'a', knobs: { K: '1' }, corpusSha256: 'c', caseIds: ['x'], root: 'r' }
    expect(passKey({ ...base, dumps: [] })).toBe(passKey(base))
    expect(passKey({ ...base, dumps: ['front-objects'] })).not.toBe(passKey(base))
    expect(passKey({ ...base, dumps: ['front-objects', 'print-psWipeTower'] })).not.toBe(passKey({ ...base, dumps: ['front-objects'] }))
  })
})

describe('the dumps across the thread counts', () => {
  /** A pass of one case with one run that kept `dump` (or none), in its store under `work`. */
  function pass(label: string, dump: Buffer | undefined): Pass {
    const run: RecordedRun = { index: 0, args: [], extraEnv: [], code: 0, timedOut: false, files: [], gone: [], dumps: dump ? ['front-objects-1.sop1'] : [] }
    if (dump) {
      mkdirSync(join(work, label, 'cube', '0', 'dumps'), { recursive: true })
      writeFileSync(join(work, label, 'cube', '0', 'dumps', 'front-objects-1.sop1'), dump)
    }
    return { label, knobs: {}, cases: [{ id: 'cube', ok: true, problems: [], warnings: [], runs: [run] }] }
  }
  const stores = (label: string) => join(work, label)

  test('B when every count wrote it the same, its RUN record left out', () => {
    const passes = [pass('oracle-p1', frontObjects('1')), pass('oracle-p8', frontObjects('8')), pass('engine-1', undefined)]
    expect(classifyDumps(passes, stores)).toEqual([{ case: 'cube', run: 0, dump: 'front-objects-1.sop1', class: 'B' }])
  })

  test('T when a record differs, or a count did not write it', () => {
    expect(classifyDumps([pass('oracle-p1', frontObjects('1')), pass('oracle-p8', frontObjects('8', 'y'))], stores)).toEqual([
      { case: 'cube', run: 0, dump: 'front-objects-1.sop1', class: 'T', detail: 'oracle-p8 differs from oracle-p1: record 2 (CONF)' },
    ])
    rmSync(join(work, 'oracle-p8'), { recursive: true, force: true })
    expect(classifyDumps([pass('oracle-p1', frontObjects('1')), pass('oracle-p8', undefined)], stores)[0]).toMatchObject({ class: 'T', detail: 'not written by oracle-p8' })
  })

  test('the goldens keep the dumps with the runs and name their classes; goldens of before have none', () => {
    const p1 = pass('oracle-p1', frontObjects('1'))
    const classes = classifyDumps([p1], stores)
    const goldens = join(work, 'goldens')
    const oracle = { exeSha256: 'e', cli: 'c' }
    const index = keepGoldens({ pass: p1, store: join(work, 'oracle-p1'), goldens, corpusSha256: 's', root: 'r', resources: 'res', work: 'w', oracle, dumps: classes })
    expect(readGoldens(goldens).dumps).toEqual(classes)
    expect(index.cases[0]!.runs[0]!.dumps).toEqual(['front-objects-1.sop1'])
    expect(readFileSync(join(goldens, 'cases', 'cube', '0', 'dumps', 'front-objects-1.sop1')).equals(frontObjects('1'))).toBe(true)
    const before = keepGoldens({ pass: p1, store: join(work, 'oracle-p1'), goldens, corpusSha256: 's', root: 'r', resources: 'res', work: 'w', oracle })
    expect('dumps' in before).toBe(false)
  })
})

describe("a dump's paths", () => {
  const root = 'C:\\w\\parity\\runs\\record\\cases'
  const roots: [string, string][] = [
    [join(root, 'cube'), '<case>'],
    [root, '<cases>'],
  ]

  test('are written as their names inside the strings, each length written again', () => {
    const dump = sop1([
      ['HEAD', head('objects', 1)],
      ['CONF', conf('run', 'post_process', `${root}\\cube\\post.py`)],
    ])
    const p = portableDump(dump, roots)!
    const file = readSop1(p)
    expect(parseHead(file.records[0]!.payload)).toEqual({ source: 'front', name: 'objects', object: -1n, occurrence: 1 })
    const r = new Sop1Reader(file.records[1]!.payload)
    expect([r.str(), r.i64(), r.i64(), r.u64(), r.str(), r.u32(), r.str(), r.u64()]).toEqual(['run', -1n, -1n, 1n, 'post_process', 5, '<case>\\post.py', 0n])
    expect(r.done()).toBe(true)
    // Two folders of other lengths give the same bytes.
    const other = 'D:\\a\\much\\longer\\folder\\parity\\runs\\certify\\cases'
    const dump2 = sop1([
      ['HEAD', head('objects', 1)],
      ['CONF', conf('run', 'post_process', `${other}\\cube\\post.py`)],
    ])
    expect(portableDump(dump2, [[join(other, 'cube'), '<case>'], [other, '<cases>']])!.equals(p)).toBe(true)
  })

  test("the custom G-code's strings too; height ranges, brim ears and painting are read whole and kept", () => {
    const f64 = (v: number) => {
      const b = Buffer.alloc(8)
      b.writeDoubleLE(v)
      return b
    }
    const f32 = (v: number) => {
      const b = Buffer.alloc(4)
      b.writeFloatLE(v)
      return b
    }
    const pcgc = (extra: string) => Buffer.concat([i32(0), u64(1), i32(0), i32(1), u64(1), f64(2.4), i32(4), i32(1), str('#FF0000'), str(extra)])
    const painting = Buffer.concat([u64(1), i32(3), i32(0), u64(10), u64(2), Buffer.from([0xff, 0x03]), u64(2), bool(false), bool(true)])
    const empty = Buffer.concat([u64(0), u64(0), u64(0), u64(0)])
    const rest: [string, Buffer][] = [
      ['HRNG', Buffer.concat([i64(0), u64(1), f64(0), f64(1.5)])],
      ['BRIM', Buffer.concat([i64(0), u64(1), f32(1), f32(2), f32(0), f32(0.5)])],
      ['PNTV', Buffer.concat([i64(0), i64(0), painting, empty, empty, empty])],
    ]
    const dump = sop1([['HEAD', head('objects', 1)], ['PCGC', pcgc(`M117 ${root}\\cube`)], ...rest])
    const file = readSop1(portableDump(dump, roots)!)
    const r = new Sop1Reader(file.records[1]!.payload)
    const z = f64(2.4).readBigUInt64LE()
    expect([r.i32(), r.u64(), r.i32(), r.i32(), r.u64(), r.u64(), r.i32(), r.i32(), r.str(), r.str()]).toEqual([0, 1n, 0, 1, 1n, z, 4, 1, '#FF0000', 'M117 <case>'])
    expect(r.done()).toBe(true)
    expect(file.records.slice(2)).toEqual(rest.map(([kind, payload]) => ({ kind, payload })))
    // A PNTV that does not read as its fields leaves the whole dump as it is.
    const short = sop1([['PCGC', pcgc(`${root}\\cube`)], ['PNTV', Buffer.concat([i64(0), i64(0), painting])]])
    expect(portableDump(short, roots)).toBe(short)
  })

  test('a dump with no path in it, or that is no SOP1 file, is left as it is', () => {
    const dump = frontObjects('1')
    expect(portableDump(dump, roots)).toBe(dump)
    const bad = Buffer.from(`not SOP1 ${root}`)
    expect(portableDump(bad, roots)).toBe(bad)
    expect(portableDump(undefined, roots)).toBeUndefined()
  })

  test('portableDumpBytes reads a kept dump with its side\'s roots', () => {
    const store = join(work, 'store')
    mkdirSync(join(store, 'cube', '0', 'dumps'), { recursive: true })
    writeFileSync(join(store, 'cube', '0', 'dumps', 'front-objects-1.sop1'), sop1([['CONF', conf('run', 'k', `${root}\\cube\\m.stl`)]]))
    const run: RecordedRun = { index: 0, args: [], extraEnv: [], code: 0, timedOut: false, files: [], gone: [], dumps: ['front-objects-1.sop1'] }
    const b = portableDumpBytes(store, 'cube', run, 'front-objects-1.sop1', { root, resources: 'C:\\res', work: 'C:\\w' })!
    expect(b.includes(Buffer.from('<case>\\m.stl'))).toBe(true)
    expect(portableDumpBytes(store, 'cube', run, 'front-objects-2.sop1', { root, resources: 'C:\\res', work: 'C:\\w' })).toBeUndefined()
  })
})
