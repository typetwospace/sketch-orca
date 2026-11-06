// The clock and entropy masks of the parity harness (parity/masks.json) and the comparison of one
// output of two engines: 'identical' (the same bytes), 'masked' (the same once the masks that name
// that file are applied) or 'different'. A mask applies only to the files its `files` globs name:
// an output by its file name ("*.gcode"), an entry of an archive as "<archive>!<entry>"
// ("*.3mf!Metadata/plate_*.gcode"); `*` matches within one name, never across "/" or "!".
//
// Kinds (masks.json says which source line each one is for):
//   text       a regular expression (flags g and m) whose matches are replaced in both files
//   md5-of     an archive entry holding the MD5 of another entry: masked when each archive's MD5 is
//              that of its own entry, which is compared on its own
//   zip-times  the DOS times of an archive's entries, in the local headers and the central directory
// An archive is masked only when its entry names and order, its compression methods and its central
// directory records (times, CRC-32, sizes and offsets aside) are the same, and each entry is
// identical or masked. Text is compared as Latin-1, byte for byte.

import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { basename, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { extractZipEntry, readZipEntries, type ZipEntry } from '../zip.ts'

export const MASKS_FORMAT = 'sketch-orca-parity-masks/1'

/** parity/masks.json in this repository. */
export const MASKS_FILE = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'parity', 'masks.json')

interface MaskBase {
  id: string
  files: string[]
  source: string
  why: string
}
export interface TextMask extends MaskBase {
  kind: 'text'
  pattern: string
  replace: string
}
export interface Md5Mask extends MaskBase {
  kind: 'md5-of'
}
export interface ZipTimesMask extends MaskBase {
  kind: 'zip-times'
}
export type Mask = TextMask | Md5Mask | ZipTimesMask

export type OutputStatus = 'identical' | 'masked' | 'different'

export interface Comparison {
  status: OutputStatus
  /** The masks that made it equal (status 'masked'), by id, sorted. */
  masks: string[]
  /** Where it differs (status 'different'). */
  detail?: string
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

/** Checks parsed masks.json and returns its masks; throws naming the bad field. */
export function parseMasks(value: unknown): Mask[] {
  if (!isRecord(value) || value.format !== MASKS_FORMAT) throw new Error(`masks: format must be "${MASKS_FORMAT}"`)
  if (!Array.isArray(value.masks)) throw new Error('masks: masks must be a list')
  const ids = new Set<string>()
  return value.masks.map((m, i): Mask => {
    if (!isRecord(m)) throw new Error(`masks[${i}]: not an object`)
    const at = typeof m.id === 'string' && /^[a-z0-9][a-z0-9-]*$/.test(m.id) ? m.id : undefined
    if (!at) throw new Error(`masks[${i}]: id must be lower-case letters, digits and -`)
    if (ids.has(at)) throw new Error(`masks: ${at} is there twice`)
    ids.add(at)
    if (!Array.isArray(m.files) || m.files.length === 0 || !m.files.every((f) => typeof f === 'string' && f !== '')) throw new Error(`masks ${at}: files must be a list of globs`)
    for (const k of ['source', 'why']) if (typeof m[k] !== 'string' || (m[k] as string).trim() === '') throw new Error(`masks ${at}: ${k} must say which source line it is for, and why`)
    const base = { id: at, files: m.files as string[], source: m.source as string, why: m.why as string }
    if (m.kind === 'text') {
      if (typeof m.pattern !== 'string' || typeof m.replace !== 'string') throw new Error(`masks ${at}: a text mask needs pattern and replace`)
      try {
        new RegExp(m.pattern, 'gm')
      } catch (err) {
        throw new Error(`masks ${at}: bad pattern: ${(err as Error).message}`)
      }
      if (new RegExp(m.pattern, 'gm').test('')) throw new Error(`masks ${at}: the pattern matches empty text`)
      return { ...base, kind: 'text', pattern: m.pattern, replace: m.replace }
    }
    if (m.kind === 'md5-of') {
      if (!base.files.every((f) => f.includes('!') && f.endsWith('.md5'))) throw new Error(`masks ${at}: an md5-of mask names archive entries ending in .md5`)
      return { ...base, kind: 'md5-of' }
    }
    if (m.kind === 'zip-times') {
      if (base.files.some((f) => f.includes('!'))) throw new Error(`masks ${at}: a zip-times mask names archives, not entries`)
      return { ...base, kind: 'zip-times' }
    }
    throw new Error(`masks ${at}: unknown kind ${JSON.stringify(m.kind)}`)
  })
}

/** The masks of parity/masks.json, and the SHA-256 of the file (same.json names it). */
export function readMasks(path: string = MASKS_FILE): { masks: Mask[]; sha256: string } {
  const bytes = readFileSync(path)
  return { masks: parseMasks(JSON.parse(bytes.toString('utf8'))), sha256: createHash('sha256').update(bytes).digest('hex') }
}

/** A `files` glob as a regular expression: `*` and `?` stay within one name (no "/" or "!"). */
export function globRegExp(glob: string): RegExp {
  let re = ''
  for (const ch of glob) {
    if (ch === '*') re += '[^/!]*'
    else if (ch === '?') re += '[^/!]'
    else re += ch.replace(/[.+^${}()|[\]\\]/g, '\\$&')
  }
  return new RegExp(`^${re}$`)
}

/** The masks whose globs name `key` (a file name, or "<archive name>!<entry>"). */
export function masksFor(masks: Mask[], key: string): Mask[] {
  return masks.filter((m) => m.files.some((g) => globRegExp(g).test(key)))
}

function applyText(text: string, masks: Mask[], used: Set<string>): string {
  let out = text
  for (const m of masks) {
    if (m.kind !== 'text') continue
    const re = new RegExp(m.pattern, 'gm')
    if (re.test(out)) used.add(m.id)
    out = out.replace(new RegExp(m.pattern, 'gm'), m.replace)
  }
  return out
}

/** The first line where two texts differ, both lines cut to 160 characters. */
export function firstDifference(a: string, b: string): string {
  const la = a.split('\n')
  const lb = b.split('\n')
  for (let i = 0; i < Math.max(la.length, lb.length); i++) {
    if (la[i] !== lb[i]) {
      const show = (s: string | undefined) => (s === undefined ? '(no line)' : JSON.stringify(s.replace(/\r$/, '').slice(0, 160)))
      return `line ${i + 1}: ${show(la[i])} against ${show(lb[i])}`
    }
  }
  return 'the same lines'
}

function compareText(key: string, a: Buffer, b: Buffer, masks: Mask[]): Comparison {
  const used = new Set<string>()
  const textMasks = masksFor(masks, key).filter((m) => m.kind === 'text')
  const ta = applyText(a.toString('latin1'), textMasks, used)
  const tb = applyText(b.toString('latin1'), textMasks, used)
  if (ta === tb) return { status: 'masked', masks: [...used].sort() }
  return { status: 'different', masks: [], detail: firstDifference(ta, tb) }
}

const LOCAL = 0x04034b50
const CENTRAL = 0x02014b50
const EOCD = 0x06054b50
const EOCD64 = 0x06064b50
const EOCD64_LOCATOR = 0x07064b50

/** Where each central directory record starts and ends, in archive order. */
export function centralRecords(buf: Buffer): { start: number; end: number; local: number }[] {
  let eocd = -1
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 0xffff); i--) {
    if (buf.readUInt32LE(i) === EOCD) {
      eocd = i
      break
    }
  }
  if (eocd < 0) throw new Error('zip: no end of central directory record')
  let count = buf.readUInt16LE(eocd + 10)
  let offset = buf.readUInt32LE(eocd + 16)
  if (count === 0xffff || offset === 0xffffffff) {
    const loc = eocd - 20
    if (loc < 0 || buf.readUInt32LE(loc) !== EOCD64_LOCATOR) throw new Error('zip: ZIP64 archive without its locator')
    const rec = Number(buf.readBigUInt64LE(loc + 8))
    if (buf.readUInt32LE(rec) !== EOCD64) throw new Error('zip: bad ZIP64 end of central directory record')
    count = Number(buf.readBigUInt64LE(rec + 32))
    offset = Number(buf.readBigUInt64LE(rec + 48))
  }
  const out: { start: number; end: number; local: number }[] = []
  let p = offset
  for (let n = 0; n < count; n++) {
    if (p + 46 > buf.length || buf.readUInt32LE(p) !== CENTRAL) throw new Error(`zip: bad central directory entry ${n}`)
    const end = p + 46 + buf.readUInt16LE(p + 28) + buf.readUInt16LE(p + 30) + buf.readUInt16LE(p + 32)
    out.push({ start: p, end, local: buf.readUInt32LE(p + 42) })
    p = end
  }
  return out
}

/** A copy of an archive with every entry's DOS time and date set to 0, in both its headers. */
export function zeroZipTimes(buf: Buffer): Buffer {
  const out = Buffer.from(buf)
  for (const r of centralRecords(buf)) {
    out.writeUInt32LE(0, r.start + 12)
    if (r.local !== 0xffffffff && r.local + 30 <= buf.length && buf.readUInt32LE(r.local) === LOCAL) out.writeUInt32LE(0, r.local + 10)
  }
  return out
}

/** A central directory record with its time and date, CRC-32, sizes and local header offset set to 0. */
function recordShape(buf: Buffer, r: { start: number; end: number }): Buffer {
  const rec = Buffer.from(buf.subarray(r.start, r.end))
  rec.fill(0, 12, 28)
  rec.fill(0, 42, 46)
  return rec
}

function md5Hex(bytes: Uint8Array): string {
  return createHash('md5').update(bytes).digest('hex').toUpperCase()
}

function compareZip(name: string, a: Buffer, b: Buffer, masks: Mask[]): Comparison {
  const used = new Set<string>()
  const different = (detail: string): Comparison => ({ status: 'different', masks: [], detail })
  let ea: ZipEntry[]
  let eb: ZipEntry[]
  let ra: ReturnType<typeof centralRecords>
  let rb: ReturnType<typeof centralRecords>
  try {
    ea = readZipEntries(a)
    eb = readZipEntries(b)
    ra = centralRecords(a)
    rb = centralRecords(b)
  } catch (err) {
    return different(`not a readable archive: ${(err as Error).message}`)
  }
  const namesA = ea.map((e) => e.name)
  const namesB = eb.map((e) => e.name)
  if (namesA.join('\n') !== namesB.join('\n')) {
    const i = namesA.findIndex((n, k) => n !== namesB[k])
    return different(`entries differ: ${JSON.stringify(namesA[i < 0 ? namesA.length : i] ?? null)} against ${JSON.stringify(namesB[i < 0 ? namesA.length : i] ?? null)} at entry ${i < 0 ? namesA.length + 1 : i + 1}`)
  }
  const content = (buf: Buffer, entries: ZipEntry[], entryName: string) => extractZipEntry(buf, entries.find((e) => e.name === entryName)!)
  for (let i = 0; i < ea.length; i++) {
    const x = ea[i]!
    const y = eb[i]!
    if (x.method !== y.method) return different(`${x.name}: compression method ${x.method} against ${y.method}`)
    if (!recordShape(a, ra[i]!).equals(recordShape(b, rb[i]!))) return different(`${x.name}: its central directory records differ beyond the time, CRC-32, sizes and offset`)
    let ca: Buffer
    let cb: Buffer
    try {
      ca = extractZipEntry(a, x)
      cb = extractZipEntry(b, y)
    } catch (err) {
      return different(`${x.name}: ${(err as Error).message}`)
    }
    if (ca.equals(cb)) continue
    const key = `${name}!${x.name}`
    const md5 = masksFor(masks, key).find((m) => m.kind === 'md5-of')
    if (md5) {
      const of = x.name.replace(/\.md5$/, '')
      const selfCheck = (buf: Buffer, entries: ZipEntry[], own: Buffer) => entries.some((e) => e.name === of) && own.toString('latin1').trim().toUpperCase() === md5Hex(content(buf, entries, of))
      if (selfCheck(a, ea, ca) && selfCheck(b, eb, cb)) {
        used.add(md5.id)
        continue
      }
      return different(`${x.name}: the MD5s differ and are not each that of its own ${of}`)
    }
    const t = compareText(key, ca, cb, masks)
    if (t.status === 'different') return different(`${x.name}: ${t.detail}`)
    for (const id of t.masks) used.add(id)
  }
  const za = zeroZipTimes(a)
  const zb = zeroZipTimes(b)
  if (!za.equals(a) || !zb.equals(b)) {
    const timesDiffer = ra.some((r, i) => a.readUInt32LE(r.start + 12) !== b.readUInt32LE(rb[i]!.start + 12) || a.readUInt32LE(r.local + 10) !== b.readUInt32LE(rb[i]!.local + 10))
    if (timesDiffer) {
      const zipTimes = masksFor(masks, name).find((m) => m.kind === 'zip-times')
      if (!zipTimes) return different('the entries were written at other times, and no mask names this archive')
      used.add(zipTimes.id)
    }
  }
  // Every entry is the same or masked: what is left of the bytes is the compressed data of masked
  // entries and the times. Without a masked entry the rest must be byte for byte the same.
  const maskedEntries = [...used].some((id) => masks.find((m) => m.id === id)?.kind !== 'zip-times')
  if (!maskedEntries && !za.equals(zb)) return different('the archives differ in bytes outside their entries, their times and their central directory records')
  return { status: 'masked', masks: [...used].sort() }
}

function looksLikeZip(name: string, a: Buffer, b: Buffer): boolean {
  return /\.(3mf|zip)$/i.test(name) && a.length >= 4 && b.length >= 4 && a.readUInt32LE(0) === LOCAL && b.readUInt32LE(0) === LOCAL
}

/**
 * Compares one output of two engines. `rel` is the output's path (its file name picks the masks);
 * a missing side (undefined) is 'different', naming which side wrote it.
 */
export function compareOutput(rel: string, a: Buffer | undefined, b: Buffer | undefined, masks: Mask[], labels: [string, string] = ['reference', 'head']): Comparison {
  if (a === undefined && b === undefined) return { status: 'identical', masks: [] }
  if (a === undefined || b === undefined) return { status: 'different', masks: [], detail: `only the ${a === undefined ? labels[1] : labels[0]} wrote it` }
  if (a.equals(b)) return { status: 'identical', masks: [] }
  const name = basename(rel)
  let out: Comparison
  try {
    out = looksLikeZip(name, a, b) ? compareZip(name, a, b, masks) : compareText(name, a, b, masks)
  } catch (err) {
    out = { status: 'different', masks: [], detail: `cannot be compared: ${(err as Error).message}` }
  }
  return out.status === 'masked' && out.masks.length === 0 ? { status: 'different', masks: [], detail: 'the bytes differ, and no mask names what' } : out
}
