// A small tar writer for the tests: ustar headers (with the prefix field), pax extended headers,
// GNU long names, base-256 sizes, and entries that are not regular files.

export interface TarInput {
  name: string
  data?: Buffer | string
  /** The type flag: '0' a regular file (default), '5' a folder, '2' a symbolic link, and so on. */
  type?: string
  /** The ustar prefix field (the name's folder when the name is too long). */
  prefix?: string
  /** pax records for an extended header before the entry (path, size, ...). */
  pax?: Record<string, string>
  /** A GNU long name header (././@LongLink, type L) before the entry. */
  gnuLongName?: string
  /** A GNU long link header (type K) before the entry. */
  gnuLongLink?: string
  /** The size written in base-256 instead of octal. */
  base256?: boolean
  /** The size the header says, when it is not the data's. */
  size?: number
  /** The magic and version fields (default ustar, 00). */
  magic?: string
}

function field(h: Buffer, at: number, len: number, value: string): void {
  Buffer.from(value, 'utf8').copy(h, at, 0, len)
}

function octal(n: number, len: number): string {
  return n.toString(8).padStart(len - 1, '0') + '\0'
}

export function tarHeader(o: { name: string; size: number; type: string; prefix?: string; base256?: boolean; magic?: string }): Buffer {
  const h = Buffer.alloc(512)
  field(h, 0, 100, o.name)
  field(h, 100, 8, octal(0o644, 8))
  field(h, 108, 8, octal(0, 8))
  field(h, 116, 8, octal(0, 8))
  if (o.base256) {
    h[124] = 0x80
    let v = o.size
    for (let i = 135; i > 124; i--) {
      h[i] = v % 256
      v = Math.floor(v / 256)
    }
  } else field(h, 124, 12, octal(o.size, 12))
  field(h, 136, 12, octal(0, 12))
  h[156] = o.type.charCodeAt(0)
  Buffer.from(o.magic ?? 'ustar\u000000', 'latin1').copy(h, 257)
  if (o.prefix) field(h, 345, 155, o.prefix)
  let sum = 0
  for (let i = 0; i < 512; i++) sum += i >= 148 && i < 156 ? 0x20 : h[i]!
  field(h, 148, 8, sum.toString(8).padStart(6, '0') + '\0 ')
  return h
}

function padded(data: Buffer): Buffer {
  const pad = (512 - (data.length % 512)) % 512
  return Buffer.concat([data, Buffer.alloc(pad)])
}

/** pax records as an extended header's data. */
export function paxData(records: Record<string, string>): Buffer {
  const parts = Object.entries(records).map(([k, v]) => {
    const body = ` ${k}=${v}\n`
    let len = Buffer.byteLength(body) + 1
    while (String(len).length + Buffer.byteLength(body) !== len) len = String(len).length + Buffer.byteLength(body)
    return Buffer.from(`${len}${body}`, 'utf8')
  })
  return Buffer.concat(parts)
}

export function writeTar(entries: TarInput[], opts: { end?: boolean } = {}): Buffer {
  const out: Buffer[] = []
  for (const e of entries) {
    const data = typeof e.data === 'string' ? Buffer.from(e.data, 'utf8') : (e.data ?? Buffer.alloc(0))
    if (e.pax) {
      const pax = paxData(e.pax)
      out.push(tarHeader({ name: 'PaxHeader/x', size: pax.length, type: 'x' }), padded(pax))
    }
    for (const [type, value] of [['K', e.gnuLongLink], ['L', e.gnuLongName]] as const) {
      if (value === undefined) continue
      const long = Buffer.from(`${value}\0`, 'utf8')
      out.push(tarHeader({ name: '././@LongLink', size: long.length, type, magic: 'ustar  \0' }), padded(long))
    }
    out.push(tarHeader({ name: e.name, size: e.size ?? data.length, type: e.type ?? '0', prefix: e.prefix, base256: e.base256, magic: e.magic }), padded(data))
  }
  if (opts.end !== false) out.push(Buffer.alloc(1024))
  return Buffer.concat(out)
}
