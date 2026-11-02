import { describe, expect, test } from 'bun:test'
import { oneChunk, paxRecords, readTar, tarNumber, type TarFile } from '../scripts/lib/tar.ts'
import { paxData, writeTar } from './support/tar-writer.ts'

/** Every regular file of a tar, whole, as path and text; `want` picks the files read whole. */
async function files(source: AsyncIterable<Uint8Array>, want: (f: TarFile) => boolean = () => true, head = 4) {
  const seen: { path: string; size: number; head: string }[] = []
  const whole: { path: string; text: string }[] = []
  await readTar(
    source,
    head,
    (f) => {
      seen.push({ path: f.path, size: f.size, head: f.head.toString('latin1') })
      return want(f)
    },
    (f, bytes) => void whole.push({ path: f.path, text: bytes.toString('utf8') }),
  )
  return { seen, whole }
}

/** A buffer as a stream of chunks of `n` bytes. */
async function* chunks(b: Buffer, n: number): AsyncGenerator<Uint8Array> {
  for (let i = 0; i < b.length; i += n) yield b.subarray(i, i + n)
}

describe('tar', () => {
  test('tarNumber: octal text, padded with NUL or spaces, and base-256', () => {
    expect(tarNumber(Buffer.from('0000644\0'))).toBe(0o644)
    expect(tarNumber(Buffer.from(' 17 \0'))).toBe(15)
    expect(tarNumber(Buffer.from('\0\0\0'))).toBe(0)
    expect(tarNumber(Buffer.from([0x80, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]))).toBe(2 ** 32)
    expect(() => tarNumber(Buffer.from('12a'))).toThrow(/bad number/)
  })

  test('paxRecords: length-prefixed records, UTF-8 values; a record that overruns is refused', () => {
    const data = paxData({ path: 'a/grün/b.txt', size: '12345', 'SCHILY.xattr.x': 'y=z' })
    expect([...paxRecords(data)]).toEqual([
      ['path', 'a/grün/b.txt'],
      ['size', '12345'],
      ['SCHILY.xattr.x', 'y=z'],
    ])
    expect(() => paxRecords(Buffer.from('99 path=x\n'))).toThrow(/bad pax record/)
  })

  test('readTar: regular files with their first bytes; folders and links left out; the whole file only when asked', async () => {
    const tar = writeTar([
      { name: 'pkg/', type: '5' },
      { name: 'pkg/a.txt', data: 'alpha file' },
      { name: 'pkg/link', type: '2' },
      { name: 'pkg/b.bin', data: Buffer.alloc(1300, 0x41) },
      { name: 'pkg/empty', data: '' },
      { name: 'pkg/c.txt', data: 'gamma' },
    ])
    const { seen, whole } = await files(oneChunk(tar), (f) => f.path !== 'pkg/b.bin')
    expect(seen).toEqual([
      { path: 'pkg/a.txt', size: 10, head: 'alph' },
      { path: 'pkg/b.bin', size: 1300, head: 'AAAA' },
      { path: 'pkg/empty', size: 0, head: '' },
      { path: 'pkg/c.txt', size: 5, head: 'gamm' },
    ])
    expect(whole).toEqual([
      { path: 'pkg/a.txt', text: 'alpha file' },
      { path: 'pkg/empty', text: '' },
      { path: 'pkg/c.txt', text: 'gamma' },
    ])
  })

  test('readTar: the same files from a stream in chunks of any size', async () => {
    const tar = writeTar([
      { name: 'x/one', data: 'first file, a little longer than its head' },
      { name: 'x/two', data: Buffer.alloc(2000, 0x42) },
      { name: 'x/three', data: 'third' },
    ])
    const want = await files(oneChunk(tar), () => true, 16)
    for (const n of [1, 7, 511, 512, 513, 4096]) expect(await files(chunks(tar, n), () => true, 16)).toEqual(want)
  })

  test('readTar: long names from the ustar prefix, a pax header (path and size) and a GNU long name', async () => {
    const long = `${'deep/'.repeat(30)}file.txt`
    const tar = writeTar([
      { name: 'b.txt', prefix: 'pkg/with/prefix', data: 'prefixed' },
      { name: 'truncated-name', pax: { path: long }, data: 'from pax' },
      { name: 'big', pax: { size: '3' }, size: 0, data: 'abc' },
      { name: '././@cut', gnuLongName: `gnu/${long}`, gnuLongLink: 'target/of/a/link', magic: 'ustar  \0', data: 'from gnu' },
      // The pax and GNU names apply to one entry only.
      { name: 'after', data: 'plain' },
    ])
    const { whole } = await files(oneChunk(tar))
    expect(whole).toEqual([
      { path: 'pkg/with/prefix/b.txt', text: 'prefixed' },
      { path: long, text: 'from pax' },
      { path: 'big', text: 'abc' },
      { path: `gnu/${long}`, text: 'from gnu' },
      { path: 'after', text: 'plain' },
    ])
  })

  test('readTar: a base-256 size; the end at two empty blocks or at the end of the stream', async () => {
    const tar = writeTar([{ name: 'n', data: 'numbered', base256: true }])
    expect((await files(oneChunk(tar))).whole).toEqual([{ path: 'n', text: 'numbered' }])
    const open = writeTar([{ name: 'n', data: 'no end blocks' }], { end: false })
    expect((await files(oneChunk(open))).whole).toEqual([{ path: 'n', text: 'no end blocks' }])
    expect((await files(oneChunk(Buffer.alloc(0)))).whole).toEqual([])
  })

  test('readTar: refuses a stream that is not tar, a damaged header, and a stream that ends inside an entry or a header', async () => {
    await expect(files(oneChunk(Buffer.alloc(512, 0x41)))).rejects.toThrow(/bad header checksum/)
    const tar = writeTar([{ name: 'a', data: Buffer.alloc(3000, 0x43) }])
    const damaged = Buffer.from(tar)
    damaged[10] = 0x5a
    await expect(files(oneChunk(damaged))).rejects.toThrow(/bad header checksum/)
    const cut = tar.subarray(0, 512 + 1000)
    await expect(files(oneChunk(cut))).rejects.toThrow(/ends inside/)
    await expect(files(oneChunk(cut), () => false)).rejects.toThrow(/ends inside/)
    const paxCut = writeTar([{ name: 'p', pax: { path: 'x'.repeat(600) }, data: 'x' }]).subarray(0, 700)
    await expect(files(oneChunk(paxCut))).rejects.toThrow(/ends inside a header/)
  })
})
