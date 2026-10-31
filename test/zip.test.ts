import { describe, expect, test } from 'bun:test'
import { crc32, extractZipEntry, readZipEntries } from '../scripts/lib/zip.ts'
import { writeZip } from './support/zip-writer.ts'

const GCODE = '; HEADER_BLOCK_START\n; CHANGE_LAYER\n; Z_HEIGHT: 0.2\n'.repeat(200)

describe('zip', () => {
  test('crc32 gives the standard check value', () => {
    expect(crc32(Buffer.from('123456789'))).toBe(0xcbf43926)
    expect(crc32(Buffer.alloc(0))).toBe(0)
  })

  test('reads stored and deflated entries, UTF-8 names and an archive comment', () => {
    const zip = writeZip(
      [
        { name: 'Metadata/plate_1.gcode', data: GCODE },
        { name: 'Metadata/plate_1.gcode.md5', data: 'abc', method: 0 },
        { name: 'dir/grün.txt', data: 'ü' },
        { name: 'empty', data: '', method: 0 },
      ],
      { comment: 'made by a test' },
    )
    const entries = readZipEntries(zip)
    expect(entries.map((e) => [e.name, e.method])).toEqual([
      ['Metadata/plate_1.gcode', 8],
      ['Metadata/plate_1.gcode.md5', 0],
      ['dir/grün.txt', 8],
      ['empty', 0],
    ])
    expect(entries[0]!.compressedSize).toBeLessThan(entries[0]!.size)
    expect(extractZipEntry(zip, entries[0]!).toString('utf8')).toBe(GCODE)
    expect(extractZipEntry(zip, entries[1]!).toString('utf8')).toBe('abc')
    expect(extractZipEntry(zip, entries[2]!).toString('utf8')).toBe('ü')
    expect(extractZipEntry(zip, entries[3]!).length).toBe(0)
  })

  test('reads ZIP64 sizes, offsets and end records', () => {
    const zip = writeZip(
      [
        { name: 'a.txt', data: 'first' },
        { name: 'Metadata/plate_1.gcode', data: GCODE },
      ],
      { zip64: true },
    )
    const entries = readZipEntries(zip)
    expect(entries.map((e) => e.name)).toEqual(['a.txt', 'Metadata/plate_1.gcode'])
    expect(entries[1]!.size).toBe(Buffer.byteLength(GCODE))
    expect(entries[1]!.localHeaderOffset).toBeGreaterThan(0)
    expect(extractZipEntry(zip, entries[0]!).toString('utf8')).toBe('first')
    expect(extractZipEntry(zip, entries[1]!).toString('utf8')).toBe(GCODE)
  })

  test('an empty archive has no entries', () => {
    expect(readZipEntries(writeZip([]))).toEqual([])
  })

  test('refuses what is not a ZIP archive', () => {
    expect(() => readZipEntries(Buffer.from('G1 X0 Y0\n'.repeat(10)))).toThrow(/no end of central directory/)
  })

  test('refuses a corrupted entry, a wrong size and an unknown compression method', () => {
    const zip = writeZip([{ name: 'a.txt', data: 'hello world', method: 0 }])
    const [entry] = readZipEntries(zip)
    const bad = Buffer.from(zip)
    bad[30 + 'a.txt'.length] = 'H'.charCodeAt(0)
    expect(() => extractZipEntry(bad, entry!)).toThrow(/fails its CRC-32 check/)
    expect(() => extractZipEntry(zip, { ...entry!, size: entry!.size + 1 })).toThrow(/the directory says 12/)
    const odd = writeZip([{ name: 'b.bin', data: 'xyz', method: 12 }])
    expect(() => extractZipEntry(odd, readZipEntries(odd)[0]!)).toThrow(/compression method 12/)
    expect(() => extractZipEntry(zip, { ...entry!, localHeaderOffset: 4 })).toThrow(/bad local header/)
  })

  test('refuses a central directory that lies past the end of the file', () => {
    const zip = writeZip([{ name: 'a.txt', data: 'hello' }])
    const eocd = zip.length - 22
    const bad = Buffer.from(zip)
    bad.writeUInt32LE(zip.length, eocd + 16)
    expect(() => readZipEntries(bad)).toThrow(/past the end/)
  })
})
