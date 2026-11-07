// parity goldens --check and the dumps (scripts/lib/parity/goldens.ts): the second folder's pass
// keeps the oracle's dumps as parity record does, and each kept dump is compared with the goldens',
// each folder's paths made portable inside the dump's strings; one that still differs, or that one
// folder alone wrote, is an H-site named dumps/<file>. Hermetic: temporary folders and hand-made
// SOP1 files only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { folderDifferences, withHSites } from '../scripts/lib/parity/goldens.ts'
import type { RecordedRun } from '../scripts/lib/parity/same.ts'

let tmp = ''
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), 'orca-parity-goldens-'))
})
afterEach(() => rmSync(tmp, { recursive: true, force: true }))

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
const str = (s: string) => Buffer.concat([u32(Buffer.byteLength(s)), Buffer.from(s, 'utf8')])

/** A front-objects dump whose one CONF value is `value`. */
function dump(value: string): Buffer {
  const records: [string, Buffer][] = [
    ['HEAD', Buffer.concat([str('front'), str('objects'), i64(-1), u32(1)])],
    ['CONF', Buffer.concat([str('run'), i64(-1), i64(-1), u64(1), str('post_process'), u32(5), str(value), u64(0)])],
  ]
  const parts: Buffer[] = [Buffer.from('SOP1', 'latin1'), u32(1)]
  for (const [kind, payload] of [...records, ['END ', u64(records.length)] as [string, Buffer]]) parts.push(Buffer.from(kind, 'latin1'), u64(payload.length), payload)
  return Buffer.concat(parts)
}

/** Run 0 of case c1 in `store`, with the dumps `dumps` (file name to bytes). */
function store(base: string, dumps: Record<string, Buffer>): RecordedRun {
  const d = join(base, 'c1', '0')
  mkdirSync(join(d, 'files'), { recursive: true })
  writeFileSync(join(d, 'stdout'), '')
  writeFileSync(join(d, 'stderr'), '')
  if (Object.keys(dumps).length > 0) mkdirSync(join(d, 'dumps'), { recursive: true })
  for (const [k, v] of Object.entries(dumps)) writeFileSync(join(d, 'dumps', k), v)
  return { index: 0, args: [], extraEnv: [], code: 0, timedOut: false, files: [], gone: [], dumps: Object.keys(dumps).sort() }
}

describe('goldens --check and the dumps', () => {
  const ra = () => join(tmp, 'wa', 'parity', 'runs', 'record', 'cases')
  const rb = () => join(tmp, 'a-longer-second-folder', 'parity', 'runs', 'record', 'cases')
  const side = (s: string, root: string, w: string, run: RecordedRun) => ({ store: s, cases: [{ id: 'c1', runs: [run] }], root, resources: join(w, 'res'), work: w })

  test("a dump whose only difference is each folder's paths is no H-site", () => {
    const a = store(join(tmp, 'a'), { 'front-objects-1.sop1': dump(join(ra(), 'c1', 'post.py')) })
    const b = store(join(tmp, 'b'), { 'front-objects-1.sop1': dump(join(rb(), 'c1', 'post.py')) })
    const d = folderDifferences({ golden: side(join(tmp, 'a'), ra(), join(tmp, 'wa'), a), check: side(join(tmp, 'b'), rb(), join(tmp, 'a-longer-second-folder'), b) })
    // exit, stdout, stderr and the one dump.
    expect(d.compared).toBe(4)
    expect(d.sites).toEqual([])
  })

  test('a dump that differs otherwise, or that one folder alone wrote, is an H-site named dumps/<file>', () => {
    const a = store(join(tmp, 'a'), { 'front-objects-1.sop1': dump('x'), 'front-objects-2.sop1': dump('y') })
    const b = store(join(tmp, 'b'), { 'front-objects-1.sop1': dump('z') })
    const d = folderDifferences({ golden: side(join(tmp, 'a'), ra(), join(tmp, 'wa'), a), check: side(join(tmp, 'b'), rb(), join(tmp, 'wb'), b) })
    expect(d.sites).toEqual([
      { case: 'c1', run: 0, output: 'dumps/front-objects-1.sop1', detail: 'differs between the folders' },
      { case: 'c1', run: 0, output: 'dumps/front-objects-2.sop1', detail: 'written in the first folder only' },
    ])
    // An H-site of a dump is added to record.json's outputs as H.
    expect(withHSites([], d.sites).map((o) => [o.output, o.class])).toEqual([
      ['dumps/front-objects-1.sop1', 'H'],
      ['dumps/front-objects-2.sop1', 'H'],
    ])
  })

  test('goldens recorded before dumps were kept compare no dumps', () => {
    const a = store(join(tmp, 'a'), {})
    delete a.dumps
    const b = store(join(tmp, 'b'), { 'front-objects-1.sop1': dump('x') })
    const d = folderDifferences({ golden: side(join(tmp, 'a'), ra(), join(tmp, 'wa'), a), check: side(join(tmp, 'b'), rb(), join(tmp, 'wb'), b) })
    expect(d.sites).toEqual([{ case: 'c1', run: 0, output: 'dumps/front-objects-1.sop1', detail: 'written in the second folder only' }])
  })
})
