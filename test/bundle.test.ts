// privacy-fixtures: /Users/Builder /home/builder /home/me /build/orca test@example.invalid me@example.org
import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  buildingText,
  BUILT_WITH,
  bundleCopies,
  archivedBinaries,
  archivedPrivacyProblems,
  binaryArch,
  bundleFixtureFile,
  bundleName,
  commitFileSha256,
  ensurePrebuiltSource,
  excludePathspecs,
  gmpHeaderVersion,
  keepEngineFile,
  listDownloads,
  LOCAL_WORK_FOLDERS,
  mpfrHeaderVersion,
  orcaArchiveExcludes,
  orcaArchiveName,
  ownBundleFiles,
  parsePinnedSources,
  pinnedSources,
  planDependencies,
  PREBUILT_SOURCES,
  prebuiltLabel,
  prebuiltLibraries,
  prebuiltLine,
  prebuiltSourcePath,
  repositoryFiles,
  SOURCE_VERSION,
  sourceGaps,
  verifySums,
  withArchivedSources,
  withPrebuiltSources,
  type DependencyArchive,
  type DownloadedFile,
  type Fetcher,
  type PinnedSource,
  type PrebuiltLibrary,
  type PrebuiltSource,
} from '../scripts/lib/bundle.ts'
import { depsBuildArgs, formatCommand, orcaBuildArgs } from '../scripts/lib/cmake.ts'
import { firstExisting, gitCandidates } from '../scripts/lib/env.ts'
import { currentPlatform } from '../scripts/lib/toolchain.ts'
import { listFiles, nativePath, sha256Bytes, sha256Sums, treeEntries, writeText } from '../scripts/lib/files.ts'
import { LOCK_FORMAT, type OrcaLock } from '../scripts/lib/lock.ts'
import { buildManifest, findPersonalPaths, personalPathNeedles, PRIVACY_FIXTURE_MARK, privacyProblems, privacyProblemsIn, withoutFixtures, type EngineManifest } from '../scripts/lib/stage.ts'

const LOCK: OrcaLock = {
  format: LOCK_FORMAT,
  repo: 'OrcaSlicer/OrcaSlicer',
  tag: 'v2.4.2',
  commit: '8500fcdccaa10b5099ac20d252af3a7c560046f1',
  version: '2.4.2',
  publishedAt: '2026-07-07T03:33:49Z',
  license: 'AGPL-3.0',
}
const H = (c: string) => c.repeat(64)

// Excerpts of OrcaSlicer v2.4.2's deps/<Name>/<Name>.cmake.
const BLOSC = [
  '    orcaslicer_add_cmake_project(Blosc',
  '        #URL https://github.com/Blosc/c-blosc/archive/refs/tags/v1.17.0.zip',
  '        #URL_HASH SHA256=7463a1df566704f212263312717ab2c36b45d45cba6cd0dccebf91b2cc4b4da9',
  '        URL https://github.com/tamasmeszaros/c-blosc/archive/refs/heads/v1.17.0_tm.zip',
  '        URL_HASH SHA256=dcb48bf43a672fa3de6a4b1de2c4c238709dad5893d1e097b8374ad84b1fc3b3',
  '        DEPENDS ${ZLIB_PKG}',
  '        # Patching upstream does not work this way with git version 2.28 installed on mac worker',
  '        # PATCH_COMMAND  ${GIT_EXECUTABLE} apply --ignore-space-change --whitespace=fix ${CMAKE_CURRENT_LIST_DIR}/blosc-mods.patch',
  '        CMAKE_ARGS',
].join('\n')
const MPFR = [
  '    ExternalProject_Add(dep_MPFR',
  '        URL https://ftp.gnu.org/gnu/mpfr/mpfr-4.2.2.tar.bz2',
  '            https://www.mpfr.org/mpfr-4.2.2/mpfr-4.2.2.tar.bz2',
  '        URL_HASH SHA256=9ad62c7dc910303cd384ff8f1f4767a655124980bb6d8650fe62c815a231bb7b',
  '        DOWNLOAD_DIR ${DEP_DOWNLOAD_DIR}/MPFR',
].join('\r\n')
const OPENSSL = [
  'ExternalProject_Add(dep_OpenSSL',
  '    #EXCLUDE_FROM_ALL ON',
  '    URL "https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz"',
  '    URL_HASH SHA256=2130E8C2FB3B79D1086186F78E59E8BC8D1A6AEDF17AB3907F4CB9AE20918C41',
  '    # URL "https://github.com/openssl/openssl/archive/refs/tags/openssl-3.1.2.tar.gz"',
  '    # URL_HASH SHA256=8c776993154652d0bb393f506d850b811517c8bd8d24b1008aef57fbe55d3f31',
  '    DOWNLOAD_DIR ${DEP_DOWNLOAD_DIR}/OpenSSL',
].join('\n')
const PNG = [
  '    orcaslicer_add_cmake_project(PNG ',
  '        GIT_REPOSITORY https://github.com/glennrp/libpng.git ',
  '        GIT_TAG v1.6.35',
  '        DEPENDS ${ZLIB_PKG}',
  'else ()',
  '    orcaslicer_add_cmake_project(PNG ',
  '        # GIT_REPOSITORY https://github.com/glennrp/libpng.git ',
  '        # GIT_TAG v1.6.35',
  '        URL https://github.com/glennrp/libpng/archive/refs/tags/v1.6.35.zip',
  '        URL_HASH SHA256=3d22d46c566b1761a0e15ea397589b3a5f36ac09b7c785382e6470156c04247f',
  '        DEPENDS ${ZLIB_PKG}',
].join('\n')
const LIBNOISE = [
  'orcaslicer_add_cmake_project(libnoise',
  '    URL https://github.com/SoftFever/Orca-deps-libnoise/archive/refs/tags/1.0.zip',
  '        URL_HASH SHA256=96ffd6cc47898dd8147aab53d7d1b1911b507d9dbaecd5613ca2649468afd8b6',
].join('\n')

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'orca-bundle-'))
})
afterEach(() => {
  rmSync(dir, { recursive: true, force: true })
})

function put(rel: string, text = rel): void {
  writeText(nativePath(dir, rel), text)
}

function dl(file: string, sha256: string): DownloadedFile {
  return { file, path: join('/downloads', file), sha256, size: 10 }
}

function manifest(over: Partial<{ version: string; arch: string; platform: 'win32' | 'linux' }> = {}): EngineManifest {
  return buildManifest({
    lock: LOCK,
    engineVersion: over.version ?? '0.1.0',
    exeName: over.platform === 'linux' ? 'sketch-orca' : 'sketch-orca.exe',
    platform: over.platform ?? 'win32',
    arch: over.arch ?? 'x64',
    patches: [],
    cli: H('c'),
    runtime: { copy: [], system: [], missing: [], crt: [] },
    smokePassed: true,
    inputs: H('d'),
    files: [{ path: over.platform === 'linux' ? 'bin/sketch-orca' : 'bin/sketch-orca.exe', size: 1, sha256: H('a') }],
    stagedAt: '2026-10-30T12:00:00.000Z',
  })
}

describe('pinned dependency packages', () => {
  test('parsePinnedSources reads URL and URL_HASH, leaves comments and git sources out, and lower-cases the hash', () => {
    expect(parsePinnedSources(BLOSC)).toEqual([
      { urls: ['https://github.com/tamasmeszaros/c-blosc/archive/refs/heads/v1.17.0_tm.zip'], sha256: 'dcb48bf43a672fa3de6a4b1de2c4c238709dad5893d1e097b8374ad84b1fc3b3' },
    ])
    expect(parsePinnedSources(MPFR)).toEqual([
      { urls: ['https://ftp.gnu.org/gnu/mpfr/mpfr-4.2.2.tar.bz2', 'https://www.mpfr.org/mpfr-4.2.2/mpfr-4.2.2.tar.bz2'], sha256: '9ad62c7dc910303cd384ff8f1f4767a655124980bb6d8650fe62c815a231bb7b' },
    ])
    expect(parsePinnedSources(OPENSSL)).toEqual([
      { urls: ['https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz'], sha256: '2130e8c2fb3b79d1086186f78e59e8bc8d1a6aedf17ab3907f4cb9ae20918c41' },
    ])
    expect(parsePinnedSources(PNG)).toEqual([
      { urls: ['https://github.com/glennrp/libpng/archive/refs/tags/v1.6.35.zip'], sha256: '3d22d46c566b1761a0e15ea397589b3a5f36ac09b7c785382e6470156c04247f' },
    ])
    expect(parsePinnedSources(LIBNOISE)).toHaveLength(1)
    expect(parsePinnedSources('set(_srcdir ${CMAKE_CURRENT_LIST_DIR}/gmp)\nURL_HASH SHA256=' + H('a'))).toEqual([])
  })

  test('pinnedSources: every recipe of deps/, by dependency', () => {
    put('deps/Blosc/Blosc.cmake', BLOSC)
    put('deps/OpenSSL/OpenSSL.cmake', OPENSSL)
    put('deps/OpenSSL/notes.txt', 'URL https://example.invalid/x.tar.gz URL_HASH SHA256=' + H('b'))
    put('deps/EXPAT/EXPAT.cmake', 'orcaslicer_add_cmake_project(EXPAT\n  SOURCE_DIR ${CMAKE_CURRENT_LIST_DIR}/expat\n)\n')
    put('deps/CMakeLists.txt', LIBNOISE)
    const pins = pinnedSources(join(dir, 'deps'))
    expect([...pins.keys()].sort()).toEqual(['Blosc', 'OpenSSL'])
    expect(pins.get('OpenSSL')![0]!.sha256).toBe('2130e8c2fb3b79d1086186f78e59e8bc8d1a6aedf17ab3907f4cb9ae20918c41')
    expect(pinnedSources(join(dir, 'none')).size).toBe(0)
  })

  test('listDownloads hashes each package under its dependency', () => {
    put('downloads/OpenSSL/OpenSSL_1_1_1w.tar.gz', 'tarball')
    put('downloads/PNG/v1.6.35.zip', 'zip')
    put('downloads/PNG/v1.6.34.zip', 'old')
    put('downloads/stray.txt', 'x')
    mkdirSync(join(dir, 'downloads', 'Empty'))
    const d = listDownloads(join(dir, 'downloads'))
    expect([...d.keys()].sort()).toEqual(['Empty', 'OpenSSL', 'PNG'])
    expect(d.get('OpenSSL')).toEqual([{ file: 'OpenSSL_1_1_1w.tar.gz', path: join(dir, 'downloads', 'OpenSSL', 'OpenSSL_1_1_1w.tar.gz'), sha256: sha256Bytes('tarball'), size: 7 }])
    expect(d.get('PNG')!.map((f) => f.file)).toEqual(['v1.6.34.zip', 'v1.6.35.zip'])
    expect(d.get('Empty')).toEqual([])
  })

  test('planDependencies: the pinned package of each built dependency; in-tree, missing and unpinned ones named', () => {
    const pinned = new Map<string, PinnedSource[]>([
      ['MPFR', parsePinnedSources(MPFR)],
      ['OpenSSL', parsePinnedSources(OPENSSL)],
      ['PNG', parsePinnedSources(PNG)],
      ['TBB', [{ urls: ['https://github.com/oneapi-src/oneTBB/archive/refs/tags/v2021.5.0.zip'], sha256: H('7') }]],
      ['libnoise', parsePinnedSources(LIBNOISE)],
    ])
    const downloads = new Map<string, DownloadedFile[]>([
      ['MPFR', [dl('mpfr-4.2.2.tar.bz2', '9ad62c7dc910303cd384ff8f1f4767a655124980bb6d8650fe62c815a231bb7b')]],
      ['OpenSSL', [dl('OpenSSL_1_1_1w.tar.gz', '2130e8c2fb3b79d1086186f78e59e8bc8d1a6aedf17ab3907f4cb9ae20918c41')]],
      ['PNG', [dl('v1.6.34.zip', H('0')), dl('v1.6.35.zip', '3d22d46c566b1761a0e15ea397589b3a5f36ac09b7c785382e6470156c04247f')]],
      ['TBB', [dl('v2021.4.0.zip', H('8'))]],
      ['libnoise', [dl('1.0.zip', '96ffd6cc47898dd8147aab53d7d1b1911b507d9dbaecd5613ca2649468afd8b6')]],
    ])
    const plan = planDependencies(['PNG', 'OpenSSL', 'EXPAT', 'libnoise', 'MPFR', 'TBB', 'Mystery'], pinned, downloads, (n) => n === 'EXPAT')
    expect(plan.archives.map((a) => [a.name, a.file, a.url])).toEqual([
      ['MPFR', 'mpfr-4.2.2.tar.bz2', 'https://ftp.gnu.org/gnu/mpfr/mpfr-4.2.2.tar.bz2'],
      ['OpenSSL', 'OpenSSL_1_1_1w.tar.gz', 'https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz'],
      ['PNG', 'v1.6.35.zip', 'https://github.com/glennrp/libpng/archive/refs/tags/v1.6.35.zip'],
      ['libnoise', '1.0.zip', 'https://github.com/SoftFever/Orca-deps-libnoise/archive/refs/tags/1.0.zip'],
    ])
    expect(plan.archives[0]).toEqual({ name: 'MPFR', file: 'mpfr-4.2.2.tar.bz2', path: join('/downloads', 'mpfr-4.2.2.tar.bz2'), url: 'https://ftp.gnu.org/gnu/mpfr/mpfr-4.2.2.tar.bz2', sha256: '9ad62c7dc910303cd384ff8f1f4767a655124980bb6d8650fe62c815a231bb7b', size: 10 })
    expect(plan.inTree).toEqual(['EXPAT'])
    expect(plan.missing).toEqual(['Mystery', 'TBB'])
    expect(plan.unpinned).toEqual(['PNG/v1.6.34.zip', 'TBB/v2021.4.0.zip'])
  })

  test('planDependencies takes the URL that names the file, query aside, else the first', () => {
    const pins = new Map<string, PinnedSource[]>([['Boost', [{ urls: ['https://a.example/boost_1_84_0.tar.gz', 'https://b.example/dl/boost-1.84.0.tar.gz?raw=1'], sha256: H('5') }]]])
    const named = planDependencies(['Boost'], pins, new Map([['Boost', [dl('boost-1.84.0.tar.gz', H('5'))]]]), () => false)
    expect(named.archives[0]!.url).toBe('https://b.example/dl/boost-1.84.0.tar.gz?raw=1')
    const renamed = planDependencies(['Boost'], pins, new Map([['Boost', [dl('boost.tgz', H('5'))]]]), () => false)
    expect(renamed.archives[0]!.url).toBe('https://a.example/boost_1_84_0.tar.gz')
  })
})

describe('prebuilt libraries', () => {
  test('the GMP and MPFR versions their headers give', () => {
    expect(gmpHeaderVersion('#define __GNU_MP_VERSION 5\n#define __GNU_MP_VERSION_MINOR 0\n#define __GNU_MP_VERSION_PATCHLEVEL 1\n')).toBe('5.0.1')
    expect(gmpHeaderVersion('#define __GNU_MP_VERSION 5\n')).toBeUndefined()
    expect(mpfrHeaderVersion('#define MPFR_VERSION_MAJOR 3\n#define MPFR_VERSION_STRING "3.0.0"\n')).toBe('3.0.0')
    expect(mpfrHeaderVersion('')).toBeUndefined()
  })

  test('prebuiltLibraries: the ones the staged engine ships, from Orca\'s tree', () => {
    put('src/deps/GMP/gmp/include/gmp.h', '#define __GNU_MP_VERSION 5\n#define __GNU_MP_VERSION_MINOR 0\n#define __GNU_MP_VERSION_PATCHLEVEL 1\n')
    put('src/deps/MPFR/mpfr/include/mpfr.h', '#define MPFR_VERSION_STRING "3.0.0"\n')
    const src = join(dir, 'src')
    expect(prebuiltLibraries(src, ['libgmp-10.dll', 'libmpfr-4.dll', 'TKernel.dll'])).toEqual([
      { name: 'GMP', version: '5.0.1', dir: 'deps/GMP/gmp' },
      { name: 'MPFR', version: '3.0.0', dir: 'deps/MPFR/mpfr' },
    ])
    expect(prebuiltLibraries(src, ['TKernel.dll'])).toEqual([])
    expect(prebuiltLibraries(join(dir, 'none'), ['libgmp-10.dll'])).toEqual([{ name: 'GMP', version: 'unknown', dir: 'deps/GMP/gmp' }])
  })
})

describe('the bundle', () => {
  test('names', () => {
    expect(bundleName(manifest())).toBe('sketch-orca-0.1.0-orca-2.4.2-win32-x64-source')
    expect(bundleName(manifest({ version: '', arch: '', platform: 'linux' }))).toBe('sketch-orca-0-orca-2.4.2-linux-unknown-source')
    expect(orcaArchiveName(LOCK)).toBe('orca/OrcaSlicer-v2.4.2.tar.gz')
  })

  test('keepEngineFile leaves out git, node_modules, the local work folders, logs and editor litter', () => {
    for (const p of ['orca.lock.json', 'scripts/orca.ts', 'cli/main.cpp', 'test/fixtures/github/release-latest.json', 'package.json', 'bun.lock', '.gitattributes', '.gitignore', 'LICENSE', 'protocol/schema/job.schema.json', 'scripts/lib/work.ts', 'test/dist.test.ts'])
      expect(keepEngineFile(p)).toBe(true)
    for (const p of ['node_modules', 'node_modules/x/index.js', 'scripts/node_modules/y.js', '.git', '.git/HEAD', 'cli/.DS_Store', 'Thumbs.db', 'README.md~', 'work', 'work/src/CMakeLists.txt', 'dist/engine/manifest.json', 'logs', 'build.log'])
      expect(keepEngineFile(p)).toBe(false)
  })

  test('keepEngineFile leaves out what .gitignore names', () => {
    const ignore = readFileSync(join(import.meta.dir, '..', '.gitignore'), 'utf8')
    const patterns = ignore.split('\n').filter((l) => l.trim() !== '' && !l.startsWith('#'))
    expect(patterns).toEqual(['node_modules/', ...LOCAL_WORK_FOLDERS.map((f) => `/${f}/`), '*.log', '.DS_Store', 'Thumbs.db', '*~'])
  })

  test('bundleCopies: engine/, dependencies/ and LICENSES/, sorted; without packages; two files on one path refused', () => {
    const archives: DependencyArchive[] = [
      { name: 'PNG', file: 'v1.6.35.zip', path: join(dir, 'downloads', 'PNG', 'v1.6.35.zip'), url: 'u', sha256: H('1'), size: 1 },
      { name: 'OpenSSL', file: 'OpenSSL_1_1_1w.tar.gz', path: join(dir, 'downloads', 'OpenSSL', 'OpenSSL_1_1_1w.tar.gz'), url: 'u', sha256: H('2'), size: 1 },
    ]
    const base = {
      engineDir: join(dir, 'engine'),
      engineFiles: ['orca.lock.json', 'patches/0001-deps-without-gui.patch', 'cli/main.cpp'],
      archives,
      prebuiltArchives: [{ name: 'GMP', file: 'gmp-5.0.1.tar.bz2', path: join(dir, 'prebuilt', 'GMP', 'gmp-5.0.1.tar.bz2') }],
      withDeps: true,
      licensesDir: join(dir, 'LICENSES'),
      licenseFiles: ['README.txt', 'OrcaSlicer/LICENSE.txt'],
    }
    const copies = bundleCopies(base)
    expect(copies.map((c) => c.to)).toEqual([
      'LICENSES/OrcaSlicer/LICENSE.txt',
      'LICENSES/README.txt',
      'dependencies/OpenSSL/OpenSSL_1_1_1w.tar.gz',
      'dependencies/PNG/v1.6.35.zip',
      'engine/cli/main.cpp',
      'engine/orca.lock.json',
      'engine/patches/0001-deps-without-gui.patch',
      'prebuilt/GMP/gmp-5.0.1.tar.bz2',
    ])
    expect(copies.find((c) => c.to === 'engine/cli/main.cpp')!.from).toBe(join(dir, 'engine', 'cli', 'main.cpp'))
    expect(copies.find((c) => c.to === 'dependencies/PNG/v1.6.35.zip')!.from).toBe(archives[0]!.path)
    expect(copies.find((c) => c.to === 'prebuilt/GMP/gmp-5.0.1.tar.bz2')!.from).toBe(join(dir, 'prebuilt', 'GMP', 'gmp-5.0.1.tar.bz2'))
    expect(bundleCopies({ ...base, withDeps: false }).some((c) => c.to.startsWith('dependencies/') || c.to.startsWith('prebuilt/'))).toBe(false)
    expect(() => bundleCopies({ ...base, licenseFiles: ['a.txt', 'A.TXT'] })).toThrow(/two files would land at LICENSES\/A\.TXT/)
  })

  test('BUILDING.md: the patches in order, the pipeline\'s own command lines, and every dependency accounted for', () => {
    const plan = {
      archives: [{ name: 'OpenSSL', file: 'OpenSSL_1_1_1w.tar.gz', path: 'p', url: 'https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz', sha256: H('2'), size: 1 }],
      inTree: ['EXPAT'],
      missing: ['TBB'],
      unpinned: [],
    }
    const prebuilt = [{ name: 'GMP', version: '5.0.1', dir: 'deps/GMP/gmp' }]
    const patches = ['0001-deps-without-gui.patch', '0002-root-cmake-without-gui-packages.patch', '0003-src-cmake-sketch-orca-front-end.patch']
    const text = buildingText({ lock: LOCK, manifest: manifest(), patches, plan, prebuilt, withDeps: true })
    const lines = text.split('\n')
    // Applied in <src> with autocrlf off: Git for Windows' default would write the patched files with CRLF.
    const at = (p: string) => lines.indexOf(`   - git -c core.autocrlf=false apply <bundle>/engine/patches/${p}`)
    expect(at(patches[0]!)).toBeGreaterThan(0)
    expect(at(patches[1]!)).toBe(at(patches[0]!) + 1)
    expect(at(patches[2]!)).toBe(at(patches[0]!) + 2)
    expect(lines[at(patches[0]!) - 3]).toStartWith('2. In <src>, apply the patches in this order.')
    // Inside a git work tree, git apply skips the patches silently and Orca's deps build changes its patch paths.
    expect(text).toContain('Work outside any git work tree')
    expect(text).toContain('GIT_CEILING_DIRECTORIES')
    expect(text).toContain('IPP-ICV')
    expect(text).toContain('CL=/d1trimfile:"<folder>\\\\"')
    expect(text).toContain('1. Unpack orca/OrcaSlicer-v2.4.2.tar.gz')
    expect(text).toContain('3. Copy dependencies/ to a folder <downloads>.')
    expect(lines).toContain(formatCommand('cmake', depsBuildArgs('<deps>', 4)))
    expect(lines).toContain(formatCommand('cmake', orcaBuildArgs('<build>', 8)))
    const depsConfigure = lines.find((l) => l.startsWith('cmake -S <src>/deps '))!
    expect(depsConfigure).toContain('-DCMAKE_MAKE_PROGRAM=<ninja.exe>')
    expect(depsConfigure).toContain('-DDEP_DOWNLOAD_DIR=<downloads>')
    expect(depsConfigure).toContain('-DDEP_OPENCV_IPP=OFF')
    // Windows: the static runtime for the dependencies (patch 0005) and the engine, said in words too.
    expect(depsConfigure).toContain('-DDEP_MSVC_STATIC_RUNTIME=ON')
    expect(text).toContain("-DDEP_MSVC_STATIC_RUNTIME=ON (patch 0005) builds every dependency with MSVC's static")
    expect(text).toContain('engine/cli/CMakeLists.txt refuses to configure with any other.')
    const orcaConfigure = lines.find((l) => l.startsWith('cmake -S <src> '))!
    expect(orcaConfigure).toContain('-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded')
    expect(orcaConfigure).toContain('-DCMAKE_POLICY_DEFAULT_CMP0091=NEW')
    expect(orcaConfigure).toContain('-DSKETCH_ORCA_CLI_DIR=<bundle>/engine/cli')
    expect(orcaConfigure).toContain(`-DSKETCH_ORCA_COMMIT=${LOCK.commit}`)
    expect(orcaConfigure).toContain('-DSKETCH_ORCA_TAG=v2.4.2')
    expect(text).toContain('The engine is <build>/sketch-orca/sketch-orca.exe.')
    expect(lines).toContain(`- OpenSSL: OpenSSL_1_1_1w.tar.gz from https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz (SHA-256 ${H('2')})`)
    expect(lines).toContain('- EXPAT: built from its sources inside the OrcaSlicer tree')
    expect(lines.some((l) => l.startsWith('- GMP 5.0.1: OrcaSlicer ships it prebuilt for Windows in deps/GMP/gmp') && l.includes('Its source is NOT in this bundle'))).toBe(true)
    // The toolchain it has been built with, by version.
    for (const v of Object.values(BUILT_WITH)) expect(text).toContain(v)
    expect(text).toContain(`- Bun ${BUILT_WITH.bun}, which runs the pipeline scripts, and git.`)
    expect(text).toContain('(the copies Strawberry Perl ships in its c\\bin;')
    expect(lines).toContain('- TBB: built, but its source package is NOT in this bundle')
    expect(text.endsWith('\n')).toBe(true)
    // TBB has no package and GMP no source: the text says so itself, whatever the caller passes.
    expect(text).toContain('This folder is the source of the sketch-orca engine')
    expect(text).toContain('It is NOT complete corresponding source:')
    expect(lines).toContain('- no pinned source package for the built dependencies TBB; fetch them with build')
    const whole = buildingText({ lock: LOCK, manifest: manifest(), patches, plan: { ...plan, missing: [] }, prebuilt: [], withDeps: true })
    expect(whole).toContain('This folder is the complete corresponding source of the sketch-orca engine')
    expect(whole).not.toContain('NOT complete')
    // A bundle made with --allow-missing says so, and what the Orca archive leaves out.
    const partial = buildingText({ lock: LOCK, manifest: manifest(), patches, plan, prebuilt, withDeps: true, orcaExcluded: ['deps/WebView2', 'tools/*.exe'], incomplete: ['no pinned source for the prebuilt MPFR 3.1.6'] })
    expect(partial).toContain('This folder is the source of the sketch-orca engine')
    expect(partial).not.toContain('complete corresponding source of')
    expect(partial.split('\n')).toContain('- no pinned source for the prebuilt MPFR 3.1.6')
    expect(partial).toContain('The OrcaSlicer archive leaves out binaries of its tree that this build does not use: deps/WebView2, tools/*.exe.')

    const linux = buildingText({ lock: LOCK, manifest: manifest({ platform: 'linux' }), patches, plan: { ...plan, missing: [] }, prebuilt: [], withDeps: false })
    expect(linux).toContain('The dependency packages were left out of this bundle')
    expect(linux).toContain('-DCMAKE_MAKE_PROGRAM=<ninja>')
    expect(linux).not.toContain('-DCMAKE_CXX_COMPILER=cl')
    expect(linux).not.toContain('DEP_MSVC_STATIC_RUNTIME')
    expect(linux).not.toContain('CMAKE_MSVC_RUNTIME_LIBRARY')
    expect(linux).toContain('The engine is <build>/sketch-orca/sketch-orca.')
    expect(linux).not.toContain('NOT in this bundle')
    // --no-deps leaves the packages out: never complete corresponding source.
    expect(linux).not.toContain('complete corresponding source of')
    expect(linux.split('\n')).toContain('- the source packages of OpenSSL are left out (--no-deps); bundle.json lists each one with its URL and SHA-256')
  })

  test('sourceGaps: what stops source unless --allow-missing, apart from what --no-deps leaves out', () => {
    const archive = { name: 'OpenSSL', file: 'o.tar.gz', path: 'p', url: 'https://example.invalid/o.tar.gz', sha256: H('2'), size: 1 }
    const plan = { archives: [archive], inTree: ['EXPAT'], missing: [], unpinned: [] }
    const source = { file: 'gmp-5.0.1.tar.bz2', urls: ['https://example.invalid/gmp-5.0.1.tar.bz2'], size: 1, sha256: H('3'), provenance: 'a test' }
    const gmp: PrebuiltLibrary = { name: 'GMP', version: '5.0.1', dir: 'deps/GMP/gmp', source: { ...source, recipe: 'prebuilt/gmp.sh' }, archived: [{ path: 'deps/GMP/gmp/lib/win-x64/libgmp-10.dll', sha256: H('4'), sourced: true }] }
    expect(sourceGaps({ plan, prebuilt: [gmp], withDeps: true })).toEqual({ missing: [], omitted: [] })
    // The audit's case: --no-deps with nothing else missing still leaves out what complete source needs.
    expect(sourceGaps({ plan, prebuilt: [gmp], withDeps: false })).toEqual({ missing: [], omitted: ['the source packages of GMP, OpenSSL are left out (--no-deps); bundle.json lists each one with its URL and SHA-256'] })
    expect(sourceGaps({ plan: { ...plan, archives: [] }, prebuilt: [], withDeps: false })).toEqual({ missing: [], omitted: [] })
    const gaps = sourceGaps({
      plan: { ...plan, missing: ['TBB'] },
      prebuilt: [
        { ...gmp, source, archived: [{ ...gmp.archived![0]!, sourced: false }] },
        { name: 'MPFR', version: '3.0.0', dir: 'deps/MPFR/mpfr', archived: [{ path: 'deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll', sha256: H('5'), sourced: false }] },
      ],
      withDeps: true,
      unknownBinaries: ['tools/new.exe'],
    })
    expect(gaps.omitted).toEqual([])
    expect(gaps.missing).toEqual([
      'no pinned source package for the built dependencies TBB; fetch them with build',
      'no pinned source for the prebuilt MPFR 3.0.0, whose binaries the Orca archive keeps (deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll); PREBUILT_SOURCES (scripts/lib/bundle.ts) pins the binaries whose source is known',
      `the Orca archive keeps deps/GMP/gmp/lib/win-x64/libgmp-10.dll (SHA-256 ${H('4')}), which no pin of the source the bundle carries lists by path and SHA-256 (PREBUILT_SOURCES archived)`,
      'no recipe that rebuilds the prebuilt GMP 5.0.1 from its pinned source (how it was configured and built is not known)',
      'the Orca archive keeps binaries whose source the bundle does not have: tools/new.exe; leave them out (orcaArchiveExcludes, scripts/lib/bundle.ts) or pin their source',
    ])
  })

  test('verifySums: changed, missing and unlisted files', () => {
    put('b/BUILDING.md', 'text')
    put('b/orca/OrcaSlicer-v2.4.2.tar.gz', 'tar')
    put('b/engine/orca.lock.json', '{}')
    const b = join(dir, 'b')
    expect(verifySums(b)).toEqual(['no SHA256SUMS'])
    put('b/SHA256SUMS', sha256Sums(treeEntries(b, (rel) => rel !== 'SHA256SUMS')))
    expect(verifySums(b)).toEqual([])
    put('b/BUILDING.md', 'edited')
    put('b/extra.txt', 'x')
    rmSync(join(b, 'engine', 'orca.lock.json'))
    expect(verifySums(b).sort()).toEqual(['changed BUILDING.md', 'missing engine/orca.lock.json', 'unlisted file extra.txt'])
    put('b/SHA256SUMS', 'not a sums file\n')
    expect(verifySums(b)).toEqual([expect.stringMatching(/not a sha256sum line/)])
  })

  test("the privacy scan reads the bundle's own files, not the packages of others", () => {
    put('b/BUILDING.md', 'built in D:\\Build-Cache\\orca')
    put('b/bundle.json', '{}')
    put('b/engine/cli/run.cpp', '// C:/Users/Builder/sketch-orca/cli/run.cpp')
    put('b/LICENSES/NOTICE.txt', 'text')
    put('b/orca/OrcaSlicer-v2.4.2.tar.gz', 'D:\\build-cache\\orca')
    put('b/dependencies/Boost/boost.tar.gz', 'D:\\build-cache\\orca')
    put('b/prebuilt/GMP/gmp-5.0.1.tar.bz2', 'D:\\build-cache\\orca')
    const b = join(dir, 'b')
    const own = ownBundleFiles(listFiles(b))
    expect(own).toEqual(['BUILDING.md', 'LICENSES/NOTICE.txt', 'bundle.json', 'engine/cli/run.cpp'])
    expect(privacyProblems(b, own, personalPathNeedles('C:\\Users\\Builder', 'Builder', ['D:\\build-cache\\orca']))).toEqual([
      'BUILDING.md (d:\\build-cache\\orca)',
      'engine/cli/run.cpp (/users/builder, c:/users/builder)',
    ])
  })
})

describe('prebuilt libraries and their sources', () => {
  const GMP_PIN = PREBUILT_SOURCES.find((p) => p.name === 'GMP')!
  const gmp = (): PrebuiltLibrary => ({ name: 'GMP', version: '5.0.1', dir: 'deps/GMP/gmp' })
  const shipping = (sha256: string, arch = 'x64') => ({
    arch,
    files: [
      { path: 'bin/libgmp-10.dll', size: 543027, sha256 },
      { path: 'bin/sketch-orca.exe', size: 1, sha256: H('a') },
    ],
  })

  test("the GMP pin: OrcaSlicer's prebuilt Windows x64 DLL and the GMP 5.0.1 release archive", () => {
    expect(GMP_PIN).toMatchObject({ version: '5.0.1', binary: 'libgmp-10.dll', arch: 'x64', file: 'gmp-5.0.1.tar.bz2', size: 2006109 })
    expect(GMP_PIN.binarySha256).toBe('fec43cb18ce0ec8a5dd6ad1db745747167310ccb92b5acff0d445a8b3013f009')
    expect(GMP_PIN.sha256).toBe('a2a610f01fd3298dc08c87bf30498c2402590e1bcb227fc40b15ee6d280939fb')
    expect(GMP_PIN.urls).toEqual(['https://gmplib.org/download/gmp/gmp-5.0.1.tar.bz2', 'https://ftp.gnu.org/gnu/gmp/gmp-5.0.1.tar.bz2'])
    for (const p of PREBUILT_SOURCES) {
      expect(p.binarySha256).toMatch(/^[0-9a-f]{64}$/)
      expect(p.sha256).toMatch(/^[0-9a-f]{64}$/)
      for (const u of p.urls) {
        expect(u).toStartWith('https://')
        expect(u.endsWith(`/${p.file}`)).toBe(true)
      }
      // The DLL and its import library, in Orca's folder for the pin's architecture; the DLL is the binary the engine would ship.
      expect(p.archived.map((a) => a.path.split('/').pop())).toEqual([p.binary, p.binary.replace(/\.dll$/, '.lib')])
      for (const a of p.archived) {
        expect(binaryArch(a.path)).toBe(p.arch)
        expect(a.sha256).toMatch(/^[0-9a-f]{64}$/)
      }
      expect(p.archived[0]!.sha256).toBe(p.binarySha256)
    }
    expect(GMP_PIN.archived).toEqual([
      { path: 'deps/GMP/gmp/lib/win-x64/libgmp-10.dll', sha256: 'fec43cb18ce0ec8a5dd6ad1db745747167310ccb92b5acff0d445a8b3013f009' },
      { path: 'deps/GMP/gmp/lib/win-x64/libgmp-10.lib', sha256: '63fbb59cd3b7879f71cf2d3d9133f6edcd6d6afb122a1fff085afb7c2f486f0f' },
    ])
  })

  test("the MPFR pin: OrcaSlicer's prebuilt Windows x64 DLL and import library, and the MPFR 3.0.0 release archive", () => {
    const pin = PREBUILT_SOURCES.find((p) => p.name === 'MPFR')!
    expect(pin).toMatchObject({ version: '3.0.0', binary: 'libmpfr-4.dll', arch: 'x64', file: 'mpfr-3.0.0.tar.bz2', size: 1138685 })
    expect(pin.sha256).toBe('8f4e5f9c53536cb798a30455ac429b1f9fc75a0f8af32d6e0ac31ebf1024821f')
    expect(pin.urls).toEqual(['https://www.mpfr.org/mpfr-3.0.0/mpfr-3.0.0.tar.bz2', 'https://ftp.gnu.org/gnu/mpfr/mpfr-3.0.0.tar.bz2'])
    expect(pin.archived).toEqual([
      { path: 'deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll', sha256: '09b3b868b96433991fc15c9c5ae6f9a44c62d2e21194110442607917391ed927' },
      { path: 'deps/MPFR/mpfr/lib/win-x64/libmpfr-4.lib', sha256: 'db89bd46fcbbd99b56197284e5593b3f9ce744ccb8ead9aebee9b714bd2547dc' },
    ])
    // No recipe yet: a bundle that carries it is not complete.
    expect(pin.recipe).toBeUndefined()
  })

  test('withArchivedSources: binaries only the Orca archive keeps (MPFR) match their pin by path, architecture and SHA-256', () => {
    const MPFR_PIN = PREBUILT_SOURCES.find((p) => p.name === 'MPFR')!
    const mpfr = (): PrebuiltLibrary => ({ name: 'MPFR', version: '3.0.0', dir: 'deps/MPFR/mpfr' })
    const kept = MPFR_PIN.archived.map((a) => ({ ...a }))
    // The engine does not ship MPFR: withPrebuiltSources leaves it without a source, the archive's files give it one.
    const [runtimeOnly] = withPrebuiltSources([mpfr()], shipping(GMP_PIN.binarySha256))
    expect(runtimeOnly).toEqual(mpfr())
    const [lib] = withArchivedSources([runtimeOnly!], kept)
    expect(lib!.source).toMatchObject({ file: 'mpfr-3.0.0.tar.bz2', sha256: MPFR_PIN.sha256 })
    expect(lib!.archived).toEqual(kept.map((a) => ({ ...a, sourced: true })))
    // Another DLL at that path, or the pinned bytes for another architecture: no source.
    expect(withArchivedSources([mpfr()], [{ ...kept[0]!, sha256: H('b') }, kept[1]!])[0]!.source).toBeUndefined()
    const arm = kept.map((a) => ({ ...a, path: a.path.replace('win-x64', 'win-arm64') }))
    expect(withArchivedSources([mpfr()], arm)[0]).toEqual({ ...mpfr(), archived: arm.map((a) => ({ ...a, sourced: false })) })
    // GMP, which the engine ships: the pin found by the shipped DLL must also list the import library the archive keeps.
    const gmpKept = GMP_PIN.archived.map((a) => ({ ...a }))
    const [g] = withArchivedSources(withPrebuiltSources([gmp()], shipping(GMP_PIN.binarySha256)), gmpKept)
    expect(g!.archived!.every((a) => a.sourced)).toBe(true)
    const [changedLib] = withArchivedSources(withPrebuiltSources([gmp()], shipping(GMP_PIN.binarySha256)), [gmpKept[0]!, { ...gmpKept[1]!, sha256: H('c') }])
    expect(changedLib!.source).toBeDefined()
    expect(changedLib!.archived!.map((a) => a.sourced)).toEqual([true, false])
    // A shipped GMP with no pin: its archived files are not sourced either, even when a pin lists them.
    const [unpinned] = withArchivedSources(withPrebuiltSources([gmp()], shipping(H('d'))), gmpKept)
    expect(unpinned!.source).toBeUndefined()
    expect(unpinned!.archived!.map((a) => a.sourced)).toEqual([false, false])
    // A library none of whose binaries the archive keeps is left as it is.
    expect(withArchivedSources([mpfr()], gmpKept)).toEqual([mpfr()])
    // BUILDING.md names the files the archive keeps.
    expect(prebuiltLine(lib!, true)).toContain(`The OrcaSlicer archive keeps deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll (SHA-256 ${MPFR_PIN.binarySha256}) and deps/MPFR/mpfr/lib/win-x64/libmpfr-4.lib`)
    expect(prebuiltLine(lib!, true)).toContain('Its source is prebuilt/MPFR/mpfr-3.0.0.tar.bz2, from https://www.mpfr.org/mpfr-3.0.0/mpfr-3.0.0.tar.bz2')
  })

  test('withPrebuiltSources: the source only for the very binary it was pinned for', () => {
    const [pinned] = withPrebuiltSources([gmp()], shipping(GMP_PIN.binarySha256))
    expect(pinned).toEqual({
      ...gmp(),
      binary: 'bin/libgmp-10.dll',
      binarySha256: GMP_PIN.binarySha256,
      source: { file: GMP_PIN.file, urls: [...GMP_PIN.urls], size: GMP_PIN.size, sha256: GMP_PIN.sha256, provenance: GMP_PIN.provenance },
    })
    // Another build of the DLL (Orca's tree changed), another architecture, another version: no source.
    expect(withPrebuiltSources([gmp()], shipping(H('b')))[0]).toEqual({ ...gmp(), binary: 'bin/libgmp-10.dll', binarySha256: H('b') })
    expect(withPrebuiltSources([gmp()], shipping(GMP_PIN.binarySha256, 'arm64'))[0]!.source).toBeUndefined()
    expect(withPrebuiltSources([{ ...gmp(), version: '6.2.1' }], shipping(GMP_PIN.binarySha256))[0]!.source).toBeUndefined()
    // A manifest that ships no GMP binary: the library as it was.
    expect(withPrebuiltSources([gmp()], { arch: 'x64', files: [{ path: 'bin/sketch-orca.exe', size: 1, sha256: H('a') }] })).toEqual([gmp()])
    expect(prebuiltLabel(withPrebuiltSources([gmp()], shipping(H('b')))[0]!)).toBe(`GMP 5.0.1 (bin/libgmp-10.dll, SHA-256 ${H('b')})`)
  })

  test('ensurePrebuiltSource: kept when it matches, else downloaded from the URLs in turn and checked', async () => {
    const bytes = new TextEncoder().encode('gmp source')
    const urls = ['https://a.example/gmp-test.tar.bz2', 'https://b.example/gmp-test.tar.bz2', 'https://c.example/gmp-test.tar.bz2']
    const lib: PrebuiltLibrary = { ...gmp(), source: { file: 'gmp-test.tar.bz2', urls, size: bytes.length, sha256: sha256Bytes(bytes), provenance: 'a test' } }
    const asked: string[] = []
    const fetcher: Fetcher = async (url) => {
      asked.push(url)
      if (url.startsWith('https://a.')) throw new Error('HTTP 404')
      if (url.startsWith('https://b.')) return new TextEncoder().encode('something else')
      return bytes
    }
    const lines: string[] = []
    const root = join(dir, 'prebuilt')
    const path = await ensurePrebuiltSource(lib, root, fetcher, (l) => lines.push(l))
    expect(path).toBe(prebuiltSourcePath(root, 'GMP', 'gmp-test.tar.bz2'))
    expect(path).toBe(join(root, 'GMP', 'gmp-test.tar.bz2'))
    expect(readFileSync(path, 'utf8')).toBe('gmp source')
    expect(existsSync(`${path}.partial`)).toBe(false)
    expect(asked).toEqual(urls)
    // Already there: nothing is downloaded.
    asked.length = 0
    expect(await ensurePrebuiltSource(lib, root, fetcher, () => {})).toBe(path)
    expect(asked).toEqual([])
    // A copy that does not match is replaced.
    writeText(path, 'tampered')
    expect(await ensurePrebuiltSource(lib, root, fetcher, (l) => lines.push(l))).toBe(path)
    expect(readFileSync(path, 'utf8')).toBe('gmp source')
    expect(lines.some((l) => l.includes('is not the pinned gmp-test.tar.bz2'))).toBe(true)
  })

  test('ensurePrebuiltSource: refused when no URL gives the pinned bytes, or nothing is pinned', async () => {
    const lib: PrebuiltLibrary = { ...gmp(), source: { file: 'gmp-test.tar.bz2', urls: ['https://a.example/x', 'https://b.example/x'], size: 3, sha256: H('e'), provenance: 'a test' } }
    const fetcher: Fetcher = async (url) => {
      if (url.startsWith('https://a.')) throw new Error('HTTP 503')
      return new TextEncoder().encode('abc')
    }
    const root = join(dir, 'prebuilt')
    await expect(ensurePrebuiltSource(lib, root, fetcher, () => {})).rejects.toThrow(/https:\/\/a\.example\/x: HTTP 503; https:\/\/b\.example\/x: 3 bytes with SHA-256 [0-9a-f]{64}, not 3 bytes with e{64}/)
    expect(existsSync(prebuiltSourcePath(root, 'GMP', 'gmp-test.tar.bz2'))).toBe(false)
    await expect(ensurePrebuiltSource(gmp(), root, fetcher, () => {})).rejects.toThrow(/no pinned source for the prebuilt GMP 5\.0\.1/)
  })

  test("the bundle's files for a Windows engine that ships the pinned GMP: its source at prebuilt/GMP/", async () => {
    const bytes = new TextEncoder().encode('gmp 5.0.1 source')
    const pins: PrebuiltSource[] = [{ ...GMP_PIN, size: bytes.length, sha256: sha256Bytes(bytes) }]
    const [lib] = withPrebuiltSources([gmp()], shipping(GMP_PIN.binarySha256), pins)
    const path = await ensurePrebuiltSource(lib!, join(dir, 'prebuilt'), async () => bytes, () => {})
    const base = { engineDir: join(dir, 'engine'), engineFiles: ['cli/main.cpp', 'orca.lock.json'], archives: [], withDeps: true, licensesDir: join(dir, 'LICENSES'), licenseFiles: ['deps/GMP/gmp.README'] }
    const files = bundleCopies({ ...base, prebuiltArchives: [{ name: 'GMP', file: lib!.source!.file, path }] }).map((c) => c.to)
    expect(files).toEqual(['LICENSES/deps/GMP/gmp.README', 'engine/cli/main.cpp', 'engine/orca.lock.json', 'prebuilt/GMP/gmp-5.0.1.tar.bz2'])
    // BUILDING.md says where the source is, where it came from and what the binary is.
    const line = prebuiltLine(lib!, true)
    expect(line).toStartWith(`- GMP 5.0.1: OrcaSlicer ships it prebuilt for Windows in deps/GMP/gmp, and the engine ships it as bin/libgmp-10.dll (SHA-256 ${GMP_PIN.binarySha256}); its licence is in LICENSES/deps/GMP.`)
    expect(line).toContain(`Its source is prebuilt/GMP/gmp-5.0.1.tar.bz2, from https://gmplib.org/download/gmp/gmp-5.0.1.tar.bz2 (SHA-256 ${sha256Bytes(bytes)}).`)
    expect(line).toContain("The binary is CGAL's MinGW-w64 build of this release")
    expect(prebuiltLine(lib!, false)).toContain('Its source, gmp-5.0.1.tar.bz2, was left out of this bundle; it is at https://gmplib.org/download/gmp/gmp-5.0.1.tar.bz2')
    expect(prebuiltLine(gmp(), true)).toContain('Its source is NOT in this bundle')
    // The pin has no recipe that rebuilds the DLL: BUILDING.md says the bundle is not complete for it.
    expect(GMP_PIN.recipe).toBeUndefined()
    expect(line).toContain('The bundle has no recipe that rebuilds this binary from that source, so for it the bundle is not complete.')
    const withRecipe = withPrebuiltSources([gmp()], shipping(GMP_PIN.binarySha256), [{ ...pins[0]!, recipe: 'prebuilt/gmp-5.0.1.sh' }])[0]!
    expect(withRecipe.source?.recipe).toBe('prebuilt/gmp-5.0.1.sh')
    expect(prebuiltLine(withRecipe, true)).toContain('engine/prebuilt/gmp-5.0.1.sh rebuilds it from that source.')
    // The archive's binaries are accounted for, and the bundle says whether it is complete: a new bundle version.
    expect(SOURCE_VERSION).toBe(5)
  })

  test("orcaArchiveExcludes and archivedBinaries: the Orca archive keeps only the prebuilt binaries the engine's build uses", () => {
    // OrcaSlicer v2.4.2's binaries.
    const tree = [
      ...['win-arm64', 'win-x64', 'win-x86'].flatMap((a) => [`deps/GMP/gmp/lib/${a}/libgmp-10.dll`, `deps/GMP/gmp/lib/${a}/libgmp-10.lib`, `deps/MPFR/mpfr/lib/${a}/libmpfr-4.dll`, `deps/MPFR/mpfr/lib/${a}/libmpfr-4.lib`]),
      ...['win-arm64', 'win-x64', 'win-x86'].flatMap((a) => [`deps/WebView2/lib/${a}/WebView2Loader.dll`, `deps/WebView2/lib/${a}/WebView2LoaderStatic.lib`]),
      'tools/7z.exe',
      'tools/msgfmt.exe',
      'tools/msgmerge.exe',
      'tools/xgettext.exe',
      'tools/README.md',
      'deps/GMP/gmp/include/gmp.h',
      'src/libslic3r/Print.cpp',
    ]
    const win = orcaArchiveExcludes('win32', 'x64')
    expect(win).toEqual(['deps/WebView2', 'tools/*.exe', 'deps/GMP/gmp/lib/win-x86', 'deps/GMP/gmp/lib/win-arm64', 'deps/MPFR/mpfr/lib/win-x86', 'deps/MPFR/mpfr/lib/win-arm64'])
    expect(excludePathspecs(win).slice(0, 3)).toEqual([':(exclude)deps/WebView2', ':(exclude,glob)tools/*.exe', ':(exclude)deps/GMP/gmp/lib/win-x86'])
    const kept = archivedBinaries(tree, win)
    expect(Object.fromEntries(kept.prebuilt)).toEqual({
      GMP: ['deps/GMP/gmp/lib/win-x64/libgmp-10.dll', 'deps/GMP/gmp/lib/win-x64/libgmp-10.lib'],
      MPFR: ['deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll', 'deps/MPFR/mpfr/lib/win-x64/libmpfr-4.lib'],
    })
    expect(kept.unknown).toEqual([])
    // Off Windows the build makes GMP and MPFR from their packages: the archive keeps no binary.
    const linux = archivedBinaries(tree, orcaArchiveExcludes('linux', 'x64'))
    expect(linux).toEqual({ prebuilt: new Map(), unknown: [] })
    // A binary a later Orca release adds, which the archive would carry without its source.
    expect(archivedBinaries([...tree, 'tools/sub/new.exe', 'deps/Foo/foo.dll'], win).unknown).toEqual(['tools/sub/new.exe', 'deps/Foo/foo.dll'])
    // Without the exclusions every one of them would ship.
    expect(archivedBinaries(tree, []).unknown).toHaveLength(10)
  })

  test.skipIf(firstExisting(gitCandidates(currentPlatform(), process.env)) === undefined)('excludePathspecs: git archive leaves out what orcaArchiveExcludes names', () => {
    const git = firstExisting(gitCandidates(currentPlatform(), process.env))!
    const repo = join(dir, 'orca')
    for (const rel of ['deps/GMP/gmp/lib/win-x64/libgmp-10.dll', 'deps/GMP/gmp/lib/win-x86/libgmp-10.dll', 'deps/WebView2/lib/win-x64/WebView2Loader.dll', 'tools/msgfmt.exe', 'tools/README.md', 'src/a.cpp']) put(`orca/${rel}`, 'x')
    const empty = join(dir, 'gitconfig')
    writeFileSync(empty, '')
    const env = { ...process.env, GIT_CONFIG_GLOBAL: empty, GIT_CONFIG_NOSYSTEM: '1', GIT_AUTHOR_NAME: 'test', GIT_AUTHOR_EMAIL: 'test@example.invalid', GIT_COMMITTER_NAME: 'test', GIT_COMMITTER_EMAIL: 'test@example.invalid' }
    const run = (...args: string[]) => {
      const r = spawnSync(git, ['-C', repo, ...args], { encoding: 'utf8', windowsHide: true, env })
      if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`)
      return r.stdout
    }
    run('init', '-q')
    run('add', '-A')
    run('commit', '-q', '-m', 'tree')
    const tar = join(dir, 'orca.tar')
    run('archive', '--format=tar', '-o', tar, 'HEAD', '--', ...excludePathspecs(orcaArchiveExcludes('win32', 'x64')))
    const names = readFileSync(tar, 'latin1').match(/(deps|tools|src)\/[\w./-]+/g) ?? []
    expect([...new Set(names)].filter((n) => !n.endsWith('/')).sort()).toEqual(['deps/GMP/gmp/lib/win-x64/libgmp-10.dll', 'src/a.cpp', 'tools/README.md'])
    // The bytes the archive keeps, from the commit and not from the working tree.
    put('orca/deps/GMP/gmp/lib/win-x64/libgmp-10.dll', 'changed in the working tree')
    expect(commitFileSha256(git, repo, 'HEAD', 'deps/GMP/gmp/lib/win-x64/libgmp-10.dll')).toBe(sha256Bytes('x'))
    expect(() => commitFileSha256(git, repo, 'HEAD', 'deps/none.dll')).toThrow(/git cat-file blob HEAD:deps\/none\.dll failed/)
  })
})

describe("the repository's files in the bundle", () => {
  const git = firstExisting(gitCandidates(currentPlatform(), process.env))

  test('the privacy scan of the whole repository, as engine/, passes for builders named like the made-up paths its tests declare', () => {
    const root = join(import.meta.dir, '..')
    const listed = git && existsSync(join(root, '.git')) ? spawnSync(git, ['-C', root, '-c', 'core.quotePath=false', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], { encoding: 'utf8', windowsHide: true }).stdout.split('\0') : listFiles(root)
    const rels = [...new Set(listed.filter((rel) => rel !== '' && keepEngineFile(rel) && existsSync(join(root, rel))))].sort()
    expect(rels).toContain('test/pe.test.ts')
    // The audit's case (an account named builder), and the names, folders and addresses the tests make up.
    const accounts = ['builder', 'Builder', 'PreviousBuilder', 'me', 'Me', 'John Smith', '\u674e', '\u00c9mile', 'test', 'user', 'runner', 'admin', 'kevin']
    const roots = ['D:\\build-cache\\orca', 'D:\\Build-Cache\\orca', 'C:\\build', '/build/orca']
    const emails = ['test@example.invalid', 'me@example.org']
    const read = (rel: string) => readFileSync(join(root, rel.slice('engine/'.length)))
    const engineRels = rels.map((rel) => `engine/${rel}`)
    for (const user of accounts) {
      for (const home of [`C:\\Users\\${user}`, `/home/${user}`, `/Users/${user}`]) {
        const needles = personalPathNeedles(home, user, roots, emails)
        const problems = engineRels.flatMap((rel) => privacyProblemsIn(rel, bundleFixtureFile(rel) ? withoutFixtures(read(rel)) : read(rel), needles))
        expect({ user, home, problems }).toEqual({ user, home, problems: [] })
      }
    }
    // Without the declarations, the audit's builder trips the tests' fixtures (/home/builder in test/pe.test.ts).
    const plain = engineRels.flatMap((rel) => privacyProblemsIn(rel, read(rel), personalPathNeedles('/home/builder', 'builder')))
    expect(plain).toContain('engine/test/pe.test.ts (/home/builder)')
    // Only the repository's tests declare fixtures.
    expect(bundleFixtureFile('engine/test/pe.test.ts')).toBe(true)
    expect(bundleFixtureFile('engine/scripts/lib/stage.ts')).toBe(false)
    expect(bundleFixtureFile('BUILDING.md')).toBe(false)
    for (const rel of rels.filter((r) => !r.startsWith('test/'))) expect({ rel, declares: readFileSync(join(root, rel), 'latin1').startsWith(PRIVACY_FIXTURE_MARK) }).toEqual({ rel, declares: false })
  })

  test('withoutFixtures: only the strings the first line declares, in any letter case, and that line', () => {
    const text = '// privacy-fixtures: /home/me C:/Users/Builder\nconst a = "/HOME/ME/x" // and C:/users/builder\nconst b = "/home/meg" + "/home/you"\n'
    const out = withoutFixtures(Buffer.from(text)).toString()
    expect(out.length).toBe(text.length)
    expect(out.split('\n')[0]!.trim()).toBe('')
    expect(findPersonalPaths(Buffer.from(out), ['/home/me', 'c:/users/builder', '/home/you'])).toEqual(['/home/you'])
    // /home/meg holds the fixture /home/me: what is left of it, g, names no one.
    expect(out).toContain('g" + "/home/you"')
    // A file that declares nothing is read as it is.
    expect(withoutFixtures(Buffer.from('const a = "/home/me"\n')).toString()).toBe('const a = "/home/me"\n')
    expect(withoutFixtures(Buffer.from('const x = 1\n// privacy-fixtures: /home/me\n"/home/me"')).toString()).toContain('"/home/me"')
  })

  test('archivedPrivacyProblems: a binary the Orca archive keeps that records a folder of a machine or a person is named', () => {
    const lib = Buffer.concat([Buffer.from('!<arch>\n', 'latin1'), Buffer.from('obj D:\\agent\\_work\\1\\s\\gmp\\mul.c and C:\\Users\\Someone\\x', 'latin1')])
    const clean = Buffer.concat([Buffer.from('MZ', 'latin1'), Buffer.alloc(64), Buffer.from('../../mpfr-3.0.0/acos.c /W64_4.4_160271-src/build-gcc http://www.mpfr.org/', 'latin1')])
    const files = new Map<string, Uint8Array>([
      ['deps/GMP/gmp/lib/win-x64/libgmp-10.lib', lib],
      ['deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll', clean],
    ])
    expect(archivedPrivacyProblems('orca/OrcaSlicer-v2.4.2.tar.gz', files, ['/home/me'])).toEqual([
      'orca/OrcaSlicer-v2.4.2.tar.gz: deps/GMP/gmp/lib/win-x64/libgmp-10.lib (:\\users\\)',
      'orca/OrcaSlicer-v2.4.2.tar.gz: deps/GMP/gmp/lib/win-x64/libgmp-10.lib (build path C:\\Users\\Someone\\x)',
      'orca/OrcaSlicer-v2.4.2.tar.gz: deps/GMP/gmp/lib/win-x64/libgmp-10.lib (build path D:\\agent\\_work\\1\\s\\gmp\\mul.c)',
    ])
  })

  /** git in the test's own repository, with no user or system configuration. */
  function inRepo(repo: string, ...args: string[]): string {
    const empty = join(dir, 'gitconfig')
    if (!existsSync(empty)) writeFileSync(empty, '')
    const r = spawnSync(git!, ['-C', repo, ...args], {
      encoding: 'utf8',
      windowsHide: true,
      env: { ...process.env, GIT_CONFIG_GLOBAL: empty, GIT_CONFIG_NOSYSTEM: '1', GIT_AUTHOR_NAME: 'test', GIT_AUTHOR_EMAIL: 'test@example.invalid', GIT_COMMITTER_NAME: 'test', GIT_COMMITTER_EMAIL: 'test@example.invalid' },
    })
    if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`)
    return r.stdout
  }

  test.skipIf(git === undefined)('from a git checkout: exactly the files git tracks, at its commit; a change or a file git does not track is refused', () => {
    const repo = join(dir, 'repo')
    for (const [rel, text] of [['cli/main.cpp', 'int main() {}\n'], ['README.md', 'readme\n'], ['.gitignore', 'node_modules/\n/work/\n*.log\n']] as const) {
      mkdirSync(join(repo, rel, '..'), { recursive: true })
      writeFileSync(join(repo, rel), text)
    }
    inRepo(repo, 'init', '-q')
    inRepo(repo, 'add', '-A')
    inRepo(repo, 'commit', '-q', '-m', 'first')
    const head = inRepo(repo, 'rev-parse', 'HEAD').trim()
    // Ignored files are never listed and never block: the packages, the work folder, logs.
    for (const rel of ['node_modules/x/index.js', 'work/src/a.txt', 'build.log']) {
      mkdirSync(join(repo, rel, '..'), { recursive: true })
      writeFileSync(join(repo, rel), 'local')
    }
    expect(repositoryFiles(git, repo)).toEqual({ files: ['.gitignore', 'README.md', 'cli/main.cpp'], commit: head })

    // A file git does not track (notes, a key) stops the bundle, by name.
    writeFileSync(join(repo, 'notes.txt'), 'private')
    expect(() => repositoryFiles(git, repo)).toThrow('notes.txt')
    rmSync(join(repo, 'notes.txt'))
    // So does a tracked file changed and not committed, and a rename.
    writeFileSync(join(repo, 'README.md'), 'changed\n')
    expect(() => repositoryFiles(git, repo)).toThrow('README.md')
    inRepo(repo, 'checkout', '-q', '--', 'README.md')
    inRepo(repo, 'mv', 'README.md', 'READ.md')
    expect(() => repositoryFiles(git, repo)).toThrow('READ.md')
    inRepo(repo, 'mv', 'READ.md', 'README.md')
    expect(repositoryFiles(git, repo).commit).toBe(head)
  })

  test("outside a checkout (a bundle's own engine/ folder): every file keepEngineFile keeps, and no commit", () => {
    const engine = join(dir, 'engine')
    for (const rel of ['cli/main.cpp', 'README.md', 'node_modules/x.js', 'work/a.txt']) {
      mkdirSync(join(engine, rel, '..'), { recursive: true })
      writeFileSync(join(engine, rel), 'x')
    }
    expect(repositoryFiles(undefined, engine)).toEqual({ files: ['README.md', 'cli/main.cpp'], commit: null })
  })
})
