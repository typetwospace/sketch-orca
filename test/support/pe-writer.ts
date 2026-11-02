// A minimal PE writer for the tests.

/**
 * A minimal PE image: one section (.rdata, RVA 0x1000, file offset 0x400) holding the import
 * descriptors (RVA 0x1000), the delay-load descriptors (RVA 0x1100), the DLL names (RVA 0x1200)
 * and the debug directory (RVA 0x1380), whose CodeView records follow the section (file 0x800).
 */
export function makePe(o: { is64: boolean; imports: string[]; delay?: string[]; delayAsVa?: boolean; debug?: { sig: 'RSDS' | 'NB10' | 'MISC'; path: string }[] }): Buffer {
  const debug = o.debug ?? []
  const buf = Buffer.alloc(0x800 + 0x100 * debug.length)
  buf.write('MZ', 0, 'latin1')
  buf.writeUInt32LE(0x80, 0x3c)
  buf.writeUInt32LE(0x00004550, 0x80)
  buf.writeUInt16LE(o.is64 ? 0x8664 : 0x14c, 0x84)
  buf.writeUInt16LE(1, 0x86)
  const optSize = o.is64 ? 240 : 224
  buf.writeUInt16LE(optSize, 0x94)
  const opt = 0x98
  buf.writeUInt16LE(o.is64 ? 0x20b : 0x10b, opt)
  const imageBase = o.is64 ? 0x140000000 : 0x400000
  if (o.is64) buf.writeBigUInt64LE(BigInt(imageBase), opt + 24)
  else buf.writeUInt32LE(imageBase, opt + 28)
  buf.writeUInt32LE(16, opt + (o.is64 ? 108 : 92))
  const dirs = opt + (o.is64 ? 112 : 96)
  const sec = opt + optSize
  buf.write('.rdata', sec, 'latin1')
  buf.writeUInt32LE(0x400, sec + 8)
  buf.writeUInt32LE(0x1000, sec + 12)
  buf.writeUInt32LE(0x400, sec + 16)
  buf.writeUInt32LE(0x400, sec + 20)
  const at = (rva: number) => rva - 0x1000 + 0x400
  let nameRva = 0x1200
  const putName = (s: string): number => {
    const r = nameRva
    buf.write(`${s}\0`, at(r), 'latin1')
    nameRva += s.length + 1
    return r
  }
  if (o.imports.length > 0) {
    buf.writeUInt32LE(0x1000, dirs + 8)
    buf.writeUInt32LE(20 * (o.imports.length + 1), dirs + 12)
    o.imports.forEach((n, i) => {
      const d = at(0x1000 + 20 * i)
      buf.writeUInt32LE(0x1300, d)
      buf.writeUInt32LE(putName(n), d + 12)
      buf.writeUInt32LE(0x1300, d + 16)
    })
  }
  const delay = o.delay ?? []
  if (delay.length > 0) {
    buf.writeUInt32LE(0x1100, dirs + 8 * 13)
    buf.writeUInt32LE(32 * (delay.length + 1), dirs + 8 * 13 + 4)
    delay.forEach((n, i) => {
      const d = at(0x1100 + 32 * i)
      const r = putName(n)
      buf.writeUInt32LE(o.delayAsVa ? 0 : 1, d)
      buf.writeUInt32LE(o.delayAsVa ? imageBase + r : r, d + 4)
    })
  }
  if (debug.length > 0) {
    buf.writeUInt32LE(0x1380, dirs + 8 * 6)
    buf.writeUInt32LE(28 * debug.length, dirs + 8 * 6 + 4)
    debug.forEach((r, i) => {
      const d = at(0x1380 + 28 * i)
      const raw = 0x800 + 0x100 * i
      const head = r.sig === 'RSDS' ? 24 : 16
      const size = head + Buffer.byteLength(r.path, 'utf8') + 1
      buf.write(r.sig === 'MISC' ? 'MISC' : r.sig, raw, 'latin1')
      buf.write(`${r.path}\0`, raw + head, 'utf8')
      buf.writeUInt32LE(r.sig === 'MISC' ? 4 : 2, d + 12)
      buf.writeUInt32LE(size, d + 16)
      buf.writeUInt32LE(raw, d + 24)
    })
  }
  return buf
}
