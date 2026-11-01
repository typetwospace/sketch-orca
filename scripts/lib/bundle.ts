// `source`: the complete corresponding source of the staged engine (AGPL-3.0, section 6), in
// <work>/dist/source/<name>/:
//
//   orca/OrcaSlicer-<tag>.tar.gz    OrcaSlicer at the locked commit, as upstream has it (git archive)
//   engine/                         this repository (sketch-orca): the lock, the patches, the
//                                   command-line front end (cli/), the protocol, the pipeline scripts
//                                   and their tests, the README and the licence files; from a git
//                                   checkout, exactly the files git tracks, at the commit bundle.json
//                                   names (a checkout with uncommitted changes or untracked files is
//                                   refused, so no local file is published)
//   dependencies/<Name>/<file>      the source packages Orca's dependency build downloaded, each one
//                                   checked against the SHA-256 that Orca's recipe pins
//   prebuilt/<Name>/<file>          the source packages of the libraries Orca ships prebuilt (GMP on
//                                   Windows), each pinned to the very binary the engine ships
//                                   (PREBUILT_SOURCES)
//   LICENSES/                       the licence texts shipped with the engine
//   BUILDING.md                     how to build the engine from this folder
//   bundle.json                     sketch-orca-source/1: what this is the source of, and what it holds
//   SHA256SUMS                      every other file
//
// The bundle is made only for a staged engine that matches the current lock, patches and front end.
// Run again, it skips when its inputs are unchanged and every file matches SHA256SUMS.
// --no-deps leaves the dependency packages and the prebuilt libraries' sources out (they stay listed
// in bundle.json, with their URLs). A built dependency or a shipped prebuilt library whose source is
// not pinned stops the step, unless --allow-missing.

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import type { Context } from '../orca.ts'
import { headCommit, listPatches } from './checkout.ts'
import { depsBuildArgs, depsConfigureArgs, formatCommand, orcaBuildArgs, orcaConfigureArgs } from './cmake.ts'
import { firstExisting, gitCandidates, type Platform } from './env.ts'
import { copyFile, hashTree, listFiles, nativePath, parseSha256Sums, sha256Bytes, sha256Sums, treeEntries, writeText } from './files.ts'
import type { OrcaLock } from './lock.ts'
import { sha256File, sha256Text, writeStamp } from './paths.ts'
import { capture, Log, runLogged, StepError } from './run.ts'
import { builtDependencies, dependencySourceDir, readManifest, verifyStaged, type EngineManifest, type StageCopy } from './stage.ts'
import { currentPlatform } from './toolchain.ts'

export const SOURCE_FORMAT = 'sketch-orca-source/1'
/** Bump when the bundle's layout changes: a bundle of an older version is made again. */
export const SOURCE_VERSION = 3

/**
 * The toolchain the engine has been built with, on Windows x64 (2026-10-30, 2026-11-01), as BUILDING.md (the
 * bundle's and this repository's) gives it. Other versions may work; these are known to.
 */
export const BUILT_WITH = {
  bun: '1.2.15',
  visualStudio: 'Visual Studio Community 2022 17.12',
  msvc: '14.42.34433',
  msvcToolset: 'v143',
  windowsSdk: '10.0.22000.0',
  cmake: '3.29.2',
  ninja: '1.12.0',
  strawberryPerl: '5.42.3.1',
} as const

// ---------------------------------------------------------------- dependency packages

export interface PinnedSource {
  urls: string[]
  /** Lower case. */
  sha256: string
}

/** The `URL ... URL_HASH SHA256=...` pairs of one of Orca's ExternalProject recipes, comments left out. */
export function parsePinnedSources(cmake: string): PinnedSource[] {
  const text = cmake
    .split(/\r?\n/)
    .map((l) => l.replace(/(^|\s)#.*$/, '$1'))
    .join('\n')
  const out: PinnedSource[] = []
  for (const m of text.matchAll(/\bURL\s+((?:"?[a-z][a-z0-9+.-]*:\/\/[^\s"]+"?\s+)+)URL_HASH\s+SHA256=([0-9A-Fa-f]{64})\b/g)) {
    const urls = m[1]!
      .trim()
      .split(/\s+/)
      .map((u) => u.replace(/^"|"$/g, ''))
    out.push({ urls, sha256: m[2]!.toLowerCase() })
  }
  return out
}

/** Every pinned package of Orca's `deps/<Name>/*.cmake`, by dependency name. */
export function pinnedSources(depsSrcDir: string): Map<string, PinnedSource[]> {
  const out = new Map<string, PinnedSource[]>()
  if (!existsSync(depsSrcDir)) return out
  for (const ent of readdirSync(depsSrcDir, { withFileTypes: true })) {
    if (!ent.isDirectory()) continue
    const pins: PinnedSource[] = []
    for (const f of readdirSync(join(depsSrcDir, ent.name))) {
      if (f.toLowerCase().endsWith('.cmake')) pins.push(...parsePinnedSources(readFileSync(join(depsSrcDir, ent.name, f), 'utf8')))
    }
    if (pins.length > 0) out.set(ent.name, pins)
  }
  return out
}

export interface DownloadedFile {
  file: string
  path: string
  sha256: string
  size: number
}

/** The packages in the download folder (`<downloads>/<Name>/<file>`), hashed. */
export function listDownloads(downloadsDir: string): Map<string, DownloadedFile[]> {
  const out = new Map<string, DownloadedFile[]>()
  if (!existsSync(downloadsDir)) return out
  for (const ent of readdirSync(downloadsDir, { withFileTypes: true })) {
    if (!ent.isDirectory()) continue
    const files: DownloadedFile[] = []
    for (const f of readdirSync(join(downloadsDir, ent.name), { withFileTypes: true })) {
      if (!f.isFile()) continue
      const path = join(downloadsDir, ent.name, f.name)
      files.push({ file: f.name, path, sha256: sha256File(path), size: statSync(path).size })
    }
    out.set(ent.name, files.sort((a, b) => (a.file < b.file ? -1 : 1)))
  }
  return out
}

export interface DependencyArchive {
  name: string
  file: string
  path: string
  url: string
  sha256: string
  size: number
}

export interface DependencyPlan {
  /** Packages to bundle: downloaded and matching a pinned hash. */
  archives: DependencyArchive[]
  /** Built from sources inside Orca's tree (part of the Orca archive). */
  inTree: string[]
  /** Built, but no downloaded package matches what Orca pins. */
  missing: string[]
  /** Downloaded files no recipe pins (an older release's package, say); left out. */
  unpinned: string[]
}

/**
 * Which source package each built dependency came from. A dependency without a pinned package
 * must build from Orca's own tree (`inTree`), or it is missing.
 */
export function planDependencies(built: string[], pinned: Map<string, PinnedSource[]>, downloads: Map<string, DownloadedFile[]>, isInTree: (name: string) => boolean): DependencyPlan {
  const plan: DependencyPlan = { archives: [], inTree: [], missing: [], unpinned: [] }
  for (const name of [...built].sort()) {
    const pins = pinned.get(name) ?? []
    const files = downloads.get(name) ?? []
    if (pins.length === 0) {
      if (isInTree(name)) plan.inTree.push(name)
      else plan.missing.push(name)
      continue
    }
    let found = false
    for (const f of files) {
      const pin = pins.find((p) => p.sha256 === f.sha256)
      if (!pin) {
        plan.unpinned.push(`${name}/${f.file}`)
        continue
      }
      found = true
      // The URL whose last path segment is the file's name (query and fragment aside), else the first.
      const url = pin.urls.find((u) => u.split(/[?#]/)[0]!.split('/').filter(Boolean).pop() === f.file) ?? pin.urls[0]!
      plan.archives.push({ name, file: f.file, path: f.path, url, sha256: f.sha256, size: f.size })
    }
    if (!found) plan.missing.push(name)
  }
  return plan
}

/** Libraries Orca ships prebuilt in its tree (Windows: GMP and MPFR), with the versions their headers give. */
export interface PrebuiltLibrary {
  name: string
  version: string
  /** In Orca's tree. */
  dir: string
  /** The file the staged engine ships it as (bin/<file>) and its SHA-256, when its manifest lists one (withPrebuiltSources). */
  binary?: string
  binarySha256?: string
  /** Its source package, when PREBUILT_SOURCES pins that very binary; in the bundle as prebuilt/<name>/<file>. */
  source?: { file: string; urls: string[]; size: number; sha256: string; provenance: string }
}

export function gmpHeaderVersion(text: string): string | undefined {
  const n = (k: string) => new RegExp(`#define\\s+${k}\\s+(\\d+)`).exec(text)?.[1]
  const major = n('__GNU_MP_VERSION')
  const minor = n('__GNU_MP_VERSION_MINOR')
  const patch = n('__GNU_MP_VERSION_PATCHLEVEL')
  return major && minor && patch ? `${major}.${minor}.${patch}` : undefined
}

export function mpfrHeaderVersion(text: string): string | undefined {
  return /#define\s+MPFR_VERSION_STRING\s+"([^"]+)"/.exec(text)?.[1]
}

/** The prebuilt libraries the staged engine ships (by the manifest's runtime list). */
export function prebuiltLibraries(srcDir: string, runtime: string[]): PrebuiltLibrary[] {
  const out: PrebuiltLibrary[] = []
  const read = (p: string) => (existsSync(p) ? readFileSync(p, 'utf8') : '')
  if (runtime.some((r) => /^libgmp/i.test(r))) out.push({ name: 'GMP', version: gmpHeaderVersion(read(join(srcDir, 'deps', 'GMP', 'gmp', 'include', 'gmp.h'))) ?? 'unknown', dir: 'deps/GMP/gmp' })
  if (runtime.some((r) => /^libmpfr/i.test(r))) out.push({ name: 'MPFR', version: mpfrHeaderVersion(read(join(srcDir, 'deps', 'MPFR', 'mpfr', 'include', 'mpfr.h'))) ?? 'unknown', dir: 'deps/MPFR/mpfr' })
  return out
}

/**
 * The source package of a library Orca ships prebuilt, pinned to the exact binary it is the source
 * of: the bundle takes it only for that binary (by its SHA-256 in the staged engine's manifest).
 */
export interface PrebuiltSource {
  name: string
  version: string
  /** The file in the engine's bin/, the architecture it is built for, and its SHA-256. */
  binary: string
  arch: string
  binarySha256: string
  /** The source package: its file name, where it is published (tried in order), its size and SHA-256. */
  file: string
  urls: string[]
  size: number
  sha256: string
  /** How the binary was made from it, as far as is known (BUILDING.md: "The binary is ..."). */
  provenance: string
}

/**
 * The prebuilt binaries whose exact source is known.
 *
 * GMP 5.0.1, Windows x64. OrcaSlicer's deps/GMP/gmp at v2.4.2 (libgmp-10.dll, include/gmp.h and
 * gmp.README) is byte for byte the GMP of CGAL's win64 auxiliary libraries
 * (CGAL-5.4.1-win64-auxiliary-libraries-gmp-mpfr.zip, from github.com/CGAL/cgal releases): a
 * MinGW-w64 build of the GMP 5.0.1 release. The DLL's debug strings name the gmp-5.0.1/ sources and
 * GCC 4.4.5 20100604 (prerelease, mingw-w64) with binutils 2.20.51, and gmp.h is gmp-5.0.1's
 * gmp-h.in with only configure's values filled in (a 64-bit DLL; CC gcc; CFLAGS -O2 -pedantic -m64
 * -std=gnu99 -mtune=k8 -march=k8). The package is the GMP project's release archive: the same
 * SHA-256 from gmplib.org, ftp.gnu.org, ftpmirror.gnu.org and mirrors.kernel.org, and a good
 * signature (gmp-5.0.1.tar.bz2.sig) by the GMP Project key
 * 73D4 6C36 6746 1E4B D939 7249 5D6D 47DF DB89 9F46 from the GNU keyring (checked 2026-10-31).
 */
export const PREBUILT_SOURCES: readonly PrebuiltSource[] = [
  {
    name: 'GMP',
    version: '5.0.1',
    binary: 'libgmp-10.dll',
    arch: 'x64',
    binarySha256: 'fec43cb18ce0ec8a5dd6ad1db745747167310ccb92b5acff0d445a8b3013f009',
    file: 'gmp-5.0.1.tar.bz2',
    urls: ['https://gmplib.org/download/gmp/gmp-5.0.1.tar.bz2', 'https://ftp.gnu.org/gnu/gmp/gmp-5.0.1.tar.bz2'],
    size: 2006109,
    sha256: 'a2a610f01fd3298dc08c87bf30498c2402590e1bcb227fc40b15ee6d280939fb',
    provenance:
      "CGAL's MinGW-w64 build of this release (GCC 4.4.5 20100604, binutils 2.20.51; gmp.h records CC gcc and CFLAGS -O2 -pedantic -m64 -std=gnu99 -mtune=k8 -march=k8), as CGAL publishes it in its win64 auxiliary libraries; OrcaSlicer's build copies it from deps/GMP/gmp and does not rebuild it, and CGAL's package does not say how it was configured",
  },
]

/** The file each prebuilt library ships as, directly in the engine's bin/. */
const PREBUILT_BINARY: Record<string, RegExp> = { GMP: /^bin\/libgmp[^/]*$/i, MPFR: /^bin\/libmpfr[^/]*$/i }

/**
 * The prebuilt libraries with the binary the staged engine ships of each (from its manifest's
 * files, with the SHA-256) and, when `pins` has that very binary for the engine's architecture,
 * its source package.
 */
export function withPrebuiltSources(libs: PrebuiltLibrary[], m: Pick<EngineManifest, 'files' | 'arch'>, pins: readonly PrebuiltSource[] = PREBUILT_SOURCES): PrebuiltLibrary[] {
  return libs.map((lib) => {
    const re = PREBUILT_BINARY[lib.name]
    const bin = re ? m.files.find((f) => re.test(f.path)) : undefined
    if (!bin) return { ...lib }
    const out: PrebuiltLibrary = { ...lib, binary: bin.path, binarySha256: bin.sha256 }
    const pin = pins.find((p) => p.name === lib.name && p.version === lib.version && p.arch === m.arch && `bin/${p.binary}` === bin.path && p.binarySha256 === bin.sha256)
    if (pin) out.source = { file: pin.file, urls: [...pin.urls], size: pin.size, sha256: pin.sha256, provenance: pin.provenance }
    return out
  })
}

/** A prebuilt library for messages: GMP 5.0.1 (bin/libgmp-10.dll, SHA-256 ...). */
export function prebuiltLabel(p: PrebuiltLibrary): string {
  return `${p.name} ${p.version}${p.binary ? ` (${p.binary}, SHA-256 ${p.binarySha256})` : ''}`
}

/** Gets the bytes at a URL; throws when it cannot. */
export type Fetcher = (url: string) => Promise<Uint8Array>

/** A Fetcher on the global fetch, following redirects. */
export const httpFetcher: Fetcher = async (url) => {
  const res = await fetch(url, { redirect: 'follow' })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return new Uint8Array(await res.arrayBuffer())
}

/** Where `source` keeps a prebuilt library's source package: <work>/prebuilt/<Name>/<file>. */
export function prebuiltSourcePath(prebuiltDir: string, name: string, file: string): string {
  return join(prebuiltDir, name, file)
}

/**
 * The source package of a pinned prebuilt library, in `prebuiltDir`: a copy already there is kept
 * when its size and SHA-256 are the pin's (else removed), otherwise the pin's URLs are tried in
 * order and the first download of the pinned size and SHA-256 is kept. Throws, naming what each URL
 * gave, when none gives it.
 */
export async function ensurePrebuiltSource(lib: PrebuiltLibrary, prebuiltDir: string, fetcher: Fetcher, line: (msg: string) => void): Promise<string> {
  const s = lib.source
  if (!s) throw new StepError(`no pinned source for the prebuilt ${prebuiltLabel(lib)}`)
  const path = prebuiltSourcePath(prebuiltDir, lib.name, s.file)
  if (existsSync(path)) {
    if (statSync(path).size === s.size && sha256File(path) === s.sha256) return path
    line(`source: ${path} is not the pinned ${s.file} (SHA-256 ${s.sha256}); getting it again`)
    rmSync(path, { force: true })
  }
  const failures: string[] = []
  for (const url of s.urls) {
    let bytes: Uint8Array
    try {
      line(`source: downloading ${url}`)
      bytes = await fetcher(url)
    } catch (err) {
      failures.push(`${url}: ${(err as Error).message}`)
      continue
    }
    const sha256 = sha256Bytes(bytes)
    if (bytes.length !== s.size || sha256 !== s.sha256) {
      failures.push(`${url}: ${bytes.length} bytes with SHA-256 ${sha256}, not ${s.size} bytes with ${s.sha256}`)
      continue
    }
    mkdirSync(dirname(path), { recursive: true })
    const partial = `${path}.partial`
    writeFileSync(partial, bytes)
    renameSync(partial, path)
    return path
  }
  throw new StepError(`cannot get the source of the prebuilt ${prebuiltLabel(lib)}: ${failures.join('; ')}`)
}

// ---------------------------------------------------------------- the bundle

export function bundleName(m: EngineManifest): string {
  return `sketch-orca-${m.engine.version || '0'}-orca-${m.orca.version}-${m.platform}-${m.arch || 'unknown'}-source`
}

export function orcaArchiveName(lock: OrcaLock): string {
  return `orca/OrcaSlicer-${lock.tag}.tar.gz`
}

/** Top-level folders of the repository that hold local work, never source (.gitignore). */
export const LOCAL_WORK_FOLDERS = ['work', 'dist', 'logs'] as const

/**
 * Files of this repository that belong in the bundle (as engine/): all of it, apart from git's own
 * folder and what .gitignore leaves out: node_modules, the local work folders (LOCAL_WORK_FOLDERS),
 * logs, and editor and system litter. Called for folders too (listFiles).
 */
export function keepEngineFile(rel: string): boolean {
  const parts = rel.split('/')
  const name = parts[parts.length - 1]!
  if (parts.includes('node_modules') || parts.includes('.git')) return false
  if ((LOCAL_WORK_FOLDERS as readonly string[]).includes(parts[0]!)) return false
  return name !== '.DS_Store' && name !== 'Thumbs.db' && !name.endsWith('~') && !name.endsWith('.log')
}

/**
 * The files of the repository the bundle carries as engine/, and the commit they are from. In a git
 * checkout (a `.git` in `dir`): exactly the files git tracks there (that keepEngineFile keeps), at
 * HEAD; a tree with changes that are not committed, or with files git neither tracks nor ignores,
 * is refused, so no local file (notes, keys) goes into a published bundle. Outside a checkout (a
 * bundle's own engine/ folder): every file keepEngineFile keeps, and no commit.
 */
export function repositoryFiles(git: string | undefined, dir: string): { files: string[]; commit: string | null } {
  if (!existsSync(join(dir, '.git'))) return { files: listFiles(dir, (rel) => keepEngineFile(rel)), commit: null }
  if (git === undefined) throw new StepError(`${dir} is a git checkout, and git was not found to list the files it tracks (set SKETCH_ORCA_GIT)`)
  const status = capture(git, ['-C', dir, '-c', 'core.quotePath=false', 'status', '--porcelain=v1', '-z', '--untracked-files=all'])
  if (status.code !== 0) throw new StepError(`git status failed in ${dir}: ${status.stderr.trim()}`)
  // Each entry is "XY <path>"; a rename or copy is followed by one more field, its old path.
  const fields = status.stdout.split('\0')
  const changed: string[] = []
  for (let i = 0; i < fields.length; i++) {
    const entry = fields[i]!
    if (entry.length < 4) continue
    changed.push(entry.slice(3))
    if (/[RC]/.test(entry.slice(0, 2))) i++
  }
  if (changed.length > 0) {
    const shown = changed.slice(0, 5).join(', ') + (changed.length > 5 ? `, and ${changed.length - 5} more` : '')
    throw new StepError(`${dir} has changes that are not committed, or files git does not track (${shown}); commit or remove them, then run source again`)
  }
  const listed = capture(git, ['-C', dir, '-c', 'core.quotePath=false', 'ls-files', '-z'])
  if (listed.code !== 0) throw new StepError(`git ls-files failed in ${dir}: ${listed.stderr.trim()}`)
  const commit = headCommit(git, dir)
  if (commit === undefined) throw new StepError(`${dir} has no commit to bundle`)
  const files = listed.stdout.split('\0').filter((rel) => rel.length > 0 && keepEngineFile(rel)).sort()
  return { files, commit }
}

/** A prebuilt library's source package as the bundle copies it (ensurePrebuiltSource gives the path). */
export interface PrebuiltArchive {
  name: string
  file: string
  path: string
}

/**
 * What the bundle copies: the repository, the dependency packages and the prebuilt libraries'
 * source packages (both unless left out), and the staged licences.
 */
export function bundleCopies(o: { engineDir: string; engineFiles: string[]; archives: DependencyArchive[]; prebuiltArchives?: PrebuiltArchive[]; withDeps: boolean; licensesDir: string; licenseFiles: string[] }): StageCopy[] {
  const out: StageCopy[] = []
  for (const rel of o.engineFiles) out.push({ from: nativePath(o.engineDir, rel), to: `engine/${rel}` })
  if (o.withDeps) for (const a of o.archives) out.push({ from: a.path, to: `dependencies/${a.name}/${a.file}` })
  if (o.withDeps) for (const p of o.prebuiltArchives ?? []) out.push({ from: p.path, to: `prebuilt/${p.name}/${p.file}` })
  for (const rel of o.licenseFiles) out.push({ from: nativePath(o.licensesDir, rel), to: `LICENSES/${rel}` })
  const seen = new Set<string>()
  for (const c of out) {
    const key = c.to.toLowerCase()
    if (seen.has(key)) throw new StepError(`two files would land at ${c.to} in the source bundle`)
    seen.add(key)
  }
  return out.sort((a, b) => (a.to < b.to ? -1 : a.to > b.to ? 1 : 0))
}

export interface BundleInfo {
  format: typeof SOURCE_FORMAT
  sourceVersion: number
  name: string
  engine: { version: string; platform: Platform; arch: string; manifestSha256: string }
  orca: { repo: string; tag: string; version: string; commit: string; license: string; archive: string }
  patches: { name: string; sha256: string }[]
  cli: string
  /** The commit of this repository that engine/ holds (its files as git tracks them); null when it was not made from a git checkout. */
  repository: { commit: string | null }
  dependencies: {
    included: boolean
    archives: { name: string; file: string; url: string; sha256: string; size: number }[]
    inTree: string[]
    missing: string[]
    prebuilt: PrebuiltLibrary[]
  }
  inputs: string
  createdAt: string
}

/** The build instructions in the bundle; the command lines are the pipeline's own (cmake.ts). */
export function buildingText(o: { lock: OrcaLock; manifest: EngineManifest; patches: string[]; plan: DependencyPlan; prebuilt: PrebuiltLibrary[]; withDeps: boolean }): string {
  const { lock, manifest: m } = o
  const platform = m.platform
  const ninja = platform === 'win32' ? '<ninja.exe>' : '<ninja>'
  const deps = depsConfigureArgs({ ninja, sourceDir: '<src>/deps', buildDir: '<deps>', downloadDir: '<downloads>', ignorePrefixes: [], platform })
  const orca = orcaConfigureArgs({
    ninja,
    sourceDir: '<src>',
    buildDir: '<build>',
    depsPrefix: '<deps>/OrcaSlicer_dep/usr/local',
    cliDir: '<bundle>/engine/cli',
    commit: lock.commit,
    tag: lock.tag,
    ignorePrefixes: [],
    platform,
  })
  const exe = platform === 'win32' ? 'sketch-orca.exe' : 'sketch-orca'
  const lines = [
    `# Building sketch-orca ${m.engine.version} from this source`,
    '',
    `This folder is the complete corresponding source of the sketch-orca engine ${m.engine.version}`,
    `staged for ${platform} (${m.arch}): OrcaSlicer ${lock.tag} (commit ${lock.commit}) with the changes`,
    'in engine/patches, the command-line front end in engine/cli, and the scripts that fetch, build,',
    'test and stage it (engine/scripts). It is offered under the GNU Affero General Public License',
    'version 3; the licence texts are in LICENSES/.',
    '',
    '## Tools',
    '',
    'The engine has been built on Windows x64 with these; other versions may work, these are known to:',
    '',
    `- Bun ${BUILT_WITH.bun}, which runs the pipeline scripts, and git.`,
    `- ${BUILT_WITH.visualStudio} with the MSVC x64 tools ${BUILT_WITH.msvc} (toolset ${BUILT_WITH.msvcToolset}) and the Windows SDK ${BUILT_WITH.windowsSdk}.`,
    `- CMake ${BUILT_WITH.cmake} and Ninja ${BUILT_WITH.ninja} (the copies Strawberry Perl ships in its c\\bin; the CMake files ask for CMake 3.13 or later).`,
    `- Strawberry Perl ${BUILT_WITH.strawberryPerl} (64-bit), for OpenSSL's build.`,
    '- Linux and macOS: a C and C++ compiler (gcc or clang), make, autoconf, automake, libtool, m4',
    '  and perl. These platforms have not been tried yet.',
    '',
    '## With network access',
    '',
    'From the folder that holds engine/:',
    '',
    '```',
    'bun engine/scripts/orca.ts fetch   # clones the locked tag from GitHub and checks the commit',
    'bun engine/scripts/orca.ts build   # Orca\'s dependencies without the GUI ones, then libslic3r and sketch-orca',
    'bun engine/scripts/orca.ts smoke   # slices test models and checks the results',
    'bun engine/scripts/orca.ts stage   # the engine folder with its manifest',
    '```',
    '',
    'The work folder is %LOCALAPPDATA%\\SketchDev\\orca-engine on Windows and',
    '~/.local/share/SketchDev/orca-engine elsewhere; SKETCH_ORCA_HOME overrides it. engine/README.md',
    'describes every command.',
    '',
    '## From this folder alone',
    '',
    'Work outside any git work tree: <src>, <deps>, <build> and the folder the commands run from.',
    'Inside one, `git apply` skips the patches without an error, and Orca\'s dependency build points',
    'its own patch commands at paths relative to that tree (IN_GIT_REPO in deps/CMakeLists.txt). If',
    'that cannot be avoided, set GIT_CEILING_DIRECTORIES to a folder that holds all of them, as the',
    'pipeline does with its work folder.',
    '',
    `1. Unpack ${orcaArchiveName(lock)}; call the folder it holds <src>.`,
    '2. In <src>, apply the patches in this order. `git apply` works outside a repository;',
    '   `-c core.autocrlf=false` keeps the patched files\' LF line endings, which Git for Windows\'',
    '   default (core.autocrlf=true) would turn into CRLF:',
    ...o.patches.map((p) => `   - git -c core.autocrlf=false apply <bundle>/engine/patches/${p}`),
    o.withDeps
      ? '3. Copy dependencies/ to a folder <downloads>. Orca\'s dependency build looks for each package there under its own name and, when its SHA-256 matches the pin, does not download it.'
      : '3. The dependency packages were left out of this bundle; bundle.json lists each one with its URL and SHA-256. Put them in <downloads>/<Name>/.',
    '4. Build the dependencies (on Windows, in a Visual Studio x64 developer prompt, with Strawberry Perl\'s perl first on PATH and its C compiler folder off PATH):',
    '',
    '```',
    formatCommand('cmake', deps),
    formatCommand('cmake', depsBuildArgs('<deps>', 4)),
    '```',
    '',
    '   -DDEP_OPENCV_IPP=OFF (patch 0001) builds OpenCV without Intel\'s IPP-ICV, which OpenCV\'s',
    '   build would otherwise download from outside this bundle. On Windows the pipeline also sets',
    '   CL=/d1trimfile:"<folder>\\\\" for the folder that holds <src>, <deps> and <build> and for',
    '   <bundle>/engine, before configuring, so the source paths compiled into the engine do not',
    '   name those folders; that is optional.',
    ...(platform === 'win32'
      ? [
          '   -DDEP_MSVC_STATIC_RUNTIME=ON (patch 0005) builds every dependency with MSVC\'s static',
          '   runtime library (/MT), as the engine is built in step 5, so that no Visual C++ runtime DLL',
          '   ships with it.',
        ]
      : []),
    '',
    '5. Build the engine:',
    '',
    '```',
    formatCommand('cmake', orca),
    formatCommand('cmake', orcaBuildArgs('<build>', 8)),
    '```',
    '',
    ...(platform === 'win32'
      ? [
          'CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded, with policy CMP0091 NEW, is the static runtime for',
          'Orca and the front end; engine/cli/CMakeLists.txt refuses to configure with any other.',
          '',
        ]
      : []),
    `The engine is <build>/sketch-orca/${exe}. \`${exe} version\` prints the Orca commit it was built from.`,
    '',
    '## Dependencies',
    '',
    ...o.plan.archives.map((a) => `- ${a.name}: ${a.file} from ${a.url} (SHA-256 ${a.sha256})`),
    ...o.plan.inTree.map((n) => `- ${n}: built from its sources inside the OrcaSlicer tree`),
    ...o.prebuilt.map((p) => prebuiltLine(p, o.withDeps)),
    ...o.plan.missing.map((n) => `- ${n}: built, but its source package is NOT in this bundle`),
    '',
  ]
  return lines.join('\n')
}

/** BUILDING.md's line for a library Orca ships prebuilt: where it comes from, and where its source is. */
export function prebuiltLine(p: PrebuiltLibrary, withDeps: boolean): string {
  const ships = `- ${p.name} ${p.version}: OrcaSlicer ships it prebuilt for Windows in ${p.dir}${p.binary ? `, and the engine ships it as ${p.binary} (SHA-256 ${p.binarySha256})` : ''}; its licence is in LICENSES/deps/${p.name}.`
  if (!p.source) return `${ships} Its source is NOT in this bundle: no source package is pinned for this binary (PREBUILT_SOURCES in engine/scripts/lib/bundle.ts).`
  const from = `${p.source.urls[0]} (SHA-256 ${p.source.sha256})`
  const where = withDeps ? `Its source is prebuilt/${p.name}/${p.source.file}, from ${from}.` : `Its source, ${p.source.file}, was left out of this bundle; it is at ${from}.`
  return `${ships} ${where} The binary is ${p.source.provenance}.`
}

/** Problems with a bundle folder against its SHA256SUMS: changed, missing or unlisted files. */
export function verifySums(dir: string): string[] {
  const sumsPath = join(dir, 'SHA256SUMS')
  if (!existsSync(sumsPath)) return ['no SHA256SUMS']
  let sums: Map<string, string>
  try {
    sums = parseSha256Sums(readFileSync(sumsPath, 'utf8'))
  } catch (err) {
    return [(err as Error).message]
  }
  const problems: string[] = []
  const present = new Set(listFiles(dir).filter((p) => p !== 'SHA256SUMS'))
  for (const p of present) if (!sums.has(p)) problems.push(`unlisted file ${p}`)
  for (const [p, hash] of sums) {
    if (!present.has(p)) problems.push(`missing ${p}`)
    else if (sha256File(nativePath(dir, p)) !== hash) problems.push(`changed ${p}`)
  }
  return problems
}

function findGit(platform: Platform): string {
  const git = firstExisting(gitCandidates(platform, process.env))
  if (!git) throw new StepError('git not found (set SKETCH_ORCA_GIT)')
  return git
}

/** The staged engine, checked against the current lock, patches and front end. */
export function stagedFor(ctx: Context): EngineManifest {
  const dir = join(ctx.work.dist, 'engine')
  if (!existsSync(join(dir, 'manifest.json'))) throw new StepError(`no staged engine in ${dir}; run stage first`)
  const m = readManifest(dir)
  if (m.orca.commit !== ctx.lock.commit || m.orca.tag !== ctx.lock.tag) throw new StepError(`the staged engine is from ${m.orca.tag} (${m.orca.commit}), the lock is at ${ctx.lock.tag}; run stage`)
  const patches = listPatches(ctx.engine.patches).map((p) => ({ name: p.name, sha256: p.sha256 }))
  if (JSON.stringify(m.patches) !== JSON.stringify(patches)) throw new StepError('the staged engine was built with other patches than patches/; run build and stage')
  if (m.cli !== hashTree(ctx.engine.cli)) throw new StepError('cli/ changed since the engine was staged; run build and stage')
  const problems = verifyStaged(dir, m)
  if (problems.length > 0) throw new StepError(`the staged engine does not match its manifest (${problems.slice(0, 3).join('; ')}); run stage`)
  return m
}

export async function runSource(ctx: Context, log: Log, fetcher: Fetcher = httpFetcher): Promise<number> {
  const w = ctx.work
  const platform = currentPlatform()
  const withDeps = !ctx.args.includes('--no-deps')
  const allowMissing = ctx.args.includes('--allow-missing')
  const m = stagedFor(ctx)
  const git = findGit(platform)
  if (headCommit(git, w.src) !== ctx.lock.commit) throw new StepError(`${w.src} is not at the locked commit ${ctx.lock.commit}; run fetch`)

  const built = builtDependencies(w.deps)
  const plan = planDependencies(built, pinnedSources(join(w.src, 'deps')), listDownloads(w.downloads), (name) => {
    const dir = dependencySourceDir(w, name)
    return dir !== undefined && dir.startsWith(w.src)
  })
  for (const u of plan.unpinned) log.line(`source: ${u} is pinned by no recipe of this release; left out`)
  const prebuilt = withPrebuiltSources(prebuiltLibraries(w.src, m.runtime), m)
  const unsourced = prebuilt.filter((p) => !p.source)
  const missing = [
    ...(plan.missing.length > 0 ? [`no pinned source package for the built dependencies ${plan.missing.join(', ')}; fetch them with build`] : []),
    ...(unsourced.length > 0 ? [`no pinned source for the prebuilt ${unsourced.map(prebuiltLabel).join(', ')}; PREBUILT_SOURCES (scripts/lib/bundle.ts) pins the binaries whose source is known`] : []),
  ]
  if (missing.length > 0 && !allowMissing) throw new StepError(`${missing.join('; ')}; or pass --allow-missing`)
  // The prebuilt libraries' sources, from the work folder or downloaded there (checked against the pins).
  const prebuiltArchives: PrebuiltArchive[] = []
  if (withDeps) {
    for (const p of prebuilt) if (p.source) prebuiltArchives.push({ name: p.name, file: p.source.file, path: await ensurePrebuiltSource(p, w.prebuilt, fetcher, (msg) => log.line(msg)) })
  }
  const repository = repositoryFiles(git, ctx.engine.engine)
  const engineFiles = repository.files
  if (repository.commit === null) log.line(`source: ${ctx.engine.engine} is not a git checkout: every file in it but the local work folders goes in`)
  const licensesDir = join(w.dist, 'engine', 'LICENSES')
  const licenseFiles = listFiles(licensesDir)
  const copies = bundleCopies({ engineDir: ctx.engine.engine, engineFiles, archives: plan.archives, prebuiltArchives, withDeps, licensesDir, licenseFiles })
  const manifestSha256 = sha256File(join(w.dist, 'engine', 'manifest.json'))
  const inputs = sha256Text(
    JSON.stringify({
      sourceVersion: SOURCE_VERSION,
      manifestSha256,
      engine: engineFiles.map((rel) => [rel, sha256File(nativePath(ctx.engine.engine, rel))]),
      commit: repository.commit,
      archives: plan.archives.map((a) => [a.name, a.file, a.sha256]),
      withDeps,
      missing: plan.missing,
      prebuilt,
    }),
  )
  const name = bundleName(m)
  const root = join(w.dist, 'source')
  const dir = join(root, name)
  if (existsSync(join(dir, 'bundle.json'))) {
    try {
      const old = JSON.parse(readFileSync(join(dir, 'bundle.json'), 'utf8')) as Partial<BundleInfo>
      if (old.inputs === inputs) {
        const problems = verifySums(dir)
        if (problems.length === 0) {
          log.line(`source: ${dir} is current; skipping`)
          return 0
        }
        log.line(`source: the bundle no longer matches its SHA256SUMS (${problems.slice(0, 3).join('; ')}); making it again`)
      }
    } catch {
      // Make it again.
    }
  }

  const tmp = join(root, `${name}.partial`)
  rmSync(tmp, { recursive: true, force: true })
  const archive = nativePath(tmp, orcaArchiveName(ctx.lock))
  mkdirSync(join(tmp, 'orca'), { recursive: true })
  log.line(`source: archiving OrcaSlicer ${ctx.lock.tag} at ${ctx.lock.commit}`)
  await runLogged(git, ['-C', w.src, '-c', 'core.autocrlf=false', '-c', 'core.eol=lf', 'archive', '--format=tar.gz', `--prefix=OrcaSlicer-${ctx.lock.tag}/`, '-o', archive, ctx.lock.commit], { log })
  log.line(`source: copying ${copies.length} files${withDeps ? ` (${plan.archives.length} dependency packages and ${prebuiltArchives.length} prebuilt libraries' sources, linked where the volume allows)` : ''}`)
  for (const c of copies) copyFile(c.from, nativePath(tmp, c.to), { link: c.to.startsWith('dependencies/') || c.to.startsWith('prebuilt/') })
  const patches = listPatches(ctx.engine.patches)
  writeText(join(tmp, 'BUILDING.md'), buildingText({ lock: ctx.lock, manifest: m, patches: patches.map((p) => p.name), plan, prebuilt, withDeps }))
  const info: BundleInfo = {
    format: SOURCE_FORMAT,
    sourceVersion: SOURCE_VERSION,
    name,
    engine: { version: m.engine.version, platform: m.platform, arch: m.arch, manifestSha256 },
    orca: { repo: ctx.lock.repo, tag: ctx.lock.tag, version: ctx.lock.version, commit: ctx.lock.commit, license: ctx.lock.license, archive: orcaArchiveName(ctx.lock) },
    patches: patches.map((p) => ({ name: p.name, sha256: p.sha256 })),
    cli: m.cli,
    repository: { commit: repository.commit },
    dependencies: {
      included: withDeps,
      archives: plan.archives.map((a) => ({ name: a.name, file: a.file, url: a.url, sha256: a.sha256, size: a.size })),
      inTree: plan.inTree,
      missing: plan.missing,
      prebuilt,
    },
    inputs,
    createdAt: new Date().toISOString(),
  }
  writeText(join(tmp, 'bundle.json'), JSON.stringify(info, null, 2) + '\n')
  writeText(join(tmp, 'SHA256SUMS'), sha256Sums(treeEntries(tmp, (rel) => rel !== 'SHA256SUMS')))
  const problems = verifySums(tmp)
  if (problems.length > 0) throw new StepError(`the new bundle does not match its own SHA256SUMS: ${problems.slice(0, 3).join('; ')}`)

  rmSync(dir, { recursive: true, force: true })
  renameSync(tmp, dir)
  const bytes = listFiles(dir).reduce((n, rel) => n + statSync(nativePath(dir, rel)).size, 0)
  writeStamp(w.state, 'source', inputs, { name, bytes })
  log.line(`source: ${dir} (${(bytes / 1048576).toFixed(0)} MB${withDeps ? '' : ', without the dependency packages'})`)
  for (const p of unsourced) log.line(`source: note: ${prebuiltLabel(p)} ships prebuilt from Orca's tree; its source is not in the bundle (--allow-missing)`)
  if (!withDeps) for (const p of prebuilt) if (p.source) log.line(`source: note: the source of the prebuilt ${p.name} ${p.version} (${p.source.file}) is left out (--no-deps)`)
  if (plan.missing.length > 0) log.line(`source: note: no source package for ${plan.missing.join(', ')} (--allow-missing)`)
  return 0
}
