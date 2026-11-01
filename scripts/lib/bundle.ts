// `source`: the complete corresponding source of the staged engine (AGPL-3.0, section 6), in
// <work>/dist/source/<name>/:
//
//   orca/OrcaSlicer-<tag>.tar.gz    OrcaSlicer at the locked commit, as upstream has it (git archive),
//                                   less the binaries of its tree, none of which the engine's build
//                                   uses (orcaArchiveExcludes)
//   engine/                         this repository (sketch-orca): the lock, the patches, the
//                                   command-line front end (cli/), the protocol, the pipeline scripts
//                                   and their tests, the README and the licence files; from a git
//                                   checkout, exactly the files git tracks, at the commit bundle.json
//                                   names (a checkout with uncommitted changes or untracked files is
//                                   refused, so no local file is published)
//   dependencies/<Name>/<file>      the source packages Orca's dependency build downloaded, each one
//                                   checked against the SHA-256 that Orca's recipe pins (GMP and MPFR
//                                   too: on Windows patch 0007 builds them from these with MinGW-w64)
//   LICENSES/                       the licence texts shipped with the engine
//   BUILDING.md                     how to build the engine from this folder, with the pinned
//                                   MinGW-w64 toolchain on Windows
//   bundle.json                     sketch-orca-source/1: what this is the source of, and what it holds
//   SHA256SUMS                      every other file
//
// The bundle is made only for a staged engine that matches the current lock, patches and front end.
// Run again, it skips when its inputs are unchanged and every file matches SHA256SUMS.
// --no-deps leaves the dependency packages out (they stay listed in bundle.json, with their URLs),
// and the bundle then says it is not complete. A built dependency whose source is not pinned, a file
// of the engine that is a binary of Orca's tree (its prebuilt GMP, say), and a binary the Orca
// archive keeps stop the step, unless --allow-missing; bundle.json and BUILDING.md then say the
// bundle is not complete.

import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync } from 'node:fs'
import { join } from 'node:path'
import type { Context } from '../orca.ts'
import { headCommit, listPatches } from './checkout.ts'
import { depsBuildArgs, depsConfigureArgs, formatCommand, orcaBuildArgs, orcaConfigureArgs } from './cmake.ts'
import { firstExisting, gitCandidates, type Platform } from './env.ts'
import { copyFile, hashTree, listFiles, nativePath, parseSha256Sums, sha256Bytes, sha256Sums, treeEntries, writeText } from './files.ts'
import type { OrcaLock } from './lock.ts'
import { MINGW_PACKAGES, MSYS2_BASE, type MingwPackage } from './mingw.ts'
import { sha256File, sha256Text, writeStamp } from './paths.ts'
import { capture, Log, runLogged, StepError } from './run.ts'
import { builderNeedles, builtDependencies, dependencySourceDir, privacyProblems, privacyProblemsIn, privacyRefusal, readManifest, USER_FOLDER_NEEDLES, verifyStaged, type EngineManifest, type StageCopy } from './stage.ts'
import { currentPlatform } from './toolchain.ts'

export const SOURCE_FORMAT = 'sketch-orca-source/1'
/** Bump when the bundle's layout changes: a bundle of an older version is made again. */
export const SOURCE_VERSION = 6

/**
 * The toolchain the engine has been built with, on Windows x64 (2026-10-30, 2026-11-01), as BUILDING.md (the
 * bundle's and this repository's) gives it. Other versions may work; these are known to. The
 * MinGW-w64 toolchain is the one mingw.ts pins.
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
  mingw: 'MSYS2 (base system of 2026-09-27) with the GCC 16.2.0 of its mingw64 environment (mingw-w64 14.0.0, binutils 2.47)',
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

// ---------------------------------------------------------------- the binaries of Orca's tree

/** Programs and libraries, by their file name. */
const BINARY_FILE = /\.(dll|exe|lib|a|so(\.\d+)*|dylib|pdb|jar)$/i

/** Whether `rel` (a path of OrcaSlicer's tree) is a program or a library, by its name. */
export function isBinaryPath(rel: string): boolean {
  return BINARY_FILE.test(rel)
}

/**
 * What the bundle's Orca archive leaves out of OrcaSlicer's tree, as git pathspecs: the binaries in
 * it, none of which the engine's build uses, and which would otherwise ship without their source.
 * The GUI's WebView2 loader, the gettext and 7-Zip programs of tools/, and the prebuilt GMP and MPFR
 * DLLs and import libraries Orca's Windows build copies (CGAL's builds of GMP 5.0.1 and MPFR 3.0.0,
 * whose build recipe is not published): the engine's build makes GMP and MPFR from their source
 * packages on every platform, on Windows with MinGW-w64 (patch 0007). Their headers and READMEs,
 * which are not binaries, stay.
 */
export function orcaArchiveExcludes(): string[] {
  return ['deps/WebView2', 'tools/*.exe', 'deps/GMP/gmp/lib', 'deps/MPFR/mpfr/lib']
}

/** Whether `rel` is one of `excludes` (a folder and what is in it, or a * pattern within one folder). */
function excluded(rel: string, excludes: string[]): boolean {
  return excludes.some((e) => (e.includes('*') ? new RegExp(`^${e.split('*').map((x) => x.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]*')}$`).test(rel) : rel === e || rel.startsWith(`${e}/`)))
}

/**
 * The binaries the Orca archive keeps (`files`: the paths of the commit's tree, less `excludes`).
 * The bundle has the source of none of them, so each one keeps it from being complete.
 */
export function archivedBinaries(files: string[], excludes: string[]): string[] {
  return files.filter((rel) => isBinaryPath(rel) && !excluded(rel, excludes))
}

/** The git pathspecs that leave `excludes` out of `git archive`. */
export function excludePathspecs(excludes: string[]): string[] {
  return excludes.map((e) => (e.includes('*') ? `:(exclude,glob)${e}` : `:(exclude)${e}`))
}

/** A file of `commit` (`rel`, with forward slashes), as `git archive` writes a binary. */
export function commitFileBytes(git: string, dir: string, commit: string, rel: string): Buffer {
  const r = spawnSync(git, ['-C', dir, 'cat-file', 'blob', `${commit}:${rel}`], { windowsHide: true, maxBuffer: 1 << 30 })
  if (r.error) throw r.error
  if (r.status !== 0) throw new StepError(`git cat-file blob ${commit}:${rel} failed in ${dir}: ${r.stderr.toString().trim()}`)
  return r.stdout
}

/** The SHA-256 of a file of `commit` (commitFileBytes). */
export function commitFileSha256(git: string, dir: string, commit: string, rel: string): string {
  return sha256Bytes(commitFileBytes(git, dir, commit, rel))
}

/**
 * The files of the staged engine (its manifest's) that are byte for byte a binary of OrcaSlicer's
 * tree (`treeBinaries`: path and SHA-256 of each, excluded from the archive or not), such as the
 * prebuilt GMP that Orca's Windows build copies when GMP is not built from source: the bundle has
 * no source for them, so it is not complete. Empty when the engine ships none.
 */
export function shippedTreeBinaries(files: readonly { path: string; sha256: string }[], treeBinaries: readonly { path: string; sha256: string }[]): string[] {
  const bySha = new Map<string, string[]>()
  for (const b of treeBinaries) bySha.set(b.sha256, [...(bySha.get(b.sha256) ?? []), b.path])
  return files.filter((f) => bySha.has(f.sha256)).map((f) => `${f.path} (${bySha.get(f.sha256)!.join(', ')})`)
}

/**
 * GMP and MPFR on Windows (patch 0007): built from the source packages of dependencies/GMP and
 * dependencies/MPFR by the MinGW-w64 toolchain of MSYS2, which the pipeline unpacks from these
 * pinned packages (mingw.ts). BUILDING.md lists them; bundle.json too (toolchain.mingw).
 */
export function mingwToolchain(platform: Platform): MingwPackage[] {
  return platform === 'win32' ? [MSYS2_BASE, ...MINGW_PACKAGES].map((p) => ({ ...p, urls: [...p.urls] })) : []
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

/**
 * What the bundle copies: the repository, the dependency packages (unless left out), and the staged
 * licences.
 */
export function bundleCopies(o: { engineDir: string; engineFiles: string[]; archives: DependencyArchive[]; withDeps: boolean; licensesDir: string; licenseFiles: string[] }): StageCopy[] {
  const out: StageCopy[] = []
  for (const rel of o.engineFiles) out.push({ from: nativePath(o.engineDir, rel), to: `engine/${rel}` })
  if (o.withDeps) for (const a of o.archives) out.push({ from: a.path, to: `dependencies/${a.name}/${a.file}` })
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
  /** The binaries of OrcaSlicer's tree the archive leaves out (orcaArchiveExcludes, as git pathspecs). */
  orcaExcluded: string[]
  /** Whether every binary the engine and the archive carry has its source and recipe here; `incomplete` says what has not. */
  complete: boolean
  incomplete: string[]
  dependencies: {
    included: boolean
    archives: { name: string; file: string; url: string; sha256: string; size: number }[]
    inTree: string[]
    missing: string[]
  }
  /**
   * Build tools the pipeline fetches itself, pinned: on Windows the MSYS2 packages of the MinGW-w64
   * toolchain that builds GMP and MPFR (mingwToolchain); none elsewhere.
   */
  toolchain: { mingw: MingwPackage[] }
  inputs: string
  createdAt: string
}

/**
 * What a bundle lacks of complete corresponding source. `missing`: sources it cannot have (a built
 * dependency with no pinned package, a file of the engine that is a binary of Orca's tree, a binary
 * the Orca archive keeps); `source` stops for them unless --allow-missing. `omitted`: the packages
 * --no-deps leaves out by choice; the bundle is then not complete either.
 */
export function sourceGaps(o: { plan: DependencyPlan; withDeps: boolean; archivedBinaries?: string[]; shippedTreeBinaries?: string[] }): { missing: string[]; omitted: string[] } {
  const { plan } = o
  const kept = o.archivedBinaries ?? []
  const shipped = o.shippedTreeBinaries ?? []
  const missing = [
    ...(plan.missing.length > 0 ? [`no pinned source package for the built dependencies ${plan.missing.join(', ')}; fetch them with build`] : []),
    ...(shipped.length > 0 ? [`the engine ships binaries of OrcaSlicer's tree, whose source the bundle does not have: ${shipped.join(', ')}; build them from their source packages (GMP and MPFR on Windows: -DDEP_MINGW_ROOT, patch 0007)`] : []),
    ...(kept.length > 0 ? [`the Orca archive keeps binaries whose source the bundle does not have: ${kept.join(', ')}; leave them out (orcaArchiveExcludes, scripts/lib/bundle.ts)`] : []),
  ]
  const leftOut = o.withDeps ? [] : [...new Set(plan.archives.map((a) => a.name))].sort()
  const omitted = leftOut.length > 0 ? [`the source packages of ${leftOut.join(', ')} are left out (--no-deps); bundle.json lists each one with its URL and SHA-256`] : []
  return { missing, omitted }
}

/**
 * The build instructions in the bundle; the command lines are the pipeline's own (cmake.ts). It
 * says the bundle is complete corresponding source only when neither `incomplete` nor its own
 * plan and --no-deps (sourceGaps) name a gap.
 */
export function buildingText(o: { lock: OrcaLock; manifest: EngineManifest; patches: string[]; plan: DependencyPlan; withDeps: boolean; orcaExcluded?: string[]; incomplete?: string[] }): string {
  const own = sourceGaps(o)
  const incomplete = [...new Set([...(o.incomplete ?? []), ...own.missing, ...own.omitted])]
  const { lock, manifest: m } = o
  const platform = m.platform
  const win = platform === 'win32'
  const ninja = win ? '<ninja.exe>' : '<ninja>'
  const deps = depsConfigureArgs({ ninja, sourceDir: '<src>/deps', buildDir: '<deps>', downloadDir: '<downloads>', ignorePrefixes: [], platform, ...(win ? { mingwRoot: '<msys64>' } : {}) })
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
  const exe = win ? 'sketch-orca.exe' : 'sketch-orca'
  const mingw = mingwToolchain(platform)
  // The steps of a build from this folder alone; Windows has one more, the MinGW-w64 toolchain.
  const step = { deps: win ? 5 : 4, engine: win ? 6 : 5 }
  const lines = [
    `# Building sketch-orca ${m.engine.version} from this source`,
    '',
    `This folder is the ${incomplete.length === 0 ? 'complete corresponding source' : 'source'} of the sketch-orca engine ${m.engine.version}`,
    `staged for ${platform} (${m.arch}): OrcaSlicer ${lock.tag} (commit ${lock.commit}) with the changes`,
    'in engine/patches, the command-line front end in engine/cli, and the scripts that fetch, build,',
    'test and stage it (engine/scripts). It is offered under the GNU Affero General Public License',
    'version 3; the licence texts are in LICENSES/.',
    '',
    ...(incomplete.length > 0 ? ['It is NOT complete corresponding source:', '', ...incomplete.map((m) => `- ${m}`), ''] : []),
    ...((o.orcaExcluded ?? []).length > 0
      ? [
          `The OrcaSlicer archive leaves out binaries of its tree that this build does not use: ${(o.orcaExcluded ?? []).join(', ')}.`,
          '',
        ]
      : []),
    '## Tools',
    '',
    'The engine has been built on Windows x64 with these; other versions may work, these are known to:',
    '',
    `- Bun ${BUILT_WITH.bun}, which runs the pipeline scripts, and git.`,
    `- ${BUILT_WITH.visualStudio} with the MSVC x64 tools ${BUILT_WITH.msvc} (toolset ${BUILT_WITH.msvcToolset}) and the Windows SDK ${BUILT_WITH.windowsSdk}.`,
    `- CMake ${BUILT_WITH.cmake} and Ninja ${BUILT_WITH.ninja} (the copies Strawberry Perl ships in its c\\bin; the CMake files ask for CMake 3.13 or later).`,
    `- Strawberry Perl ${BUILT_WITH.strawberryPerl} (64-bit), for OpenSSL's build.`,
    `- ${BUILT_WITH.mingw}, which builds GMP and MPFR on Windows (patch 0007); the pipeline fetches its pinned packages itself${win ? ' (see "MinGW-w64" below)' : ''}.`,
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
    ...(win
      ? [
          '4. Unpack the MinGW-w64 toolchain (see "MinGW-w64" below) into a folder; call the msys64 folder',
          '   it makes <msys64>.',
        ]
      : []),
    `${step.deps}. Build the dependencies (on Windows, in a Visual Studio x64 developer prompt, with Strawberry Perl's perl first on PATH and its C compiler folder off PATH):`,
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
    ...(win
      ? [
          '   -DDEP_MSVC_STATIC_RUNTIME=ON (patch 0005) builds every dependency with MSVC\'s static',
          `   runtime library (/MT), as the engine is built in step ${step.engine}, so that no Visual C++ runtime DLL`,
          '   ships with it.',
          '   -DDEP_MINGW_ROOT=<msys64> (patch 0007) builds GMP and MPFR from their source packages in',
          '   dependencies/ with that toolchain\'s GCC, as DLLs, and runs their test suites; without it',
          '   Orca\'s build would copy the prebuilt DLLs of its tree, whose source this folder does not',
          '   hold (the OrcaSlicer archive leaves them out).',
        ]
      : []),
    '',
    `${step.engine}. Build the engine:`,
    '',
    '```',
    formatCommand('cmake', orca),
    formatCommand('cmake', orcaBuildArgs('<build>', 8)),
    '```',
    '',
    ...(win
      ? [
          'CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded, with policy CMP0091 NEW, is the static runtime for',
          'Orca and the front end; engine/cli/CMakeLists.txt refuses to configure with any other.',
          '',
        ]
      : []),
    `The engine is <build>/sketch-orca/${exe}. \`${exe} version\` prints the Orca commit it was built from.`,
    '',
    ...(mingw.length > 0
      ? [
          '## MinGW-w64',
          '',
          'GMP and MPFR are built with GCC, not MSVC: Orca\'s deps/MinGW/build.sh (patch 0007) runs their',
          'configure scripts and makefiles with the bash, make and m4 of an MSYS2 tree and the GCC of its',
          'mingw64 environment. The pipeline unpacks these packages, each checked against its size and',
          'SHA-256, into <work>/mingw/msys64 (engine/scripts/lib/mingw.ts): the first, which holds',
          'msys64/, then each of the others into that msys64 folder, without pacman\'s records (.PKGINFO,',
          '.BUILDINFO, .MTREE, .INSTALL), with Windows\' own tar.exe. Nothing is installed.',
          '',
          ...mingw.map((p) => `- ${p.file} (${p.size} bytes, SHA-256 ${p.sha256}): ${p.urls.join(' or ')}`),
          '',
        ]
      : []),
    '## Dependencies',
    '',
    ...o.plan.archives.map((a) => `- ${a.name}: ${a.file} from ${a.url} (SHA-256 ${a.sha256})`),
    ...o.plan.inTree.map((n) => `- ${n}: built from its sources inside the OrcaSlicer tree`),
    ...o.plan.missing.map((n) => `- ${n}: built, but its source package is NOT in this bundle`),
    '',
  ]
  return lines.join('\n')
}

/**
 * The bundle's files that privacyProblems reads: all but the packages of others (Orca's archive and
 * dependencies/), which are as their projects publish them, each pinned (the locked commit, the
 * SHA-256 of each package), so that nothing of this build can be in them. The binaries the Orca
 * archive keeps, if any, are read apart (archivedPrivacyProblems).
 */
export function ownBundleFiles(rels: string[]): string[] {
  return rels.filter((rel) => !/^(orca|dependencies)\//.test(rel))
}

/** The bundle's files whose lines marked PRIVACY_FIXTURE_MARK privacyProblems leaves out: the repository's own tests. */
export function bundleFixtureFile(rel: string): boolean {
  return rel.startsWith('engine/test/')
}

/**
 * What the binaries the Orca archive keeps (`files`, their bytes at the locked commit) record of a
 * person or a machine: this build's needles, any Windows user folder, a program database recorded
 * with its folder, any other folder of a machine (privacyProblemsIn). They ship as OrcaSlicer
 * publishes them, so a hit is upstream's: leave the binary out (orcaArchiveExcludes) or allow the
 * folder in ALLOWED_BUILD_PATHS (stage.ts), saying why.
 */
export function archivedPrivacyProblems(label: string, files: Map<string, Uint8Array>, needles: string[]): string[] {
  return [...files].flatMap(([rel, bytes]) => privacyProblemsIn(`${label}: ${rel}`, bytes, [...needles, ...USER_FOLDER_NEEDLES]))
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

export async function runSource(ctx: Context, log: Log): Promise<number> {
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
  // The binaries of Orca's tree: the archive keeps none of them (each one it kept would be named
  // here), and the engine ships none of them (the prebuilt GMP, say, which has no source here).
  const orcaExcluded = orcaArchiveExcludes()
  const tree = capture(git, ['-C', w.src, '-c', 'core.quotePath=false', 'ls-tree', '-r', '-z', '--name-only', ctx.lock.commit])
  if (tree.code !== 0) throw new StepError(`git ls-tree failed in ${w.src}: ${tree.stderr.trim()}`)
  const treeFiles = tree.stdout.split('\0').filter((rel) => rel !== '')
  const kept = archivedBinaries(treeFiles, orcaExcluded)
  const treeBinaries = treeFiles.filter(isBinaryPath).map((rel) => ({ path: rel, sha256: commitFileSha256(git, w.src, ctx.lock.commit, rel) }))
  const shipped = shippedTreeBinaries(m.files, treeBinaries)
  // Nothing the bundle writes or copies from this repository names the builder (a current bundle too), and the binaries it archives name no machine.
  const needles = builderNeedles(ctx, platform, git)
  const keptBytes = new Map(kept.map((rel) => [rel, commitFileBytes(git, w.src, ctx.lock.commit, rel)] as const))
  const archivedPersonal = archivedPrivacyProblems(orcaArchiveName(ctx.lock), keptBytes, needles)
  if (archivedPersonal.length > 0) throw new StepError(`binaries the Orca archive keeps record a person's or a machine's folder: ${archivedPersonal.join('; ')}. They ship as OrcaSlicer publishes them: leave them out (orcaArchiveExcludes, scripts/lib/bundle.ts) or allow the folder in ALLOWED_BUILD_PATHS (scripts/lib/stage.ts), saying why`)
  const repository = repositoryFiles(git, ctx.engine.engine)
  const engineFiles = repository.files
  if (repository.commit === null) log.line(`source: ${ctx.engine.engine} is not a git checkout: every file in it but the local work folders goes in`)
  const gaps = sourceGaps({ plan, withDeps, archivedBinaries: kept, shippedTreeBinaries: shipped })
  const { missing } = gaps
  // What the bundle lacks of complete corresponding source, also what --no-deps leaves out by choice.
  const incomplete = [...gaps.missing, ...gaps.omitted]
  if (missing.length > 0 && !allowMissing) throw new StepError(`${missing.join('; ')}; or pass --allow-missing`)
  const toolchain = { mingw: mingwToolchain(m.platform) }
  const licensesDir = join(w.dist, 'engine', 'LICENSES')
  const licenseFiles = listFiles(licensesDir)
  const copies = bundleCopies({ engineDir: ctx.engine.engine, engineFiles, archives: plan.archives, withDeps, licensesDir, licenseFiles })
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
      orcaExcluded,
      toolchain,
      incomplete,
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
        const personal = problems.length === 0 ? privacyProblems(dir, ownBundleFiles(listFiles(dir)), needles, bundleFixtureFile) : []
        if (problems.length === 0 && personal.length === 0) {
          log.line(`source: ${dir} is current; skipping`)
          return 0
        }
        if (personal.length > 0) log.line(`source: the bundle names the builder or the build machine (${personal.slice(0, 3).join('; ')}); making it again`)
        else log.line(`source: the bundle no longer matches its SHA256SUMS (${problems.slice(0, 3).join('; ')}); making it again`)
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
  await runLogged(git, ['-C', w.src, '-c', 'core.autocrlf=false', '-c', 'core.eol=lf', 'archive', '--format=tar.gz', `--prefix=OrcaSlicer-${ctx.lock.tag}/`, '-o', archive, ctx.lock.commit, '--', ...excludePathspecs(orcaExcluded)], { log })
  log.line(`source: copying ${copies.length} files${withDeps ? ` (${plan.archives.length} dependency packages, linked where the volume allows)` : ''}`)
  for (const c of copies) copyFile(c.from, nativePath(tmp, c.to), { link: c.to.startsWith('dependencies/') })
  const patches = listPatches(ctx.engine.patches)
  writeText(join(tmp, 'BUILDING.md'), buildingText({ lock: ctx.lock, manifest: m, patches: patches.map((p) => p.name), plan, withDeps, orcaExcluded, incomplete }))
  const info: BundleInfo = {
    format: SOURCE_FORMAT,
    sourceVersion: SOURCE_VERSION,
    name,
    engine: { version: m.engine.version, platform: m.platform, arch: m.arch, manifestSha256 },
    orca: { repo: ctx.lock.repo, tag: ctx.lock.tag, version: ctx.lock.version, commit: ctx.lock.commit, license: ctx.lock.license, archive: orcaArchiveName(ctx.lock) },
    patches: patches.map((p) => ({ name: p.name, sha256: p.sha256 })),
    cli: m.cli,
    repository: { commit: repository.commit },
    orcaExcluded,
    complete: incomplete.length === 0,
    incomplete,
    dependencies: {
      included: withDeps,
      archives: plan.archives.map((a) => ({ name: a.name, file: a.file, url: a.url, sha256: a.sha256, size: a.size })),
      inTree: plan.inTree,
      missing: plan.missing,
    },
    toolchain,
    inputs,
    createdAt: new Date().toISOString(),
  }
  writeText(join(tmp, 'bundle.json'), JSON.stringify(info, null, 2) + '\n')
  const personal = privacyProblems(tmp, ownBundleFiles(listFiles(tmp)), needles, bundleFixtureFile)
  if (personal.length > 0) throw new StepError(privacyRefusal('files of the source bundle', personal))
  writeText(join(tmp, 'SHA256SUMS'), sha256Sums(treeEntries(tmp, (rel) => rel !== 'SHA256SUMS')))
  const problems = verifySums(tmp)
  if (problems.length > 0) throw new StepError(`the new bundle does not match its own SHA256SUMS: ${problems.slice(0, 3).join('; ')}`)

  rmSync(dir, { recursive: true, force: true })
  renameSync(tmp, dir)
  const bytes = listFiles(dir).reduce((n, rel) => n + statSync(nativePath(dir, rel)).size, 0)
  writeStamp(w.state, 'source', inputs, { name, bytes })
  log.line(`source: ${dir} (${(bytes / 1048576).toFixed(0)} MB${withDeps ? '' : ', without the dependency packages'})`)
  if (plan.missing.length > 0) log.line(`source: note: no source package for ${plan.missing.join(', ')} (--allow-missing)`)
  if (incomplete.length > 0) log.line(`source: the bundle is NOT complete corresponding source${missing.length > 0 ? ' (--allow-missing)' : ''}: ${incomplete.join('; ')}`)
  return 0
}
