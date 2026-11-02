// Reading a tar stream (ustar, pax and GNU long names) a block at a time, for the inventory of what
// the source packages hold (packagebins.ts): each file's first bytes, and the whole file only when
// the reader asks for it, so a package is read once without holding it in memory.

/** A file of a tar stream: its path, size and first bytes (up to the `head` asked for). */
export interface TarFile {
  path: string
  size: number
  head: Buffer
}

/** Reads `n` bytes at a time from an async stream of chunks. */
class ChunkReader {
  private chunks: Buffer[] = []
  private length = 0
  private ended = false
  private readonly source: AsyncIterator<Uint8Array>
  constructor(source: AsyncIterator<Uint8Array>) {
    this.source = source
  }

  /** Whether `n` bytes are there (reading more as needed); false at the end of the stream. */
  async have(n: number): Promise<boolean> {
    while (this.length < n && !this.ended) {
      const r = await this.source.next()
      if (r.done) this.ended = true
      else if (r.value.length > 0) {
        const b = Buffer.from(r.value.buffer, r.value.byteOffset, r.value.byteLength)
        this.chunks.push(b)
        this.length += b.length
      }
    }
    return this.length >= n
  }

  /** The next `n` bytes, which `have(n)` said are there. */
  take(n: number): Buffer {
    if (n === 0) return Buffer.alloc(0)
    const first = this.chunks[0]!
    if (first.length >= n) {
      const out = first.subarray(0, n)
      if (first.length === n) this.chunks.shift()
      else this.chunks[0] = first.subarray(n)
      this.length -= n
      return out
    }
    const parts: Buffer[] = []
    let left = n
    while (left > 0) {
      const c = this.chunks[0]!
      if (c.length <= left) {
        parts.push(c)
        this.chunks.shift()
        left -= c.length
      } else {
        parts.push(c.subarray(0, left))
        this.chunks[0] = c.subarray(left)
        left = 0
      }
    }
    this.length -= n
    return Buffer.concat(parts, n)
  }

  /** Reads and drops `n` bytes; false when the stream ends first. */
  async skip(n: number): Promise<boolean> {
    let left = n
    while (left > 0) {
      if (this.length === 0 && !(await this.have(1))) return false
      const step = Math.min(left, this.length)
      let drop = step
      while (drop > 0) {
        const c = this.chunks[0]!
        if (c.length <= drop) {
          this.chunks.shift()
          drop -= c.length
        } else {
          this.chunks[0] = c.subarray(drop)
          drop = 0
        }
      }
      this.length -= step
      left -= step
    }
    return true
  }
}

function text(b: Buffer): string {
  const end = b.indexOf(0)
  return b.toString('utf8', 0, end < 0 ? b.length : end)
}

/** A numeric field: octal text, or base-256 when its first byte has the high bit set. */
export function tarNumber(b: Buffer): number {
  if (b.length > 0 && (b[0]! & 0x80) !== 0) {
    let v = b[0]! & 0x7f
    for (let i = 1; i < b.length; i++) v = v * 256 + b[i]!
    return v
  }
  const s = text(b).trim()
  if (s === '') return 0
  if (!/^[0-7]+$/.test(s)) throw new Error(`tar: bad number ${JSON.stringify(s)}`)
  return parseInt(s, 8)
}

/** The records of a pax extended header ("<length> <key>=<value>\n" each). */
export function paxRecords(b: Buffer): Map<string, string> {
  const out = new Map<string, string>()
  let at = 0
  while (at < b.length) {
    const space = b.indexOf(0x20, at)
    if (space < 0) break
    const len = parseInt(b.toString('latin1', at, space), 10)
    if (!(len > 0) || at + len > b.length) throw new Error('tar: bad pax record')
    const rec = b.toString('utf8', space + 1, at + len - 1)
    const eq = rec.indexOf('=')
    if (eq > 0) out.set(rec.slice(0, eq), rec.slice(eq + 1))
    at += len
  }
  return out
}

/** Whether a 512-byte header block's checksum is right (the sum of its bytes with the checksum field as spaces). */
function checksumOk(h: Buffer): boolean {
  let want: number
  try {
    want = tarNumber(h.subarray(148, 156))
  } catch {
    return false
  }
  let sum = 0
  for (let i = 0; i < 512; i++) sum += i >= 148 && i < 156 ? 0x20 : h[i]!
  return sum === want
}

/**
 * The regular files of the tar stream `source`. For each one `visit` gets its path, size and first
 * `head` bytes, and returns whether it wants the whole file (then given to `whole`); what it does
 * not want is skipped. Directories, links and the like are left out. Throws on a stream that is not
 * tar or ends early.
 */
export async function readTar(
  source: AsyncIterable<Uint8Array>,
  head: number,
  visit: (file: TarFile) => boolean,
  whole: (file: TarFile, bytes: Buffer) => void | Promise<void>,
): Promise<void> {
  const r = new ChunkReader(source[Symbol.asyncIterator]())
  let longName: string | undefined
  let pax = new Map<string, string>()
  const global = new Map<string, string>()
  for (;;) {
    if (!(await r.have(512))) return
    const h = r.take(512)
    if (h.every((b) => b === 0)) return
    if (!checksumOk(h)) throw new Error('tar: bad header checksum (not a tar stream, or a damaged one)')
    const type = String.fromCharCode(h[156]!)
    const size = tarNumber(h.subarray(124, 136))
    const padded = Math.ceil(size / 512) * 512
    // Headers that describe the next entry (GNU's K, a long link target, says nothing this needs).
    if (type === 'x' || type === 'g' || type === 'L' || type === 'K') {
      if (!(await r.have(padded))) throw new Error('tar: the stream ends inside a header')
      const data = r.take(padded).subarray(0, size)
      if (type === 'L') longName = text(data)
      else if (type !== 'K') for (const [k, v] of paxRecords(data)) (type === 'g' ? global : pax).set(k, v)
      continue
    }
    const magic = h.toString('latin1', 257, 263)
    const prefix = magic === 'ustar\0' ? text(h.subarray(345, 500)) : ''
    const name = pax.get('path') ?? global.get('path') ?? longName ?? (prefix ? `${prefix}/${text(h.subarray(0, 100))}` : text(h.subarray(0, 100)))
    const realSize = pax.has('size') ? Number(pax.get('size')) : size
    longName = undefined
    pax = new Map()
    const dataBytes = Math.ceil(realSize / 512) * 512
    if (type !== '0' && type !== '\0' && type !== '7') {
      if (!(await r.skip(dataBytes))) throw new Error('tar: the stream ends inside an entry')
      continue
    }
    const n = Math.min(head, realSize)
    if (!(await r.have(n))) throw new Error(`tar: the stream ends inside ${name}`)
    const first = Buffer.from(r.take(n))
    const file: TarFile = { path: name, size: realSize, head: first }
    if (visit(file)) {
      if (!(await r.have(realSize - n))) throw new Error(`tar: the stream ends inside ${name}`)
      const rest = r.take(realSize - n)
      await whole(file, n === 0 ? Buffer.from(rest) : Buffer.concat([first, rest], realSize))
      if (!(await r.skip(dataBytes - realSize))) throw new Error('tar: the stream ends inside an entry')
    } else if (!(await r.skip(dataBytes - n))) throw new Error(`tar: the stream ends inside ${name}`)
  }
}

/** A Buffer as a stream of one chunk. */
export async function* oneChunk(b: Uint8Array): AsyncGenerator<Uint8Array> {
  yield b
}
