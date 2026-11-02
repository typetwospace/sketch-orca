// The binaries the source bundle's archives hold (sketch-orca, 2026-11-01): the packages Orca's
// dependency build downloads ship as their projects publish them, and some carry programs,
// libraries or class files beside their source (OpenCV 4.6.0 a JUnit jar under CPL-1.0, an sbt
// launcher with JLine's Windows DLLs; Draco its WebAssembly builds). source reads every file of
// every archive, and of the archives, jars and gzipped tars inside them, and finds the binaries by
// their bytes, not their names. Each one must be known (PACKAGE_BINARIES): what it is, its licence,
// where its source is, and the licence texts the bundle must add for it; the bundle lists them in
// dependencies/BINARIES.txt. A binary that is not known keeps the bundle from being complete. Each
// binary, also inside a jar, is scanned as the staged engine is (stage.ts privacyProblemsIn): the
// builder's needles never pass, and the folders of another machine it records only when its entry
// names them, as upstream published them.

import { spawn } from 'node:child_process'
import { createReadStream, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { constants, createGunzip, gunzipSync, inflateRawSync } from 'node:zlib'
import type { Platform } from './env.ts'
import { sha256Bytes } from './files.ts'
import { sha256Text } from './paths.ts'
import { readPeDebugPaths } from './pe.ts'
import { findPersonalPaths, isRecordedFolderPath, recordedBuildPaths } from './stage.ts'
import { oneChunk, readTar } from './tar.ts'
import { extractZipEntry, readZipEntries, type ZipEntry } from './zip.ts'

/** Bump when what the inventory finds or records changes: a cached inventory of an older version is made again. */
export const INVENTORY_VERSION = 1

/** How many first bytes say whether a file may be a binary or an archive. */
const HEAD = 64

/** How deep archives inside archives are read. */
const MAX_DEPTH = 6

export type BinaryKind =
  | 'PE program or library'
  | 'MS-DOS program'
  | 'ELF program or library'
  | 'Mach-O program or library'
  | 'Mach-O universal binary'
  | 'Java class'
  | 'object archive'
  | 'COFF object'
  | 'WebAssembly module'
  | 'MSF database (.pdb, .ncb)'
  | 'Dalvik executable'
  | 'compiled resources'
  | 'OLE compound file'
  | 'unreadable archive entry'

const u16 = (b: Uint8Array, at: number) => (at + 2 <= b.length ? b[at]! | (b[at + 1]! << 8) : -1)
const u32le = (b: Uint8Array, at: number) => (at + 4 <= b.length ? (b[at]! | (b[at + 1]! << 8) | (b[at + 2]! << 16) | (b[at + 3]! << 24)) >>> 0 : -1)
const u32be = (b: Uint8Array, at: number) => (at + 4 <= b.length ? ((b[at]! << 24) | (b[at + 1]! << 16) | (b[at + 2]! << 8) | b[at + 3]!) >>> 0 : -1)
const startsWith = (b: Uint8Array, s: string) => b.length >= s.length && Buffer.from(b.subarray(0, s.length)).toString('latin1') === s

const MACHO = [0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe]
const COFF_MACHINES = [0x014c, 0x8664, 0xaa64, 0x01c0, 0x01c4]
const PDB_MAGIC = ['Microsoft C/C++ MSF 7.00\r\n\x1aDS', 'Microsoft C/C++ program database 2.00\r\n']
// A compiled resource file's empty first entry (rc.exe's .res, Visual C++'s .aps), and an OLE
// compound file (Visual C++ 6's .opt, an .msi, an old Office document).
const RES_MAGIC = '\0\0\0\0\x20\0\0\0\xff\xff\0\0\xff\xff\0\0'
const OLE_MAGIC = '\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1'

/** Whether a file starting with `head` may be a binary or an archive to read (binaryKind and archiveKind decide on the whole file). */
export function mayHoldBinaries(head: Uint8Array): boolean {
  if (head.length < 2) return false
  return (
    startsWith(head, 'MZ') ||
    startsWith(head, '\x7fELF') ||
    MACHO.includes(u32be(head, 0)) ||
    u32be(head, 0) === 0xcafebabe ||
    startsWith(head, '!<arch>\n') ||
    startsWith(head, '\0asm') ||
    startsWith(head, 'dex\n') ||
    PDB_MAGIC.some((m) => startsWith(head, m.slice(0, 16))) ||
    COFF_MACHINES.includes(u16(head, 0)) ||
    startsWith(head, RES_MAGIC) ||
    startsWith(head, OLE_MAGIC) ||
    archiveKind(head) !== undefined
  )
}

/** An archive whose files are read in turn: a ZIP (jars too) or a gzip stream. */
export function archiveKind(head: Uint8Array): 'zip' | 'gzip' | undefined {
  if (startsWith(head, 'PK\x03\x04') || startsWith(head, 'PK\x05\x06')) return 'zip'
  if (head[0] === 0x1f && head[1] === 0x8b) return 'gzip'
  return undefined
}

/** What program, library or other compiled file `bytes` is, by its bytes; undefined for anything else. */
export function binaryKind(bytes: Uint8Array): BinaryKind | undefined {
  if (startsWith(bytes, 'MZ') && bytes.length >= 64) {
    const pe = u32le(bytes, 0x3c)
    if (pe > 0 && pe + 4 <= bytes.length && startsWith(bytes.subarray(pe), 'PE\0\0')) return 'PE program or library'
    // An MS-DOS program has no PE header; text that starts with MZ has no NUL in its header.
    return bytes.subarray(0, 64).includes(0) ? 'MS-DOS program' : undefined
  }
  if (startsWith(bytes, '\x7fELF')) return 'ELF program or library'
  const magic = u32be(bytes, 0)
  if (MACHO.includes(magic)) return 'Mach-O program or library'
  // A class file's minor and major version follow its magic (major 45 and up); a universal
  // binary's count of architectures, a handful.
  if (magic === 0xcafebabe) return u32be(bytes, 4) >= 45 ? 'Java class' : 'Mach-O universal binary'
  if (startsWith(bytes, '!<arch>\n')) return 'object archive'
  if (startsWith(bytes, '\0asm') && u32le(bytes, 4) === 1) return 'WebAssembly module'
  if (PDB_MAGIC.some((m) => startsWith(bytes, m))) return 'MSF database (.pdb, .ncb)'
  if (startsWith(bytes, 'dex\n') && bytes.length >= 8 && bytes[7] === 0) return 'Dalvik executable'
  if (startsWith(bytes, RES_MAGIC)) return 'compiled resources'
  if (startsWith(bytes, OLE_MAGIC)) return 'OLE compound file'
  // A COFF object (.obj): a known machine, a few sections, no optional header, a symbol table in the file.
  if (COFF_MACHINES.includes(u16(bytes, 0)) && bytes.length >= 20) {
    const sections = u16(bytes, 2)
    const symbols = u32le(bytes, 8)
    if (sections >= 1 && sections <= 96 && u16(bytes, 16) === 0 && symbols < bytes.length) return 'COFF object'
  }
  return undefined
}

/** A binary in a package: where (`a!/b` for b inside the archive a), its SHA-256, and the folders and needles it records. */
export interface PackageBinary {
  path: string
  sha256: string
  size: number
  kind: BinaryKind
  /** Program databases it records with their folder (readPeDebugPaths). */
  databases: string[]
  /** Other folders of a machine it records (recordedBuildPaths), but those of its program databases. */
  buildPaths: string[]
  /** The builder's needles it holds (findPersonalPaths): never allowed. */
  personal: string[]
}

/**
 * A file of a package that is a binary or holds some: the file itself (its `binaries` then hold it
 * alone, at its own path) or an archive inside the package, a jar say, with the binaries in it.
 */
export interface PackageUnit {
  path: string
  sha256: string
  size: number
  binaries: PackageBinary[]
}

function scanBinary(path: string, bytes: Buffer, kind: BinaryKind, needles: string[]): PackageBinary {
  let databases: string[] = []
  if (kind === 'PE program or library') {
    try {
      databases = readPeDebugPaths(bytes).filter(isRecordedFolderPath)
    } catch {
      // A PE file whose debug directory cannot be read records none we can name.
    }
  }
  const buildPaths = kind === 'unreadable archive entry' ? [] : recordedBuildPaths(bytes, []).filter((p) => !databases.some((d) => d.startsWith(p)))
  return { path, sha256: sha256Bytes(bytes), size: bytes.length, kind, databases, buildPaths, personal: findPersonalPaths(bytes, needles) }
}

function unreadable(path: string, bytes: Buffer, err: unknown): PackageBinary {
  return { path, sha256: sha256Bytes(bytes), size: bytes.length, kind: 'unreadable archive entry', databases: [], buildPaths: [], personal: [`cannot be read: ${(err as Error).message}`] }
}

/** The first `n` bytes of a ZIP entry, inflating no more than it takes. */
function zipEntryHead(buf: Buffer, e: ZipEntry, n: number): Buffer | undefined {
  const p = e.localHeaderOffset
  if (p + 30 > buf.length || buf.readUInt32LE(p) !== 0x04034b50) return undefined
  const start = p + 30 + buf.readUInt16LE(p + 26) + buf.readUInt16LE(p + 28)
  const raw = buf.subarray(start, start + e.compressedSize)
  if (e.method === 0) return raw.subarray(0, n)
  if (e.method !== 8) return undefined
  try {
    const out = inflateRawSync(raw.subarray(0, Math.min(raw.length, 4096)), { finishFlush: constants.Z_SYNC_FLUSH })
    if (out.length >= Math.min(n, e.size)) return out.subarray(0, n)
  } catch {
    // Read the whole entry instead.
  }
  return undefined
}

/** The binaries of an archive's bytes (`prefix` is its path plus "!/"), at any depth below it. */
async function binariesIn(kind: 'zip' | 'gzip', prefix: string, bytes: Buffer, needles: string[], depth: number): Promise<PackageBinary[]> {
  const out: PackageBinary[] = []
  if (depth > MAX_DEPTH) return [unreadable(prefix.slice(0, -2), bytes, new Error(`archives nested deeper than ${MAX_DEPTH}`))]
  const visit = async (path: string, data: Buffer) => {
    const inner = archiveKind(data)
    if (inner) out.push(...(await binariesIn(inner, `${path}!/`, data, needles, depth + 1)))
    else {
      const k = binaryKind(data)
      if (k) out.push(scanBinary(path, data, k, needles))
    }
  }
  if (kind === 'zip') {
    let entries: ZipEntry[]
    try {
      entries = readZipEntries(bytes)
    } catch (err) {
      return [unreadable(prefix.slice(0, -2), bytes, err)]
    }
    for (const e of entries) {
      if (e.name.endsWith('/')) continue
      const head = zipEntryHead(bytes, e, HEAD)
      if (head !== undefined && !mayHoldBinaries(head)) continue
      let data: Buffer
      try {
        data = extractZipEntry(bytes, e)
      } catch (err) {
        out.push(unreadable(`${prefix}${e.name}`, Buffer.alloc(0), err))
        continue
      }
      await visit(`${prefix}${e.name}`, data)
    }
    return out
  }
  let inner: Buffer
  try {
    inner = gunzipSync(bytes)
  } catch (err) {
    return [unreadable(prefix.slice(0, -2), bytes, err)]
  }
  // A gzipped tar, or one gzipped file.
  if (inner.length >= 512 && inner.toString('latin1', 257, 262) === 'ustar') {
    const files: { path: string; data: Buffer }[] = []
    try {
      await readTar(oneChunk(inner), HEAD, (f) => mayHoldBinaries(f.head), (f, data) => void files.push({ path: f.path, data }))
    } catch (err) {
      return [unreadable(prefix.slice(0, -2), bytes, err)]
    }
    for (const f of files) await visit(`${prefix}${f.path}`, f.data)
  } else await visit(prefix, inner)
  return out
}

/** The unit for one file of a package, if it is a binary or holds some. */
async function unitOf(path: string, data: Buffer, needles: string[]): Promise<PackageUnit | undefined> {
  const archive = archiveKind(data)
  const binaries = archive ? await binariesIn(archive, `${path}!/`, data, needles, 1) : []
  if (!archive) {
    const k = binaryKind(data)
    if (k) binaries.push(scanBinary(path, data, k, needles))
  }
  return binaries.length > 0 ? { path, sha256: sha256Bytes(data), size: data.length, binaries } : undefined
}

/** How a package of `name` is read: a ZIP, or a tar compressed as its name says. */
export function packageFormat(name: string): 'zip' | 'tar' | 'tar.gz' | 'tar.bz2' | 'tar.xz' | undefined {
  const n = name.toLowerCase()
  if (n.endsWith('.zip') || n.endsWith('.jar')) return 'zip'
  if (n.endsWith('.tar.gz') || n.endsWith('.tgz')) return 'tar.gz'
  if (n.endsWith('.tar.bz2') || n.endsWith('.tbz2') || n.endsWith('.tbz')) return 'tar.bz2'
  if (n.endsWith('.tar.xz') || n.endsWith('.txz')) return 'tar.xz'
  if (n.endsWith('.tar')) return 'tar'
  return undefined
}

/**
 * The command that writes `file` (a .tar.bz2 or .tar.xz) as a plain tar to its standard output:
 * Windows' own tar.exe (bsdtar) on Windows, which writes the entries of an archive it reads (@),
 * bzip2 or xz elsewhere.
 */
export function decompressCommand(file: string, format: 'tar.bz2' | 'tar.xz', platform: Platform, env: NodeJS.ProcessEnv = process.env): [string, string[]] {
  if (platform === 'win32') return [join(env.SystemRoot ?? env.SYSTEMROOT ?? 'C:\\Windows', 'System32', 'tar.exe'), ['-c', '-f', '-', '--format', 'pax', `@${file}`]]
  return format === 'tar.bz2' ? ['bzip2', ['-d', '-c', file]] : ['xz', ['-d', '-c', file]]
}

async function* commandOutput(cmd: string, args: string[]): AsyncGenerator<Uint8Array> {
  const child = spawn(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
  let stderr = ''
  child.stderr.on('data', (d: Buffer) => (stderr += d.toString()))
  const exited = new Promise<number | null>((resolve, reject) => {
    child.on('error', reject)
    child.on('close', resolve)
  })
  for await (const chunk of child.stdout) yield chunk as Uint8Array
  const code = await exited
  if (code !== 0) throw new Error(`${cmd} exited with ${code}: ${stderr.trim()}`)
}

/** The units of the package at `file` (every binary in it, at any depth), in the order of the package. */
export async function inventoryPackage(file: string, needles: string[], platform: Platform): Promise<PackageUnit[]> {
  const format = packageFormat(file)
  if (!format) throw new Error(`${file}: not a package format the inventory reads (zip, tar, tar.gz, tar.bz2, tar.xz)`)
  const units: PackageUnit[] = []
  if (format === 'zip') {
    const buf = readFileSync(file)
    const binaries = await binariesIn('zip', '', buf, needles, 1)
    // The archive's own files are its units; a binary inside one of them belongs to that one.
    const byTop = new Map<string, PackageBinary[]>()
    for (const b of binaries) {
      const top = b.path.split('!/')[0]!
      byTop.set(top, [...(byTop.get(top) ?? []), b])
    }
    const entries = new Map(readZipEntries(buf).map((e) => [e.name, e] as const))
    for (const [top, list] of byTop) {
      const e = entries.get(top)
      const data = e ? extractZipEntry(buf, e) : Buffer.alloc(0)
      units.push({ path: top, sha256: sha256Bytes(data), size: data.length, binaries: list })
    }
    return units
  }
  const stream: AsyncIterable<Uint8Array> =
    format === 'tar.gz' ? createReadStream(file).pipe(createGunzip()) : format === 'tar' ? createReadStream(file) : commandOutput(...decompressCommand(file, format, platform))
  await readTar(
    stream,
    HEAD,
    (f) => mayHoldBinaries(f.head),
    async (f, data) => {
      const u = await unitOf(f.path, data, needles)
      if (u) units.push(u)
    },
  )
  return units
}

/**
 * The inventory of the package at `file` (SHA-256 `sha256`), kept in `cacheFile` by its SHA-256,
 * the needles and INVENTORY_VERSION, so a package is read once.
 */
export async function cachedInventory(cacheFile: string, file: string, sha256: string, needles: string[], platform: Platform): Promise<PackageUnit[]> {
  const key = sha256Text(JSON.stringify({ version: INVENTORY_VERSION, sha256, needles: [...needles].sort() }))
  let cache: Record<string, PackageUnit[]> = {}
  if (existsSync(cacheFile)) {
    try {
      cache = JSON.parse(readFileSync(cacheFile, 'utf8')) as Record<string, PackageUnit[]>
    } catch {
      cache = {}
    }
  }
  const hit = cache[key]
  if (Array.isArray(hit)) return hit
  const units = await inventoryPackage(file, needles, platform)
  cache[key] = units
  mkdirSync(dirname(cacheFile), { recursive: true })
  writeFileSync(cacheFile, JSON.stringify(cache) + '\n')
  return units
}

/** A binary of a source package that the bundle accounts for. */
export interface KnownPackageBinary {
  /** The dependency (its folder in dependencies/) and the package file it is in. */
  dependency: string
  file: string
  /** The file in the package, and its SHA-256: a jar, say, with whatever it holds. */
  path: string
  sha256: string
  what: string
  /** Its licence. */
  license: string
  /** Where its licence text is: in the package, in the bundle's LICENSES/, or in dependencies/LICENSES/ (from texts). */
  licenseText: string
  /** Where its source is, or why it has none. */
  source: string
  /** Licence texts the package lacks, from this repository's licenses/, copied to dependencies/LICENSES/. */
  texts?: string[]
  /**
   * The folders of other machines the binaries in it record (program databases and build paths), as
   * upstream published them: folder prefixes, or 'as published' for a file of its authors' that
   * records many (an IDE's or an office program's, which keeps the folders it was used in). The
   * entry is pinned by the file's SHA-256, so this allows nothing in any other file; the builder's
   * own needles are never allowed.
   */
  recorded?: readonly string[] | 'as published'
}

const DRACO_WASM = {
  dependency: 'Draco',
  file: '1.5.7.zip',
  license: 'Apache-2.0',
  licenseText: 'LICENSE at the top of the package',
  source:
    'the C++ source in the same package (src/draco), built with Emscripten as its BUILDING.md says (CMake with DRACO_WASM=ON); https://github.com/google/draco/tree/1.5.7',
} as const

const LIBNOISE_IDE = {
  dependency: 'libnoise',
  file: '1.0.zip',
  license: 'LGPL-2.1-or-later, as libnoise\'s sources state',
  licenseText: 'LICENSES/deps/libnoise/LGPL-2.1.txt in this bundle',
  source: 'none: Visual C++ makes it from the project and the sources in the same package (libnoise.dsp, src/)',
  recorded: 'as published',
} as const

const BOOST_DOCUMENT = {
  dependency: 'Boost',
  file: 'boost-1.84.0.tar.gz',
  license: 'BSL-1.0',
  licenseText: 'LICENSE_1_0.txt at the top of the package',
  recorded: 'as published',
} as const

/**
 * The binaries of the packages Orca's dependency build pins (OrcaSlicer v2.4.2), found by
 * inventoryPackage on 2026-11-01. The engine is built from none of them: Draco's WebAssembly
 * modules are its JavaScript decoder and encoder, OpenCV's jars belong to its Java bindings' tests,
 * its Android build and its Java samples, and libnoise's and Boost's are files of their authors'
 * IDEs and spreadsheets.
 */
export const PACKAGE_BINARIES: readonly KnownPackageBinary[] = [
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/draco_decoder.wasm', sha256: '2516a4e43526d71787bf2f678f951329f7f858f8f15f42d4bc9e370b31a0da3a', what: "Draco's WebAssembly decoder" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/draco_decoder_gltf.wasm', sha256: '712db3449ae2041d6e8a224c395bda6cedb49e51322fae38b7db9beb8b381889', what: "Draco's WebAssembly decoder for glTF" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/draco_encoder.wasm', sha256: 'd2a3ac80c91980d5d321c116454834ff36264825c6d1794cc84e42288f158958', what: "Draco's WebAssembly encoder" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/npm/draco3d/draco_decoder.wasm', sha256: '2516a4e43526d71787bf2f678f951329f7f858f8f15f42d4bc9e370b31a0da3a', what: "Draco's WebAssembly decoder (its npm package draco3d)" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/npm/draco3d/draco_encoder.wasm', sha256: 'd2a3ac80c91980d5d321c116454834ff36264825c6d1794cc84e42288f158958', what: "Draco's WebAssembly encoder (its npm package draco3d)" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/npm/draco3dgltf/draco_decoder_gltf.wasm', sha256: '712db3449ae2041d6e8a224c395bda6cedb49e51322fae38b7db9beb8b381889', what: "Draco's WebAssembly decoder for glTF (its npm package draco3dgltf)" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/npm/draco3dgltf/draco_encoder.wasm', sha256: 'd2a3ac80c91980d5d321c116454834ff36264825c6d1794cc84e42288f158958', what: "Draco's WebAssembly encoder (its npm package draco3dgltf)" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/with_asserts/draco_decoder.wasm', sha256: '28a32ca2f2bb656b9b61012d39b02a505f8090565034e5a3498896cc7ddcbee0', what: "Draco's WebAssembly decoder, built with assertions" },
  { ...DRACO_WASM, path: 'draco-1.5.7/javascript/with_asserts/draco_encoder.wasm', sha256: '91f8cdf5fe8cb3986c6ba2f7f56e1aa669425fe0fe46d2e2fd3b2be9d156188e', what: "Draco's WebAssembly encoder, built with assertions" },
  {
    dependency: 'OpenCV',
    file: '4.6.0.tar.gz',
    path: 'opencv-4.6.0/modules/java/test/pure_test/lib/junit-4.11.jar',
    sha256: '1805dc56ff11b7acae75130c6fe4ffdf40aff6b157f9c856fa8d1ed4b11c3c46',
    what: 'JUnit 4.11, with the Hamcrest classes it carries (org/hamcrest): the test library of OpenCV\'s Java bindings',
    license: 'CPL-1.0 (JUnit); BSD-3-Clause (Hamcrest)',
    licenseText: 'dependencies/LICENSES/CPL-1.0.txt in this bundle (JUnit); LICENSE.txt in the jar (Hamcrest)',
    source:
      'JUnit 4.11: https://github.com/junit-team/junit4/tree/r4.11, and junit-4.11-sources.jar at https://repo1.maven.org/maven2/junit/junit/4.11/; Hamcrest: https://github.com/hamcrest/JavaHamcrest',
    texts: ['CPL-1.0.txt'],
  },
  {
    dependency: 'OpenCV',
    file: '4.6.0.tar.gz',
    path: 'opencv-4.6.0/platforms/android/gradle-wrapper/gradle/wrapper/gradle-wrapper.jar',
    sha256: '2a00d07a6854e1c1cad4377a8cf7ee1487881424cc345643cc4b3ebf9c9025f5',
    what: 'the Gradle wrapper of Gradle 4.6 (its build receipt says 4.6-rc-1), which OpenCV\'s Android build runs',
    license: 'Apache-2.0',
    licenseText: 'LICENSE at the top of the package (OpenCV\'s, the Apache License 2.0)',
    source: 'https://github.com/gradle/gradle/tree/v4.6.0 (subprojects/wrapper)',
  },
  {
    dependency: 'OpenCV',
    file: '4.6.0.tar.gz',
    path: 'opencv-4.6.0/samples/java/sbt/sbt/sbt-launch.jar',
    sha256: 'd0aebce159ba65a17e9f6a15f03477afc41bdc9adaa7486f42af306b737884a5',
    what: 'the sbt 0.12.1 launcher, which OpenCV\'s Java samples build with, with the Scala library classes, Apache Ivy 2.3.0-rc1 and JLine (and its Windows DLLs, jline/jline32.dll and jline/jline64.dll) it carries',
    license: 'BSD-style (sbt, the Scala library, JLine) and Apache-2.0 (Apache Ivy), as its NOTICE says',
    licenseText: 'LICENSE, LICENSE_Scala, LICENSE_JLine, LICENSE_Apache and NOTICE in the jar',
    source:
      'sbt: https://github.com/sbt/sbt/tree/v0.12.1; the Scala library: https://github.com/scala/scala; Apache Ivy 2.3.0-rc1: https://archive.apache.org/dist/ant/ivy/2.3.0-rc1/; JLine and its native code: https://github.com/jline/jline1',
    // The program database of JLine's 64-bit DLL, as sbt published the launcher.
    recorded: ['c:\\pws_HEAD\\external\\src\\native\\jline\\x64\\Release\\jline.pdb'],
  },
  { ...LIBNOISE_IDE, path: 'Orca-deps-libnoise-1.0/libnoise.ncb', sha256: 'ba460b37c0e4423116b1b58b192766971cfd98760f5b05395a01eaae8d4be89d', what: 'the Visual C++ 6 IntelliSense database of libnoise\'s project' },
  { ...LIBNOISE_IDE, path: 'Orca-deps-libnoise-1.0/libnoise.opt', sha256: 'b3f938a721b197f82ed92f495f668f102faec3302297205e076072bd44b36696', what: 'the Visual C++ 6 workspace options of libnoise\'s project' },
  { ...LIBNOISE_IDE, path: 'Orca-deps-libnoise-1.0/noise.aps', sha256: '91abdad15de0ac71f881e8a9cc36dbabc88e8c6452b41970da8081d34da9e272', what: 'Visual C++\'s compiled copy of libnoise\'s resources (noise.rc)', source: 'noise.rc, beside it in the package' },
  { ...LIBNOISE_IDE, path: 'Orca-deps-libnoise-1.0/src/win32/noise.aps', sha256: 'c345ee8e51484f71e85dc3b4c044264fef6a7b490a07f3c0ef601364c19d32d4', what: 'Visual C++\'s compiled copy of libnoise\'s resources (noise.rc)', source: 'src/win32/noise.rc, beside it in the package' },
  {
    ...BOOST_DOCUMENT,
    path: 'boost-1.84.0/libs/math/example/dot_net_example/distribution_explorer/distribution_explorer.suo',
    sha256: '2b9c8eef4a5bc14134e850b87af3e853058be4f04cda78ac2466b040e8e9a407',
    what: 'the Visual Studio user options of a Boost.Math example\'s solution',
    source: 'none: Visual Studio keeps it for the solution beside it',
  },
  {
    ...BOOST_DOCUMENT,
    path: 'boost-1.84.0/libs/statechart/example/Performance/Performance.xls',
    sha256: '8350bc69276fab21383ced4e3dde58a6903739c988af3b2fea954d0741e737e7',
    what: 'an Excel workbook of Boost.Statechart\'s performance measurements',
    source: 'none: a document',
  },
  {
    ...BOOST_DOCUMENT,
    path: 'boost-1.84.0/libs/local_function/example/profile.xls',
    sha256: '4bc50551797318068d8c63aae368b2075d509d3a388dc937d5618976f9ed90d1',
    what: 'an Excel workbook of Boost.LocalFunction\'s profiling results',
    source: 'none: a document',
  },
  {
    ...BOOST_DOCUMENT,
    path: 'boost-1.84.0/libs/geometry/test/algorithms/overlay/get_turn_info.xls',
    sha256: '905352443dda17e4d0c6241ee92182f38fd1c7bbbd94611361aa156ff9fc6465',
    what: 'an Excel workbook of Boost.Geometry\'s turn-info test cases',
    source: 'none: a document',
  },
]

/** What the bundle accounts for of one package's units, and what it does not. */
export interface PackageBinaryReport {
  known: { unit: PackageUnit; entry: KnownPackageBinary }[]
  /** Units no entry names (by path and SHA-256): the bundle has no source or licence for them. */
  unknown: PackageUnit[]
  /** The builder's needles in a binary, and entries that cannot be read: never allowed. */
  personal: string[]
  /** Folders of another machine a binary records that its entry does not allow. */
  recorded: string[]
}

function folded(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase()
}

/** The units of the package `file` of `dependency` against `known`. */
export function checkPackageBinaries(dependency: string, file: string, units: PackageUnit[], known: readonly KnownPackageBinary[] = PACKAGE_BINARIES): PackageBinaryReport {
  const report: PackageBinaryReport = { known: [], unknown: [], personal: [], recorded: [] }
  const label = `dependencies/${dependency}/${file}`
  for (const unit of units) {
    const entry = known.find((k) => k.dependency === dependency && k.file === file && k.path === unit.path && k.sha256 === unit.sha256)
    if (entry) report.known.push({ unit, entry })
    else report.unknown.push(unit)
    const recorded = entry?.recorded ?? []
    const allowed = (p: string) => recorded === 'as published' || recorded.some((r) => folded(p).startsWith(folded(r)))
    for (const b of unit.binaries) {
      for (const p of b.personal) report.personal.push(`${label}: ${b.path} (${p})`)
      for (const p of b.databases) if (!allowed(p)) report.recorded.push(`${label}: ${b.path} (program database ${p})`)
      for (const p of b.buildPaths) if (!allowed(p)) report.recorded.push(`${label}: ${b.path} (build path ${p})`)
    }
  }
  return report
}

/** A short description of a unit for a refusal: its path, SHA-256 and what it holds. */
export function describeUnit(u: PackageUnit): string {
  const kinds = [...new Set(u.binaries.map((b) => b.kind))].sort()
  const inside = u.binaries.length === 1 && u.binaries[0]!.path === u.path ? kinds[0] : `${u.binaries.length} binaries: ${kinds.join(', ')}`
  return `${u.path} (SHA-256 ${u.sha256}; ${inside})`
}

/** The licence texts `entries` need from this repository's licenses/ (dependencies/LICENSES/ in the bundle). */
export function binaryLicenseTexts(entries: readonly KnownPackageBinary[]): string[] {
  return [...new Set(entries.flatMap((e) => e.texts ?? []))].sort()
}

/** dependencies/BINARIES.txt: every binary of the bundled packages, its licence and where its source is. */
export function binariesNotice(entries: readonly KnownPackageBinary[]): string {
  const lines = [
    'Binaries in the source packages',
    '',
    "The packages in dependencies/ are the ones Orca's dependency build downloads, as their projects",
    'publish them (each checked against the SHA-256 its recipe pins). Besides their source, these files',
    'in them are binaries or other files made by a program. The engine is built from none of them; each',
    'is listed with its licence, where its licence text is and where its source is.',
    '',
  ]
  if (entries.length === 0) lines.push('None of the packages holds one.', '')
  const key = (e: KnownPackageBinary) => `${e.dependency}/${e.file}/${e.path}`
  for (const e of [...entries].sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0))) {
    const recorded = e.recorded === 'as published' ? ['the folders its authors used, as published'] : (e.recorded ?? []).length > 0 ? [`${(e.recorded as readonly string[]).join(', ')}, as published`] : []
    lines.push(
      `dependencies/${e.dependency}/${e.file}: ${e.path}`,
      `  SHA-256  ${e.sha256}`,
      `  What     ${e.what}`,
      `  Licence  ${e.license}; text: ${e.licenseText}`,
      `  Source   ${e.source}`,
      ...recorded.map((r) => `  Records  ${r}`),
      '',
    )
  }
  return lines.join('\n')
}
