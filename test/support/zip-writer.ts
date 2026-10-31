// A small ZIP writer for the tests: stored or deflated entries, optionally with ZIP64 records
// (sizes and offsets moved into the extra fields and the ZIP64 end records).

import { deflateRawSync } from 'node:zlib'
import { crc32 } from '../../scripts/lib/zip.ts'

export interface ZipInput {
  name: string
  data: Buffer | string
  /** 0 stored, 8 deflated (default); any other number is written as is, with the data stored. */
  method?: number
}

function zip64Extra(size: number, compressedSize: number, offset?: number): Buffer {
  const n = offset === undefined ? 16 : 24
  const b = Buffer.alloc(4 + n)
  b.writeUInt16LE(0x0001, 0)
  b.writeUInt16LE(n, 2)
  b.writeBigUInt64LE(BigInt(size), 4)
  b.writeBigUInt64LE(BigInt(compressedSize), 12)
  if (offset !== undefined) b.writeBigUInt64LE(BigInt(offset), 20)
  return b
}

export function writeZip(entries: ZipInput[], opts: { zip64?: boolean; comment?: string } = {}): Buffer {
  const z = opts.zip64 === true
  const locals: Buffer[] = []
  const centrals: Buffer[] = []
  let offset = 0
  for (const e of entries) {
    const data = Buffer.isBuffer(e.data) ? e.data : Buffer.from(e.data, 'utf8')
    const method = e.method ?? 8
    const body = method === 8 ? deflateRawSync(data) : data
    const crc = crc32(data)
    const name = Buffer.from(e.name, 'utf8')
    const version = z ? 45 : 20

    const localExtra = z ? zip64Extra(data.length, body.length) : Buffer.alloc(0)
    const lh = Buffer.alloc(30)
    lh.writeUInt32LE(0x04034b50, 0)
    lh.writeUInt16LE(version, 4)
    lh.writeUInt16LE(0, 6)
    lh.writeUInt16LE(method, 8)
    lh.writeUInt16LE(0, 10)
    lh.writeUInt16LE(0x21, 12)
    lh.writeUInt32LE(crc, 14)
    lh.writeUInt32LE(z ? 0xffffffff : body.length, 18)
    lh.writeUInt32LE(z ? 0xffffffff : data.length, 22)
    lh.writeUInt16LE(name.length, 26)
    lh.writeUInt16LE(localExtra.length, 28)
    locals.push(lh, name, localExtra, body)

    const centralExtra = z ? zip64Extra(data.length, body.length, offset) : Buffer.alloc(0)
    const ch = Buffer.alloc(46)
    ch.writeUInt32LE(0x02014b50, 0)
    ch.writeUInt16LE(version, 4)
    ch.writeUInt16LE(version, 6)
    ch.writeUInt16LE(0, 8)
    ch.writeUInt16LE(method, 10)
    ch.writeUInt16LE(0, 12)
    ch.writeUInt16LE(0x21, 14)
    ch.writeUInt32LE(crc, 16)
    ch.writeUInt32LE(z ? 0xffffffff : body.length, 20)
    ch.writeUInt32LE(z ? 0xffffffff : data.length, 24)
    ch.writeUInt16LE(name.length, 28)
    ch.writeUInt16LE(centralExtra.length, 30)
    ch.writeUInt32LE(z ? 0xffffffff : offset, 42)
    centrals.push(ch, name, centralExtra)

    offset += lh.length + name.length + localExtra.length + body.length
  }
  const cd = Buffer.concat(centrals)
  const tail: Buffer[] = []
  if (z) {
    const rec = Buffer.alloc(56)
    rec.writeUInt32LE(0x06064b50, 0)
    rec.writeBigUInt64LE(44n, 4)
    rec.writeUInt16LE(45, 12)
    rec.writeUInt16LE(45, 14)
    rec.writeBigUInt64LE(BigInt(entries.length), 24)
    rec.writeBigUInt64LE(BigInt(entries.length), 32)
    rec.writeBigUInt64LE(BigInt(cd.length), 40)
    rec.writeBigUInt64LE(BigInt(offset), 48)
    const loc = Buffer.alloc(20)
    loc.writeUInt32LE(0x07064b50, 0)
    loc.writeBigUInt64LE(BigInt(offset + cd.length), 8)
    loc.writeUInt32LE(1, 16)
    tail.push(rec, loc)
  }
  const comment = Buffer.from(opts.comment ?? '', 'utf8')
  const eocd = Buffer.alloc(22)
  eocd.writeUInt32LE(0x06054b50, 0)
  eocd.writeUInt16LE(z ? 0xffff : entries.length, 8)
  eocd.writeUInt16LE(z ? 0xffff : entries.length, 10)
  eocd.writeUInt32LE(z ? 0xffffffff : cd.length, 12)
  eocd.writeUInt32LE(z ? 0xffffffff : offset, 16)
  eocd.writeUInt16LE(comment.length, 20)
  return Buffer.concat([...locals, cd, ...tail, eocd, comment])
}
