// Reading a ZIP archive (a Bambu .gcode.3mf is one) with Node's zlib: the central directory,
// ZIP64 sizes and offsets, stored and deflated entries, CRC-32 checked.

import { inflateRawSync } from 'node:zlib'

export interface ZipEntry {
  name: string
  /** 0 stored, 8 deflated. */
  method: number
  crc32: number
  compressedSize: number
  size: number
  localHeaderOffset: number
}

const EOCD = 0x06054b50
const EOCD64 = 0x06064b50
const EOCD64_LOCATOR = 0x07064b50
const CENTRAL = 0x02014b50
const LOCAL = 0x04034b50

let crcTable: Uint32Array | undefined

export function crc32(data: Uint8Array): number {
  if (!crcTable) {
    crcTable = new Uint32Array(256)
    for (let n = 0; n < 256; n++) {
      let c = n
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
      crcTable[n] = c >>> 0
    }
  }
  let c = 0xffffffff
  for (let i = 0; i < data.length; i++) c = crcTable[(c ^ data[i]!) & 0xff]! ^ (c >>> 8)
  return (c ^ 0xffffffff) >>> 0
}

function u64(buf: Buffer, at: number): number {
  const v = buf.readBigUInt64LE(at)
  if (v > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error('zip: a 64-bit size or offset is too large')
  return Number(v)
}

/** The entries of the central directory, in archive order. */
export function readZipEntries(buf: Buffer): ZipEntry[] {
  let eocd = -1
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 0xffff); i--) {
    if (buf.readUInt32LE(i) === EOCD) {
      eocd = i
      break
    }
  }
  if (eocd < 0) throw new Error('zip: no end of central directory record')
  let count = buf.readUInt16LE(eocd + 10)
  let cdSize = buf.readUInt32LE(eocd + 12)
  let cdOffset = buf.readUInt32LE(eocd + 16)
  if (count === 0xffff || cdSize === 0xffffffff || cdOffset === 0xffffffff) {
    const loc = eocd - 20
    if (loc < 0 || buf.readUInt32LE(loc) !== EOCD64_LOCATOR) throw new Error('zip: ZIP64 archive without its locator')
    const rec = u64(buf, loc + 8)
    if (buf.readUInt32LE(rec) !== EOCD64) throw new Error('zip: bad ZIP64 end of central directory record')
    count = u64(buf, rec + 32)
    cdSize = u64(buf, rec + 40)
    cdOffset = u64(buf, rec + 48)
  }
  if (cdOffset + cdSize > buf.length) throw new Error('zip: the central directory lies past the end of the file')
  const entries: ZipEntry[] = []
  let p = cdOffset
  for (let n = 0; n < count; n++) {
    if (buf.readUInt32LE(p) !== CENTRAL) throw new Error(`zip: bad central directory entry ${n}`)
    const method = buf.readUInt16LE(p + 10)
    const crc = buf.readUInt32LE(p + 16)
    let compressedSize = buf.readUInt32LE(p + 20)
    let size = buf.readUInt32LE(p + 24)
    const nameLen = buf.readUInt16LE(p + 28)
    const extraLen = buf.readUInt16LE(p + 30)
    const commentLen = buf.readUInt16LE(p + 32)
    let localHeaderOffset = buf.readUInt32LE(p + 42)
    const name = buf.toString('utf8', p + 46, p + 46 + nameLen)
    // ZIP64 extra field: the 64-bit values, in this order, for the fields that are 0xffffffff.
    let x = p + 46 + nameLen
    const xEnd = x + extraLen
    while (x + 4 <= xEnd) {
      const id = buf.readUInt16LE(x)
      const len = buf.readUInt16LE(x + 2)
      if (id === 0x0001) {
        let q = x + 4
        if (size === 0xffffffff) {
          size = u64(buf, q)
          q += 8
        }
        if (compressedSize === 0xffffffff) {
          compressedSize = u64(buf, q)
          q += 8
        }
        if (localHeaderOffset === 0xffffffff) localHeaderOffset = u64(buf, q)
      }
      x += 4 + len
    }
    entries.push({ name, method, crc32: crc, compressedSize, size, localHeaderOffset })
    p = xEnd + commentLen
  }
  return entries
}

/** The bytes of one entry, inflated and checked against its CRC-32 and size. */
export function extractZipEntry(buf: Buffer, entry: ZipEntry): Buffer {
  const p = entry.localHeaderOffset
  if (buf.readUInt32LE(p) !== LOCAL) throw new Error(`zip: bad local header for ${entry.name}`)
  const start = p + 30 + buf.readUInt16LE(p + 26) + buf.readUInt16LE(p + 28)
  const raw = buf.subarray(start, start + entry.compressedSize)
  let data: Buffer
  if (entry.method === 0) data = Buffer.from(raw)
  else if (entry.method === 8) data = inflateRawSync(raw)
  else throw new Error(`zip: ${entry.name} uses compression method ${entry.method}`)
  if (data.length !== entry.size) throw new Error(`zip: ${entry.name} is ${data.length} bytes, the directory says ${entry.size}`)
  if (crc32(data) !== entry.crc32) throw new Error(`zip: ${entry.name} fails its CRC-32 check`)
  return data
}
