// privacy-fixtures: /Users/Builder
import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { gzipSync } from 'node:zlib'
import { sha256Bytes } from '../scripts/lib/files.ts'
import {
  archiveKind,
  binariesNotice,
  binaryKind,
  binaryLicenseTexts,
  cachedInventory,
  checkPackageBinaries,
  decompressCommand,
  describeUnit,
  inventoryPackage,
  mayHoldBinaries,
  PACKAGE_BINARIES,
  packageFormat,
  type KnownPackageBinary,
  type PackageUnit,
} from '../scripts/lib/packagebins.ts'
import { makePe } from './support/pe-writer.ts'
import { writeTar } from './support/tar-writer.ts'
import { writeZip } from './support/zip-writer.ts'

const bytes = (...parts: (string | number[] | Buffer)[]) =>
  Buffer.concat(parts.map((p) => (typeof p === 'string' ? Buffer.from(p, 'latin1') : Buffer.isBuffer(p) ? p : Buffer.from(p))))
const pad = (b: Buffer, n = 128) => Buffer.concat([b, Buffer.alloc(Math.max(0, n - b.length))])

const SAMPLES = {
  pe: makePe({ is64: true, imports: [] }),
  dos: pad(bytes('MZ', [0x90, 0, 3, 0, 0, 0, 4, 0])),
  elf: pad(bytes([0x7f], 'ELF', [2, 1, 1, 0])),
  macho: pad(bytes([0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1])),
  universal: pad(bytes([0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2])),
  javaClass: pad(bytes([0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52])),
  ar: pad(bytes('!<arch>\n/               0           0     0     0       4         `\n')),
  wasm: pad(bytes('\0asm', [1, 0, 0, 0])),
  pdb: pad(bytes('Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0')),
  ncb: pad(bytes('Microsoft C/C++ program database 2.00\r\n\x1aJG\0\0')),
  dex: pad(bytes('dex\n035\0')),
  res: pad(bytes('\0\0\0\0\x20\0\0\0\xff\xff\0\0\xff\xff\0\0')),
  ole: pad(bytes([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])),
  coff: (() => {
    const b = Buffer.alloc(200)
    b.writeUInt16LE(0x8664, 0)
    b.writeUInt16LE(3, 2)
    b.writeUInt32LE(120, 8)
    b.writeUInt32LE(4, 12)
    return b
  })(),
}

/** A PE image followed by `text` (an assertion's file path, say). */
const peWith = (text: string, debug?: string) =>
  Buffer.concat([makePe({ is64: true, imports: [], ...(debug ? { debug: [{ sig: 'RSDS' as const, path: debug }] } : {}) }), Buffer.from(`\0${text}\0`, 'latin1')])

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'packagebins-'))
})
afterEach(() => {
  rmSync(dir, { recursive: true, force: true })
})
const put = (rel: string, data: Buffer | string) => {
  const p = join(dir, rel)
  mkdirSync(dirname(p), { recursive: true })
  writeFileSync(p, data)
  return p
}

describe('what is a binary', () => {
  test('binaryKind: each kind by its bytes, whatever the name', () => {
    expect(Object.fromEntries(Object.entries(SAMPLES).map(([k, b]) => [k, binaryKind(b)]))).toEqual({
      pe: 'PE program or library',
      dos: 'MS-DOS program',
      elf: 'ELF program or library',
      macho: 'Mach-O program or library',
      universal: 'Mach-O universal binary',
      javaClass: 'Java class',
      ar: 'object archive',
      wasm: 'WebAssembly module',
      pdb: 'MSF database (.pdb, .ncb)',
      ncb: 'MSF database (.pdb, .ncb)',
      dex: 'Dalvik executable',
      res: 'compiled resources',
      ole: 'OLE compound file',
      coff: 'COFF object',
    })
    for (const b of Object.values(SAMPLES)) expect(mayHoldBinaries(b.subarray(0, 64))).toBe(true)
  })

  test('binaryKind: text and data are not binaries, also when they start like one', () => {
    for (const s of [
      'MZ is how this README starts, and it goes on for more than sixty-four characters of text.',
      '#include <stdio.h>\nint main() { return 0; }\n',
      '{"name": "draco3d", "version": "1.5.7"}',
      '\0asm without a version',
      'dex\nnot a Dalvik file',
      '',
    ]) {
      expect(binaryKind(Buffer.from(s, 'latin1'))).toBeUndefined()
    }
    // A short MZ, a PE offset past the end, a COFF header with an optional header (an image) or no sections.
    expect(binaryKind(Buffer.from('MZ\0\0'))).toBeUndefined()
    const coffImage = Buffer.from(SAMPLES.coff)
    coffImage.writeUInt16LE(0xf0, 16)
    expect(binaryKind(coffImage)).toBeUndefined()
    const coffEmpty = Buffer.from(SAMPLES.coff)
    coffEmpty.writeUInt16LE(0, 2)
    expect(binaryKind(coffEmpty)).toBeUndefined()
    expect(mayHoldBinaries(Buffer.from('plain text'))).toBe(false)
    expect(mayHoldBinaries(Buffer.from('M'))).toBe(false)
  })

  test('archiveKind: ZIP (jars too) and gzip', () => {
    expect(archiveKind(writeZip([{ name: 'a', data: 'x' }]))).toBe('zip')
    expect(archiveKind(writeZip([]))).toBe('zip')
    expect(archiveKind(gzipSync(Buffer.from('x')))).toBe('gzip')
    expect(archiveKind(Buffer.from('PK but not a zip'))).toBeUndefined()
  })

  test('packageFormat and decompressCommand', () => {
    expect(['a.zip', 'b.JAR', 'c.tar.gz', 'd.tgz', 'e.tar.bz2', 'f.tbz2', 'g.tar.xz', 'h.txz', 'i.tar', 'j.7z'].map(packageFormat)).toEqual([
      'zip',
      'zip',
      'tar.gz',
      'tar.gz',
      'tar.bz2',
      'tar.bz2',
      'tar.xz',
      'tar.xz',
      'tar',
      undefined,
    ])
    expect(decompressCommand('C:\\dl\\x.tar.bz2', 'tar.bz2', 'win32', { SystemRoot: 'C:\\Windows' })).toEqual([
      join('C:\\Windows', 'System32', 'tar.exe'),
      ['-c', '-f', '-', '--format', 'pax', '@C:\\dl\\x.tar.bz2'],
    ])
    expect(decompressCommand('/dl/x.tar.xz', 'tar.xz', 'linux', {})).toEqual(['xz', ['-d', '-c', '/dl/x.tar.xz']])
    expect(decompressCommand('/dl/x.tar.bz2', 'tar.bz2', 'darwin', {})).toEqual(['bzip2', ['-d', '-c', '/dl/x.tar.bz2']])
  })
})

/** A package's files: text, a program with its program database's folder, a jar holding classes, a DLL and a zip, a gzipped tar, a gzipped file. */
function packageFiles(): { name: string; data: Buffer }[] {
  const innerZip = writeZip([{ name: 'm.wasm', data: SAMPLES.wasm }])
  const jar = writeZip([
    { name: 'META-INF/MANIFEST.MF', data: 'Manifest-Version: 1.0\n' },
    { name: 'org/x/', data: '', method: 0 },
    { name: 'org/x/A.class', data: SAMPLES.javaClass },
    { name: 'native/x64/n.dll', data: peWith('D:\\agent\\_work\\1\\s\\native\\n.cpp') },
    { name: 'inner.zip', data: innerZip, method: 0 },
  ])
  const tgz = gzipSync(writeTar([{ name: 'tools/', type: '5' }, { name: 'tools/run', data: SAMPLES.elf }, { name: 'tools/README', data: 'readme' }]))
  return [
    { name: 'pkg/README.md', data: Buffer.from('# A package\n') },
    { name: 'pkg/bin/tool.exe', data: peWith('version 1.0', 'c:\\ci\\tool\\Release\\tool.pdb') },
    { name: 'pkg/lib/runner.jar', data: jar },
    { name: 'pkg/docs/tools.tar.gz', data: tgz },
    { name: 'pkg/data.gz', data: gzipSync(SAMPLES.macho) },
    { name: 'pkg/notes.txt', data: Buffer.from('MZ starts this note, which is text from here to the end of it, all of it.\n') },
  ]
}

function summary(units: PackageUnit[]) {
  return units.map((u) => ({ path: u.path, binaries: u.binaries.map((b) => ({ path: b.path, kind: b.kind, databases: b.databases, buildPaths: b.buildPaths, personal: b.personal })) }))
}

const EXPECTED: ReturnType<typeof summary> = [
  { path: 'pkg/bin/tool.exe', binaries: [{ path: 'pkg/bin/tool.exe', kind: 'PE program or library', databases: ['c:\\ci\\tool\\Release\\tool.pdb'], buildPaths: [], personal: [] }] },
  {
    path: 'pkg/lib/runner.jar',
    binaries: [
      { path: 'pkg/lib/runner.jar!/org/x/A.class', kind: 'Java class', databases: [], buildPaths: [], personal: [] },
      { path: 'pkg/lib/runner.jar!/native/x64/n.dll', kind: 'PE program or library', databases: [], buildPaths: ['D:\\agent\\_work\\1\\s\\native\\n.cpp'], personal: [] },
      { path: 'pkg/lib/runner.jar!/inner.zip!/m.wasm', kind: 'WebAssembly module', databases: [], buildPaths: [], personal: [] },
    ],
  },
  { path: 'pkg/docs/tools.tar.gz', binaries: [{ path: 'pkg/docs/tools.tar.gz!/tools/run', kind: 'ELF program or library', databases: [], buildPaths: [], personal: [] }] },
  { path: 'pkg/data.gz', binaries: [{ path: 'pkg/data.gz!/', kind: 'Mach-O program or library', databases: [], buildPaths: [], personal: [] }] },
]

describe('inventoryPackage', () => {
  test('a ZIP package: every binary at any depth, by its bytes, with the folders it records', async () => {
    const files = packageFiles()
    const zip = put('dl/pkg.zip', writeZip(files))
    const units = await inventoryPackage(zip, [], 'win32')
    expect(summary(units)).toEqual(EXPECTED)
    // A unit is the package's own file, by its SHA-256.
    const jar = files.find((f) => f.name === 'pkg/lib/runner.jar')!
    expect(units[1]!.sha256).toBe(sha256Bytes(jar.data))
    expect(units[1]!.size).toBe(jar.data.length)
  })

  test('a tar package, gzipped, read as a stream: the same units', async () => {
    const tar = writeTar([{ name: 'pkg/', type: '5' }, ...packageFiles()])
    const tgz = put('dl/pkg.tar.gz', gzipSync(tar))
    expect(summary(await inventoryPackage(tgz, [], 'linux'))).toEqual(EXPECTED)
    const plain = put('dl/pkg.tar', tar)
    expect(summary(await inventoryPackage(plain, [], 'linux'))).toEqual(EXPECTED)
  })

  test('a .tar.bz2 and a .tar.xz package, through the decompressor of the platform', async () => {
    const src = join(dir, 'src')
    for (const f of packageFiles()) put(`src/${f.name}`, f.data)
    const tar = process.platform === 'win32' ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar'
    const platform = process.platform === 'win32' ? 'win32' : process.platform === 'darwin' ? 'darwin' : 'linux'
    for (const [flag, name] of [['-j', 'pkg.tar.bz2'], ['-J', 'pkg.tar.xz']] as const) {
      const out = join(dir, name)
      const made = spawnSync(tar, ['-c', flag, '-f', out, '-C', src, 'pkg'], { windowsHide: true })
      if (made.status !== 0) continue // No compressor here for this format.
      const units = await inventoryPackage(out, [], platform)
      expect(summary(units).map((u) => u.path).sort()).toEqual(EXPECTED.map((u) => u.path).sort())
    }
  })

  test("the builder's needles in a binary, at any depth; text files are not read for them", async () => {
    const zip = put(
      'dl/n.zip',
      writeZip([
        { name: 'a/x.dll', data: peWith('C:/Users/Builder/src/x.cpp') },
        { name: 'a/y.jar', data: writeZip([{ name: 'z.so', data: Buffer.concat([SAMPLES.elf, Buffer.from('\0C:\\USERS\\BUILDER\\z.c\0', 'utf16le')]) }]) },
        { name: 'a/notes.txt', data: 'C:/Users/Builder/notes' },
      ]),
    )
    const units = await inventoryPackage(zip, ['c:/users/builder', 'c:\\users\\builder'], 'win32')
    expect(units.map((u) => u.binaries.map((b) => [b.path, b.personal]))).toEqual([[['a/x.dll', ['c:/users/builder']]], [['a/y.jar!/z.so', ['c:\\users\\builder']]]])
  })

  test('an archive in the package that cannot be read, or archives nested too deep, are binaries that cannot be read', async () => {
    let nested: Buffer = writeZip([{ name: 'm.wasm', data: SAMPLES.wasm }])
    // The package's own archive and five inside it: six deep, the most that is read.
    for (let i = 0; i < 4; i++) nested = writeZip([{ name: `level${i}.zip`, data: nested, method: 0 }])
    const deep = writeZip([{ name: 'deeper.zip', data: nested, method: 0 }])
    const zip = put(
      'dl/bad.zip',
      writeZip([
        { name: 'p/bad.jar', data: Buffer.from('PK\x03\x04 and then nothing a ZIP reader can use', 'latin1') },
        { name: 'p/bad.gz', data: Buffer.from([0x1f, 0x8b, 8, 0, 1, 2, 3]) },
        { name: 'p/ok.zip', data: nested, method: 0 },
        { name: 'p/deep.zip', data: deep, method: 0 },
      ]),
    )
    const units = await inventoryPackage(zip, [], 'win32')
    const kinds = units.map((u) => [u.path, u.binaries.map((b) => [b.kind, b.personal.length > 0 ? b.personal[0]!.slice(0, 15) : ''])])
    expect(kinds).toEqual([
      ['p/bad.jar', [['unreadable archive entry', 'cannot be read:']]],
      ['p/bad.gz', [['unreadable archive entry', 'cannot be read:']]],
      ['p/ok.zip', [['WebAssembly module', '']]],
      ['p/deep.zip', [['unreadable archive entry', 'cannot be read:']]],
    ])
    expect(units[3]!.binaries[0]!.personal[0]).toMatch(/archives nested deeper than 6/)
    // Not a package format it reads; a damaged package.
    await expect(inventoryPackage(put('dl/x.7z', 'x'), [], 'win32')).rejects.toThrow(/not a package format/)
    await expect(inventoryPackage(put('dl/broken.tar.gz', gzipSync(Buffer.alloc(512, 0x41))), [], 'win32')).rejects.toThrow(/bad header checksum/)
  })

  test('cachedInventory: a package is read once for its SHA-256 and the needles', async () => {
    const zip = put('dl/pkg.zip', writeZip([{ name: 'a.wasm', data: SAMPLES.wasm }]))
    const cache = join(dir, 'state', 'package-binaries.json')
    const first = await cachedInventory(cache, zip, 'sha-one', [], 'win32')
    expect(first.map((u) => u.path)).toEqual(['a.wasm'])
    expect(existsSync(cache)).toBe(true)
    // The same SHA-256 and needles: the cache answers, the file is not read.
    put('dl/pkg.zip', writeZip([{ name: 'b.wasm', data: SAMPLES.wasm }]))
    expect((await cachedInventory(cache, zip, 'sha-one', [], 'win32')).map((u) => u.path)).toEqual(['a.wasm'])
    // Another SHA-256, other needles, or a damaged cache: read again.
    expect((await cachedInventory(cache, zip, 'sha-two', [], 'win32')).map((u) => u.path)).toEqual(['b.wasm'])
    put('dl/pkg.zip', writeZip([{ name: 'c.wasm', data: SAMPLES.wasm }]))
    expect((await cachedInventory(cache, zip, 'sha-one', ['x'], 'win32')).map((u) => u.path)).toEqual(['c.wasm'])
    writeFileSync(cache, '{ not json')
    expect((await cachedInventory(cache, zip, 'sha-one', [], 'win32')).map((u) => u.path)).toEqual(['c.wasm'])
    expect(Object.keys(JSON.parse(readFileSync(cache, 'utf8')))).toHaveLength(1)
  })
})

describe('known binaries', () => {
  const unit = (path: string, sha256: string, binaries: Partial<PackageUnit['binaries'][number]>[]): PackageUnit => ({
    path,
    sha256,
    size: 1,
    binaries: binaries.map((b) => ({ path, sha256, size: 1, kind: 'PE program or library', databases: [], buildPaths: [], personal: [], ...b })),
  })
  const entry = (o: Partial<KnownPackageBinary>): KnownPackageBinary => ({
    dependency: 'Dep',
    file: 'dep-1.0.tar.gz',
    path: 'dep-1.0/tool.exe',
    sha256: 'a'.repeat(64),
    what: 'a tool',
    license: 'MIT',
    licenseText: 'LICENSE at the top of the package',
    source: 'src/tool, in the same package',
    ...o,
  })

  test('checkPackageBinaries: a unit is known by its dependency, package, path and SHA-256', () => {
    const known = [entry({})]
    const u = unit('dep-1.0/tool.exe', 'a'.repeat(64), [{}])
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [u], known)).toEqual({ known: [{ unit: u, entry: known[0]! }], unknown: [], personal: [], recorded: [] })
    for (const [dependency, file, other] of [
      ['Other', 'dep-1.0.tar.gz', u],
      ['Dep', 'dep-1.1.tar.gz', u],
      ['Dep', 'dep-1.0.tar.gz', unit('dep-1.0/tool.exe', 'b'.repeat(64), [{}])],
      ['Dep', 'dep-1.0.tar.gz', unit('dep-1.0/other.exe', 'a'.repeat(64), [{}])],
    ] as const) {
      expect(checkPackageBinaries(dependency, file, [other], known).unknown).toEqual([other])
    }
  })

  test("checkPackageBinaries: recorded folders pass only as the entry names them; the builder's needles and unreadable entries never", () => {
    const u = unit('dep-1.0/tool.exe', 'a'.repeat(64), [{ databases: ['C:\\ci\\x64\\Release\\tool.pdb'], buildPaths: ['D:\\agent\\_work\\1\\s\\tool.cpp'] }])
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [u], [entry({})]).recorded).toEqual([
      'dependencies/Dep/dep-1.0.tar.gz: dep-1.0/tool.exe (program database C:\\ci\\x64\\Release\\tool.pdb)',
      'dependencies/Dep/dep-1.0.tar.gz: dep-1.0/tool.exe (build path D:\\agent\\_work\\1\\s\\tool.cpp)',
    ])
    // Prefixes, without case and either slash.
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [u], [entry({ recorded: ['c:/CI/x64/', 'd:\\agent\\_work\\1\\s\\'] })]).recorded).toEqual([])
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [u], [entry({ recorded: ['c:/CI/x64/'] })]).recorded).toHaveLength(1)
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [u], [entry({ recorded: 'as published' })]).recorded).toEqual([])
    // An unknown unit allows nothing.
    const stranger = unit('dep-1.0/stranger.exe', 'c'.repeat(64), [{ buildPaths: ['D:\\agent\\_work\\1\\s\\x.cpp'] }])
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [stranger], [entry({ recorded: 'as published' })]).recorded).toHaveLength(1)
    // Needles and unreadable entries stop it whatever the entry says.
    const personal = unit('dep-1.0/tool.exe', 'a'.repeat(64), [{ personal: ['c:/users/builder'] }, { path: 'dep-1.0/tool.exe!/x', kind: 'unreadable archive entry', personal: ['cannot be read: bad'] }])
    expect(checkPackageBinaries('Dep', 'dep-1.0.tar.gz', [personal], [entry({ recorded: 'as published' })]).personal).toEqual([
      'dependencies/Dep/dep-1.0.tar.gz: dep-1.0/tool.exe (c:/users/builder)',
      'dependencies/Dep/dep-1.0.tar.gz: dep-1.0/tool.exe!/x (cannot be read: bad)',
    ])
  })

  test('describeUnit, binaryLicenseTexts and binariesNotice', () => {
    expect(describeUnit(unit('a/t.exe', 'a'.repeat(64), [{}]))).toBe(`a/t.exe (SHA-256 ${'a'.repeat(64)}; PE program or library)`)
    const jar = unit('a/r.jar', 'b'.repeat(64), [{ path: 'a/r.jar!/A.class', kind: 'Java class' }, { path: 'a/r.jar!/B.class', kind: 'Java class' }, { path: 'a/r.jar!/n.dll' }])
    expect(describeUnit(jar)).toBe(`a/r.jar (SHA-256 ${'b'.repeat(64)}; 3 binaries: Java class, PE program or library)`)
    const entries = [
      entry({ dependency: 'Zed', texts: ['CPL-1.0.txt'], recorded: ['c:\\ci\\'] }),
      entry({ texts: ['CPL-1.0.txt', 'A.txt'], recorded: 'as published' }),
      entry({ path: 'dep-1.0/a.exe' }),
    ]
    expect(binaryLicenseTexts(entries)).toEqual(['A.txt', 'CPL-1.0.txt'])
    const notice = binariesNotice(entries)
    const heads = notice.split('\n').filter((l) => l.startsWith('dependencies/'))
    expect(heads).toEqual(['dependencies/Dep/dep-1.0.tar.gz: dep-1.0/a.exe', 'dependencies/Dep/dep-1.0.tar.gz: dep-1.0/tool.exe', 'dependencies/Zed/dep-1.0.tar.gz: dep-1.0/tool.exe'])
    expect(notice).toContain(`  SHA-256  ${'a'.repeat(64)}\n  What     a tool\n  Licence  MIT; text: LICENSE at the top of the package\n  Source   src/tool, in the same package\n`)
    expect(notice).toContain('  Records  the folders its authors used, as published\n')
    expect(notice).toContain('  Records  c:\\ci\\, as published\n')
    expect(binariesNotice([])).toContain('None of the packages holds one.')
  })

  test('PACKAGE_BINARIES: pinned, one entry per file, and every licence text it names is in licenses/', () => {
    const licenses = join(import.meta.dir, '..', 'licenses')
    const seen = new Set<string>()
    for (const e of PACKAGE_BINARIES) {
      expect(e.sha256).toMatch(/^[0-9a-f]{64}$/)
      for (const k of ['dependency', 'file', 'path', 'what', 'license', 'licenseText', 'source'] as const) expect(e[k].trim()).not.toBe('')
      const key = `${e.dependency}/${e.file}/${e.path}`
      expect(seen.has(key)).toBe(false)
      seen.add(key)
      for (const t of e.texts ?? []) expect(existsSync(join(licenses, t))).toBe(true)
      if (Array.isArray(e.recorded)) for (const r of e.recorded) expect(r.length).toBeGreaterThan(3)
    }
    expect(binaryLicenseTexts(PACKAGE_BINARIES)).toEqual(['CPL-1.0.txt'])
    expect(readFileSync(join(licenses, 'CPL-1.0.txt'), 'utf8').split('\n')[0]).toBe('Common Public License Version 1.0')
    // The JUnit jar's licence text goes in with it; the launcher's JLine program database is the one folder it may record.
    const junit = PACKAGE_BINARIES.find((e) => e.path.endsWith('/junit-4.11.jar'))!
    expect(junit.texts).toEqual(['CPL-1.0.txt'])
    expect(PACKAGE_BINARIES.find((e) => e.path.endsWith('/sbt-launch.jar'))!.recorded).toEqual(['c:\\pws_HEAD\\external\\src\\native\\jline\\x64\\Release\\jline.pdb'])
  })
})
