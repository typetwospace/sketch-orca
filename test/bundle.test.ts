// privacy-fixtures: /Users/Builder /home/builder /home/me /build/orca test@example.invalid me@example.org
import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { gzipSync } from 'node:zlib'
import {
  buildingText,
  BUILT_WITH,
  bundleCopies,
  archivedBinaries,
  archivedPrivacyProblems,
  bundleFixtureFile,
  bundleName,
  BINARIES_NOTICE,
  BINARY_LICENSES,
  commitFileSha256,
  excludePathspecs,
  isBinaryPath,
  keepEngineFile,
  listDownloads,
  LOCAL_WORK_FOLDERS,
  mingwToolchain,
  orcaArchiveExcludes,
  orcaArchiveName,
  ownBundleFiles,
  packageBinaries,
  parsePinnedSources,
  pinnedSources,
  planDependencies,
  repositoryFiles,
  shippedTreeBinaries,
  SOURCE_VERSION,
  sourceGaps,
  verifySums,
  type DependencyArchive,
  type DownloadedFile,
  type PinnedSource,
} from '../scripts/lib/bundle.ts'
import { MINGW_PACKAGES, MSYS2_BASE } from '../scripts/lib/mingw.ts'
import { depsBuildArgs, formatCommand, orcaBuildArgs } from '../scripts/lib/cmake.ts'
import { firstExisting, gitCandidates } from '../scripts/lib/env.ts'
import { currentPlatform } from '../scripts/lib/toolchain.ts'
import { listFiles, nativePath, sha256Bytes, sha256Sums, treeEntries, writeText } from '../scripts/lib/files.ts'
import { LOCK_FORMAT, type OrcaLock } from '../scripts/lib/lock.ts'
import { PACKAGE_BINARIES, type KnownPackageBinary } from '../scripts/lib/packagebins.ts'
import { buildManifest, findPersonalPaths, personalPathNeedles, PRIVACY_FIXTURE_MARK, privacyProblems, privacyProblemsIn, withoutFixtures, type EngineManifest } from '../scripts/lib/stage.ts'
import { makePe } from './support/pe-writer.ts'
import { writeTar } from './support/tar-writer.ts'
import { writeZip } from './support/zip-writer.ts'

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
    deps: { inputs: H('7'), mingw: over.platform === 'linux' ? null : H('8') },
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
    ])
    expect(copies.find((c) => c.to === 'engine/cli/main.cpp')!.from).toBe(join(dir, 'engine', 'cli', 'main.cpp'))
    expect(copies.find((c) => c.to === 'dependencies/PNG/v1.6.35.zip')!.from).toBe(archives[0]!.path)
    expect(bundleCopies({ ...base, withDeps: false }).some((c) => c.to.startsWith('dependencies/'))).toBe(false)
    expect(() => bundleCopies({ ...base, licenseFiles: ['a.txt', 'A.TXT'] })).toThrow(/two files would land at LICENSES\/A\.TXT/)
  })

  test("BUILDING.md: the patches in order, the pipeline's own command lines, the MinGW-w64 toolchain on Windows, and every dependency accounted for", () => {
    const GMP_SHA256 = 'eae9326beb4158c386e39a356818031bd28f3124cf915f8c5b1dc4c7a36b4d7c'
    const plan = {
      archives: [
        { name: 'GMP', file: 'gmp-6.2.1.tar.bz2', path: 'p', url: 'https://github.com/SoftFever/OrcaSlicer_deps/releases/download/gmp-6.2.1/gmp-6.2.1.tar.bz2', sha256: GMP_SHA256, size: 2493916 },
        { name: 'OpenSSL', file: 'OpenSSL_1_1_1w.tar.gz', path: 'p', url: 'https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz', sha256: H('2'), size: 1 },
      ],
      inTree: ['EXPAT'],
      missing: ['TBB'],
      unpinned: [],
    }
    const patches = ['0001-deps-without-gui.patch', '0002-root-cmake-without-gui-packages.patch', '0003-src-cmake-sketch-orca-front-end.patch']
    const text = buildingText({ lock: LOCK, manifest: manifest(), patches, plan, withDeps: true })
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
    // Windows: the MinGW-w64 toolchain is a step of its own, before the dependencies.
    expect(text).toContain('4. Unpack the MinGW-w64 toolchain (see "MinGW-w64" below) into a folder; call the msys64 folder')
    expect(text).toContain('5. Build the dependencies (on Windows, in a Visual Studio x64 developer prompt')
    expect(text).toContain('6. Build the engine:')
    expect(lines).toContain(formatCommand('cmake', depsBuildArgs('<deps>', 4)))
    expect(lines).toContain(formatCommand('cmake', orcaBuildArgs('<build>', 8)))
    const depsConfigure = lines.find((l) => l.startsWith('cmake -S <src>/deps '))!
    expect(depsConfigure).toContain('-DCMAKE_MAKE_PROGRAM=<ninja.exe>')
    expect(depsConfigure).toContain('-DDEP_DOWNLOAD_DIR=<downloads>')
    expect(depsConfigure).toContain('-DDEP_OPENCV_IPP=OFF')
    // Windows: the static runtime for the dependencies (patch 0005) and the engine, said in words too.
    expect(depsConfigure).toContain('-DDEP_MSVC_STATIC_RUNTIME=ON')
    expect(text).toContain("-DDEP_MSVC_STATIC_RUNTIME=ON (patch 0005) builds every dependency with MSVC's static")
    expect(text).toContain('runtime library (/MT), as the engine is built in step 6, so that no Visual C++ runtime DLL')
    expect(text).toContain('engine/cli/CMakeLists.txt refuses to configure with any other.')
    // Windows: GMP and MPFR from their packages with that toolchain (patch 0007), never Orca's prebuilt DLLs.
    expect(depsConfigure).toContain('-DDEP_MINGW_ROOT=<msys64>')
    expect(text).toContain('-DDEP_MINGW_ROOT=<msys64> (patch 0007) builds GMP and MPFR from their source packages in')
    expect(lines).toContain('## MinGW-w64')
    for (const p of [MSYS2_BASE, ...MINGW_PACKAGES]) expect(lines).toContain(`- ${p.file} (${p.size} bytes, SHA-256 ${p.sha256}): ${p.urls.join(' or ')}`)
    expect(text).toContain('.BUILDINFO, .MTREE, .INSTALL), with Windows\' own tar.exe. Nothing is installed.')
    const orcaConfigure = lines.find((l) => l.startsWith('cmake -S <src> '))!
    expect(orcaConfigure).toContain('-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded')
    expect(orcaConfigure).toContain('-DCMAKE_POLICY_DEFAULT_CMP0091=NEW')
    expect(orcaConfigure).toContain('-DSKETCH_ORCA_CLI_DIR=<bundle>/engine/cli')
    expect(orcaConfigure).toContain(`-DSKETCH_ORCA_COMMIT=${LOCK.commit}`)
    expect(orcaConfigure).toContain('-DSKETCH_ORCA_TAG=v2.4.2')
    expect(text).toContain('The engine is <build>/sketch-orca/sketch-orca.exe.')
    expect(lines).toContain(`- GMP: gmp-6.2.1.tar.bz2 from https://github.com/SoftFever/OrcaSlicer_deps/releases/download/gmp-6.2.1/gmp-6.2.1.tar.bz2 (SHA-256 ${GMP_SHA256})`)
    expect(lines).toContain(`- OpenSSL: OpenSSL_1_1_1w.tar.gz from https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz (SHA-256 ${H('2')})`)
    expect(lines).toContain('- EXPAT: built from its sources inside the OrcaSlicer tree')
    expect(text).not.toContain('prebuilt/')
    // The toolchain it has been built with, by version.
    for (const v of Object.values(BUILT_WITH)) expect(text).toContain(v)
    expect(text).toContain(`- Bun ${BUILT_WITH.bun}, which runs the pipeline scripts, and git.`)
    expect(text).toContain('(the copies Strawberry Perl ships in its c\\bin;')
    expect(text).toContain(`- ${BUILT_WITH.mingw}, which builds GMP and MPFR on Windows (patch 0007); the pipeline fetches its pinned packages itself (see "MinGW-w64" below).`)
    expect(lines).toContain('- TBB: built, but its source package is NOT in this bundle')
    expect(text.endsWith('\n')).toBe(true)
    // TBB has no package: the text says so itself, whatever the caller passes.
    expect(text).toContain('This folder is the source of the sketch-orca engine')
    expect(text).toContain('It is NOT complete corresponding source:')
    expect(lines).toContain('- no pinned source package for the built dependencies TBB; fetch them with build')
    const whole = buildingText({ lock: LOCK, manifest: manifest(), patches, plan: { ...plan, missing: [] }, withDeps: true })
    expect(whole).toContain('This folder is the complete corresponding source of the sketch-orca engine')
    expect(whole).not.toContain('NOT complete')
    // A bundle made with --allow-missing says so, and what the Orca archive leaves out.
    const shipped = "the engine ships binaries of OrcaSlicer's tree, whose source the bundle does not have: bin/libgmp-10.dll (deps/GMP/gmp/lib/win-x64/libgmp-10.dll)"
    const partial = buildingText({ lock: LOCK, manifest: manifest(), patches, plan, withDeps: true, orcaExcluded: orcaArchiveExcludes(), incomplete: [shipped] })
    expect(partial).toContain('This folder is the source of the sketch-orca engine')
    expect(partial).not.toContain('complete corresponding source of')
    expect(partial.split('\n')).toContain(`- ${shipped}`)
    expect(partial).toContain('The OrcaSlicer archive leaves out binaries of its tree that this build does not use: deps/WebView2, tools/*.exe, deps/GMP/gmp/lib, deps/MPFR/mpfr/lib.')

    const linux = buildingText({ lock: LOCK, manifest: manifest({ platform: 'linux' }), patches, plan: { ...plan, missing: [] }, withDeps: false })
    expect(linux).toContain('The dependency packages were left out of this bundle')
    expect(linux).toContain('-DCMAKE_MAKE_PROGRAM=<ninja>')
    expect(linux).not.toContain('-DCMAKE_CXX_COMPILER=cl')
    expect(linux).not.toContain('DEP_MSVC_STATIC_RUNTIME')
    expect(linux).not.toContain('CMAKE_MSVC_RUNTIME_LIBRARY')
    // Off Windows GMP and MPFR build with the system's compilers: no MinGW-w64 step or section.
    expect(linux).not.toContain('DEP_MINGW_ROOT')
    expect(linux).not.toContain('## MinGW-w64')
    expect(linux).not.toContain('(see "MinGW-w64" below)')
    expect(linux).toContain('4. Build the dependencies')
    expect(linux).toContain('5. Build the engine:')
    expect(linux).toContain('The engine is <build>/sketch-orca/sketch-orca.')
    expect(linux).not.toContain('NOT in this bundle')
    // --no-deps leaves the packages out: never complete corresponding source.
    expect(linux).not.toContain('complete corresponding source of')
    expect(linux.split('\n')).toContain('- the source packages of GMP, OpenSSL are left out (--no-deps); bundle.json lists each one with its URL and SHA-256')
  })

  test('sourceGaps: what stops source unless --allow-missing, apart from what --no-deps leaves out', () => {
    const archive = (name: string) => ({ name, file: `${name}.tar.gz`, path: 'p', url: `https://example.invalid/${name}.tar.gz`, sha256: H('2'), size: 1 })
    const plan = { archives: [archive('GMP'), archive('MPFR'), archive('OpenSSL')], inTree: ['EXPAT'], missing: [], unpinned: [] }
    // GMP and MPFR come from their packages like every other dependency: nothing is missing.
    expect(sourceGaps({ plan, withDeps: true })).toEqual({ missing: [], omitted: [] })
    expect(sourceGaps({ plan, withDeps: true, archivedBinaries: [], shippedTreeBinaries: [] })).toEqual({ missing: [], omitted: [] })
    // The audit's case: --no-deps with nothing else missing still leaves out what complete source needs.
    expect(sourceGaps({ plan, withDeps: false })).toEqual({ missing: [], omitted: ['the source packages of GMP, MPFR, OpenSSL are left out (--no-deps); bundle.json lists each one with its URL and SHA-256'] })
    expect(sourceGaps({ plan: { ...plan, archives: [] }, withDeps: false })).toEqual({ missing: [], omitted: [] })
    const gaps = sourceGaps({
      plan: { ...plan, missing: ['TBB'] },
      withDeps: true,
      archivedBinaries: ['tools/new.exe'],
      shippedTreeBinaries: ['bin/libgmp-10.dll (deps/GMP/gmp/lib/win-x64/libgmp-10.dll)'],
    })
    expect(gaps.omitted).toEqual([])
    expect(gaps.missing).toEqual([
      'no pinned source package for the built dependencies TBB; fetch them with build',
      "the engine ships binaries of OrcaSlicer's tree, whose source the bundle does not have: bin/libgmp-10.dll (deps/GMP/gmp/lib/win-x64/libgmp-10.dll); build them from their source packages (GMP and MPFR on Windows: -DDEP_MINGW_ROOT, patch 0007)",
      'the Orca archive keeps binaries whose source the bundle does not have: tools/new.exe; leave them out (orcaArchiveExcludes, scripts/lib/bundle.ts)',
    ])
    // A binary of a package that PACKAGE_BINARIES does not name.
    expect(sourceGaps({ plan, withDeps: true, packageBinaries: ['dependencies/X/x.zip: x/a.jar (SHA-256 aa; Java class)'] }).missing).toEqual([
      'the dependency packages hold binaries the bundle does not account for: dependencies/X/x.zip: x/a.jar (SHA-256 aa; Java class); name each in PACKAGE_BINARIES (scripts/lib/packagebins.ts) with its licence and where its source is',
    ])
    // source works both out from Orca's tree at the locked commit and the staged engine's files,
    // reads every package for binaries, and the Orca archive it writes too.
    const run = readFileSync(join(import.meta.dir, '..', 'scripts', 'lib', 'bundle.ts'), 'utf8').split('export async function runSource(')[1]!
    expect(run).toMatch(/sourceGaps\(\{[^}]*\barchivedBinaries: kept\b[^}]*\bshippedTreeBinaries: shipped\b[^}]*\bpackageBinaries: binaries\?\.unknown\b[^}]*\}\)/)
    expect(run).toMatch(/packageBinaries\(plan\.archives, /)
    expect(run).toMatch(/inventoryPackage\(archive, needles, platform\)/)
  })

  test("packageBinaries: every binary of every package, known by PACKAGE_BINARIES or not; the builder's needles and unnamed folders apart", async () => {
    const putBytes = (rel: string, data: Buffer) => {
      mkdirSync(join(dir, rel, '..'), { recursive: true })
      writeFileSync(join(dir, rel), data)
    }
    const wasm = Buffer.concat([Buffer.from('\0asm'), Buffer.from([1, 0, 0, 0]), Buffer.alloc(64)])
    const pe = (text: string) => Buffer.concat([makePe({ is64: true, imports: [] }), Buffer.from(`\0${text}\0`, 'latin1')])
    putBytes('dl/Draco/1.5.7.zip', writeZip([{ name: 'draco/README.md', data: 'text' }, { name: 'draco/js/decoder.wasm', data: wasm }, { name: 'draco/js/other.wasm', data: Buffer.concat([wasm, Buffer.from('x')]) }]))
    putBytes('dl/Tool/tool-1.0.tar.gz', gzipSync(writeTar([{ name: 'tool/bin/t.dll', data: pe('C:/Users/Builder/tool/t.cpp') }, { name: 'tool/bin/u.dll', data: pe('D:\\agent\\_work\\1\\s\\u.cpp') }])))
    putBytes('dl/Plain/plain-1.0.tar.gz', gzipSync(writeTar([{ name: 'plain/src/a.c', data: 'int a;' }])))
    const archive = (name: string, file: string): DependencyArchive => ({ name, file, path: join(dir, 'dl', name, file), url: `https://example.invalid/${file}`, sha256: sha256Bytes(readFileSync(join(dir, 'dl', name, file))), size: 1 })
    const archives = [archive('Draco', '1.5.7.zip'), archive('Tool', 'tool-1.0.tar.gz'), archive('Plain', 'plain-1.0.tar.gz')]
    const known: KnownPackageBinary[] = [
      { dependency: 'Draco', file: '1.5.7.zip', path: 'draco/js/decoder.wasm', sha256: sha256Bytes(wasm), what: 'a decoder', license: 'Apache-2.0', licenseText: 'LICENSE', source: 'src/' },
    ]
    const cache = join(dir, 'state', 'package-binaries.json')
    const needles = personalPathNeedles('C:\\Users\\Builder', 'Builder')
    const r = await packageBinaries(archives, cache, needles, 'win32', undefined, known)
    expect(r.known).toEqual(known)
    expect(r.unknown).toEqual([
      `dependencies/Draco/1.5.7.zip: draco/js/other.wasm (SHA-256 ${sha256Bytes(Buffer.concat([wasm, Buffer.from('x')]))}; WebAssembly module)`,
      expect.stringMatching(/^dependencies\/Tool\/tool-1\.0\.tar\.gz: tool\/bin\/t\.dll \(SHA-256 [0-9a-f]{64}; PE program or library\)$/),
      expect.stringMatching(/^dependencies\/Tool\/tool-1\.0\.tar\.gz: tool\/bin\/u\.dll /),
    ])
    expect(r.personal).toEqual(['dependencies/Tool/tool-1.0.tar.gz: tool/bin/t.dll (/users/builder)', 'dependencies/Tool/tool-1.0.tar.gz: tool/bin/t.dll (c:/users/builder)'])
    expect(r.recorded).toEqual([
      'dependencies/Tool/tool-1.0.tar.gz: tool/bin/t.dll (build path C:/Users/Builder/tool/t.cpp)',
      'dependencies/Tool/tool-1.0.tar.gz: tool/bin/u.dll (build path D:\\agent\\_work\\1\\s\\u.cpp)',
    ])
    expect(existsSync(cache)).toBe(true)
    // The real table, for these packages: nothing of them is known.
    expect((await packageBinaries(archives, cache, needles, 'win32')).known).toEqual([])
    expect(PACKAGE_BINARIES.length).toBeGreaterThan(0)
    // A package that cannot be read stops source.
    putBytes('dl/Bad/bad.tar.gz', gzipSync(Buffer.alloc(1024, 0x41)))
    await expect(packageBinaries([archive('Bad', 'bad.tar.gz')], cache, needles, 'win32')).rejects.toThrow(/cannot read dependencies\/Bad\/bad\.tar\.gz for its binaries: tar: bad header checksum/)
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
    put(`b/${BINARIES_NOTICE}`, 'list')
    put(`b/${BINARY_LICENSES}/CPL-1.0.txt`, 'text')
    const b = join(dir, 'b')
    const own = ownBundleFiles(listFiles(b))
    expect(own).toEqual(['BUILDING.md', 'LICENSES/NOTICE.txt', 'bundle.json', 'dependencies/BINARIES.txt', 'dependencies/LICENSES/CPL-1.0.txt', 'engine/cli/run.cpp'])
    expect(privacyProblems(b, own, personalPathNeedles('C:\\Users\\Builder', 'Builder', ['D:\\build-cache\\orca']))).toEqual([
      'BUILDING.md (d:\\build-cache\\orca)',
      'engine/cli/run.cpp (/users/builder, c:/users/builder)',
    ])
  })
})

describe("the binaries of Orca's tree", () => {
  // OrcaSlicer v2.4.2's binaries, and files beside them.
  const TREE = [
    ...['win-arm64', 'win-x64', 'win-x86'].flatMap((a) => [`deps/GMP/gmp/lib/${a}/libgmp-10.dll`, `deps/GMP/gmp/lib/${a}/libgmp-10.lib`, `deps/MPFR/mpfr/lib/${a}/libmpfr-4.dll`, `deps/MPFR/mpfr/lib/${a}/libmpfr-4.lib`]),
    ...['win-arm64', 'win-x64', 'win-x86'].flatMap((a) => [`deps/WebView2/lib/${a}/WebView2Loader.dll`, `deps/WebView2/lib/${a}/WebView2LoaderStatic.lib`]),
    'tools/7z.exe',
    'tools/msgfmt.exe',
    'tools/msgmerge.exe',
    'tools/xgettext.exe',
    'tools/README.md',
    'deps/GMP/gmp/include/gmp.h',
    'deps/GMP/gmp/gmp.README',
    'src/libslic3r/Print.cpp',
  ]
  // The GMP 5.0.1 and MPFR 3.0.0 DLLs of CGAL's that OrcaSlicer v2.4.2 keeps for Windows x64.
  const ORCA_GMP_DLL = { path: 'deps/GMP/gmp/lib/win-x64/libgmp-10.dll', sha256: 'fec43cb18ce0ec8a5dd6ad1db745747167310ccb92b5acff0d445a8b3013f009' }
  const ORCA_MPFR_DLL = { path: 'deps/MPFR/mpfr/lib/win-x64/libmpfr-4.dll', sha256: '09b3b868b96433991fc15c9c5ae6f9a44c62d2e21194110442607917391ed927' }

  test("orcaArchiveExcludes and archivedBinaries: the Orca archive keeps no binary of v2.4.2's tree, the prebuilt GMP and MPFR included", () => {
    const ex = orcaArchiveExcludes()
    expect(ex).toEqual(['deps/WebView2', 'tools/*.exe', 'deps/GMP/gmp/lib', 'deps/MPFR/mpfr/lib'])
    expect(excludePathspecs(ex)).toEqual([':(exclude)deps/WebView2', ':(exclude,glob)tools/*.exe', ':(exclude)deps/GMP/gmp/lib', ':(exclude)deps/MPFR/mpfr/lib'])
    expect(archivedBinaries(TREE, ex)).toEqual([])
    // A binary a later Orca release adds, which the archive would carry without its source.
    expect(archivedBinaries([...TREE, 'tools/sub/new.exe', 'deps/Foo/foo.dll'], ex)).toEqual(['tools/sub/new.exe', 'deps/Foo/foo.dll'])
    // Without the exclusions every one of them would ship.
    expect(TREE.filter(isBinaryPath)).toHaveLength(22)
    expect(archivedBinaries(TREE, [])).toEqual(TREE.filter(isBinaryPath))
  })

  test("shippedTreeBinaries: a file of the engine that is a binary of Orca's tree (its prebuilt GMP) is named, by its bytes", () => {
    const exe = { path: 'bin/sketch-orca.exe', sha256: H('a') }
    const tree = [ORCA_GMP_DLL, ORCA_MPFR_DLL]
    // GMP built from its package: another DLL than Orca's.
    expect(shippedTreeBinaries([exe, { path: 'bin/libgmp-10.dll', sha256: H('b') }], tree)).toEqual([])
    expect(shippedTreeBinaries([exe, { path: 'bin/libgmp-10.dll', sha256: ORCA_GMP_DLL.sha256 }], tree)).toEqual(['bin/libgmp-10.dll (deps/GMP/gmp/lib/win-x64/libgmp-10.dll)'])
    // Whatever its name, at every path of the tree that holds those bytes.
    const twice = [ORCA_GMP_DLL, { ...ORCA_GMP_DLL, path: 'deps/GMP/gmp/lib/win-x86/libgmp-10.dll' }]
    expect(shippedTreeBinaries([{ path: 'bin/gmp.dll', sha256: ORCA_GMP_DLL.sha256 }], twice)).toEqual(['bin/gmp.dll (deps/GMP/gmp/lib/win-x64/libgmp-10.dll, deps/GMP/gmp/lib/win-x86/libgmp-10.dll)'])
    // source hashes every binary of the tree at the locked commit, excluded from the archive or not.
    const run = readFileSync(join(import.meta.dir, '..', 'scripts', 'lib', 'bundle.ts'), 'utf8').split('export async function runSource(')[1]!
    expect(run).toContain('treeFiles.filter(isBinaryPath)')
    expect(run).toContain('shippedTreeBinaries(m.files, treeBinaries)')
  })

  test('mingwToolchain: the pinned MSYS2 packages in a Windows bundle, none elsewhere; a new bundle version', () => {
    expect(mingwToolchain('win32').map((p) => p.file)).toEqual([MSYS2_BASE, ...MINGW_PACKAGES].map((p) => p.file))
    expect(mingwToolchain('win32')[0]).toEqual(MSYS2_BASE)
    expect(mingwToolchain('linux')).toEqual([])
    // dependencies/BINARIES.txt and dependencies/LICENSES/: a bundle of version 6 is made again.
    expect(SOURCE_VERSION).toBe(7)
  })

  test.skipIf(firstExisting(gitCandidates(currentPlatform(), process.env)) === undefined)('excludePathspecs: git archive leaves out what orcaArchiveExcludes names', () => {
    const git = firstExisting(gitCandidates(currentPlatform(), process.env))!
    const repo = join(dir, 'orca')
    for (const rel of ['deps/GMP/gmp/lib/win-x64/libgmp-10.dll', 'deps/GMP/gmp/include/gmp.h', 'deps/MPFR/mpfr/lib/win-x64/libmpfr-4.lib', 'deps/WebView2/lib/win-x64/WebView2Loader.dll', 'tools/msgfmt.exe', 'tools/README.md', 'src/a.cpp']) put(`orca/${rel}`, 'x')
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
    run('archive', '--format=tar', '-o', tar, 'HEAD', '--', ...excludePathspecs(orcaArchiveExcludes()))
    const names = readFileSync(tar, 'latin1').match(/(deps|tools|src)\/[\w./-]+/g) ?? []
    expect([...new Set(names)].filter((n) => !n.endsWith('/')).sort()).toEqual(['deps/GMP/gmp/include/gmp.h', 'src/a.cpp', 'tools/README.md'])
    // The bytes of a binary of the tree, from the commit and not from the working tree.
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
