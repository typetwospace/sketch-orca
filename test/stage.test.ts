// privacy-fixtures: /home/me /Users/me /home/李 /Users/李 /Users/PreviousBuilder D:/build-cache/orca me@example.org
import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { listPatches } from '../scripts/lib/checkout.ts'
import { hashTree, nativePath, treeEntries, writeText } from '../scripts/lib/files.ts'
import { LOCK_FORMAT, type OrcaLock } from '../scripts/lib/lock.ts'
import { depsStampProblems, enginePaths, workPaths, writeStamp, type DepsRecord } from '../scripts/lib/paths.ts'
import type { RuntimeIo } from '../scripts/lib/pe.ts'
import { SMOKE_CASES } from '../scripts/lib/smoke.ts'
import {
  buildManifest,
  buildRecordProblems,
  builtDependencies,
  cliChangedSinceBuild,
  dependencySourceDir,
  DEPS_SRC_NOTICES,
  ENGINE_FORMAT,
  findPersonalPaths,
  isLicenseFile,
  isolatedEnv,
  isSafeRelPath,
  isUnixSystemLibrary,
  leadingComment,
  licenseCopies,
  licensesReadme,
  linkedLibraries,
  linkedPartProblems,
  MANIFEST_FILE,
  modifiedDate,
  noticeText,
  ownNoticeCopies,
  parseLdd,
  parseOtoolL,
  parseVersionOutput,
  personalPathNeedles,
  readManifest,
  requiredLicenses,
  resourceCopies,
  runtimeRefusal,
  serializeManifest,
  STAGED_CASES,
  stagedEnv,
  stageInputs,
  STAGE_VERSION,
  unlicensedDependencies,
  USER_FOLDER_NEEDLES,
  validateManifest,
  verifyStaged,
  windowsRuntime,
  type BuildRecord,
  type ManifestParts,
} from '../scripts/lib/stage.ts'

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

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'orca-stage-'))
})
afterEach(() => {
  rmSync(dir, { recursive: true, force: true })
})

function put(rel: string, text = rel): void {
  writeText(nativePath(dir, rel), text)
}

function parts(over: Partial<ManifestParts> = {}): ManifestParts {
  return {
    lock: LOCK,
    engineVersion: '0.1.0',
    exeName: 'sketch-orca.exe',
    platform: 'win32',
    arch: 'x64',
    patches: [{ name: '0001-deps-without-gui.patch', sha256: H('1') }],
    cli: H('c'),
    runtime: { copy: [{ name: 'vcruntime140.dll', path: 'X' }, { name: 'TKernel.dll', path: 'Y' }], system: ['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'], missing: [], crt: [] },
    smokePassed: true,
    inputs: H('d'),
    files: [
      { path: 'resources/profiles/BBL.json', size: 2, sha256: H('b') },
      { path: 'bin/sketch-orca.exe', size: 1, sha256: H('a') },
    ],
    stagedAt: '2026-10-30T12:00:00.000Z',
    ...over,
  }
}

describe('manifest', () => {
  test('buildManifest: the engine in bin/, the lock, sorted files and libraries, the smoke result', () => {
    const m = buildManifest(parts())
    expect(m).toEqual({
      format: ENGINE_FORMAT,
      stageVersion: STAGE_VERSION,
      engine: { name: 'sketch-orca', version: '0.1.0', exe: 'bin/sketch-orca.exe', resources: 'resources', libraryPath: 'bin' },
      orca: { repo: 'OrcaSlicer/OrcaSlicer', tag: 'v2.4.2', version: '2.4.2', commit: LOCK.commit, publishedAt: LOCK.publishedAt, license: 'AGPL-3.0' },
      platform: 'win32',
      arch: 'x64',
      patches: [{ name: '0001-deps-without-gui.patch', sha256: H('1') }],
      cli: H('c'),
      runtime: ['TKernel.dll', 'vcruntime140.dll'],
      system: ['api-ms-win-crt-runtime-l1-1-0.dll', 'KERNEL32.dll'],
      smoke: { passed: true, smokeVersion: expect.any(Number) },
      inputs: H('d'),
      stagedAt: '2026-10-30T12:00:00.000Z',
      files: [
        { path: 'bin/sketch-orca.exe', size: 1, sha256: H('a') },
        { path: 'resources/profiles/BBL.json', size: 2, sha256: H('b') },
      ],
    })
    expect(validateManifest(JSON.parse(serializeManifest(m)))).toEqual(m)
    expect(serializeManifest(m).endsWith('}\n')).toBe(true)
  })

  test('validateManifest names what is wrong', () => {
    const good = JSON.parse(serializeManifest(buildManifest(parts()))) as Record<string, unknown>
    const bad: [Record<string, unknown>, RegExp][] = [
      [{ ...good, format: 'x' }, /format/],
      [{ ...good, platform: 'beos' }, /unknown platform/],
      [{ ...good, engine: { ...(good.engine as object), exe: 'bin/other.exe' } }, /engine\.exe "bin\/other\.exe" is not one of the files/],
      [{ ...good, files: [{ path: '../evil', size: 1, sha256: H('a') }] }, /files\[0\]\.path/],
      [{ ...good, files: [{ path: 'bin/sketch-orca.exe', size: -1, sha256: H('a') }] }, /files\[0\]\.size/],
      [{ ...good, files: [{ path: 'bin/sketch-orca.exe', size: 1, sha256: 'nope' }] }, /files\[0\]\.sha256/],
      [{ ...good, orca: { ...(good.orca as object), commit: 'abc' } }, /orca\.commit/],
      [{ ...good, patches: [{ name: 'p' }] }, /patches\[0\]/],
      [{ ...good, smoke: { passed: 'yes', smokeVersion: 1 } }, /smoke/],
      [{ ...good, runtime: [1] }, /runtime/],
      [{ ...good, inputs: '' }, /inputs/],
    ]
    for (const [value, re] of bad) expect(() => validateManifest(value)).toThrow(re)
  })

  test('isSafeRelPath allows only relative forward-slash paths inside the folder', () => {
    for (const p of ['bin/sketch-orca.exe', 'resources/profiles/BBL/machine/a b.json', 'a']) expect(isSafeRelPath(p)).toBe(true)
    for (const p of ['', '/abs', 'C:/x', 'a\\b', '../x', 'a/../b', 'a//b', './a', 'a/']) expect(isSafeRelPath(p)).toBe(false)
  })

  test('stageInputs changes with every input', () => {
    const base = {
      lock: LOCK,
      patches: [{ name: 'p', sha256: H('1') }],
      cli: H('c'),
      sourceDigest: H('s'),
      exeSha256: H('e'),
      runtime: [
        { name: 'a.dll', sha256: H('2') },
        { name: 'b.dll', sha256: H('4') },
      ],
      licenses: [{ to: 'LICENSES/OrcaSlicer/LICENSE.txt', sha256: H('3') }],
      smokePassed: true,
      platform: 'win32' as const,
    }
    const h = stageInputs(base)
    expect(stageInputs({ ...base })).toBe(h)
    expect(stageInputs({ ...base, runtime: [...base.runtime].reverse() })).toBe(h)
    for (const changed of [
      { ...base, lock: { ...LOCK, commit: 'f'.repeat(40) } },
      { ...base, patches: [] },
      { ...base, cli: H('d') },
      { ...base, sourceDigest: H('t') },
      { ...base, exeSha256: H('f') },
      { ...base, runtime: [{ name: 'a.dll', sha256: H('9') }] },
      { ...base, licenses: [] },
      { ...base, smokePassed: false },
      { ...base, platform: 'linux' as const },
    ]) {
      expect(stageInputs(changed)).not.toBe(h)
    }
  })

  test('buildRecordProblems: the engine must be the build of the commit, patches, checkout, dependencies and libraries there are now', () => {
    const now: BuildRecord = {
      commit: LOCK.commit,
      patches: [{ name: '0001-a.patch', sha256: H('1') }],
      sourceDigest: H('s'),
      exeSha256: H('e'),
      runtime: [
        ['TKernel.dll', H('2')],
        ['libgmp-10.dll', H('3')],
      ],
      depsInputs: H('d'),
      depsPrefixDigest: H('p'),
    }
    // As build writes it, beside its other fields.
    const recorded = { exe: 'sketch-orca.exe', cliSha256: H('c'), compilerEnv: {}, ...now, runtime: [...now.runtime].reverse() }
    expect(buildRecordProblems(recorded, now)).toEqual([])
    // A build an earlier pipeline recorded, or none.
    expect(buildRecordProblems({ exe: 'sketch-orca.exe', cliSha256: H('c') }, now)).toEqual(['the engine build was recorded without what it was built from (an earlier pipeline)'])
    expect(buildRecordProblems(undefined, now)).toHaveLength(1)
    const differs = (change: Partial<BuildRecord>) => buildRecordProblems({ ...recorded, ...change }, now)
    expect(differs({ commit: 'f'.repeat(40) })).toEqual([`the engine was built at ${'f'.repeat(40)}, the lock is at ${LOCK.commit}`])
    // build --step deps applied a new patch, and the engine was not built again.
    expect(differs({ patches: [] })).toEqual(['the engine was built with other patches than patches/ has now'])
    expect(differs({ sourceDigest: H('t') })).toEqual(['the checkout changed since the engine was built (a file edited, added or removed against the locked commit and patches/)'])
    expect(differs({ exeSha256: H('f') })).toEqual(['the engine is not the file the build made'])
    expect(differs({ runtime: [['TKernel.dll', H('9')], ['libgmp-10.dll', H('3')]] })).toEqual(['the libraries the engine loads changed since it was built'])
    expect(differs({ runtime: [['TKernel.dll', H('2')]] })).toEqual(['the libraries the engine loads changed since it was built'])
    expect(differs({ depsInputs: null })).toEqual(['the dependencies were built again, from other inputs, since the engine was'])
    // The same dependency build, but its prefix changed after the engine was linked against it (or a record without the digest).
    expect(differs({ depsPrefixDigest: H('q') })).toEqual(['the dependency prefix is not the one the engine was built with'])
    expect(buildRecordProblems({ ...recorded, depsPrefixDigest: undefined }, now)).toEqual(['the dependency prefix is not the one the engine was built with'])
  })

  test('depsStampProblems: the dependencies must be a finished build of the commit and deps/ patches there are now, fresh for stage, with the prefix it installed', () => {
    const depsPatches = [{ name: '0001-deps.patch', sha256: H('1') }]
    const record: DepsRecord = { commit: LOCK.commit, depsPatches, prefixDigest: H('p'), fresh: true }
    const stamp = { inputs: H('i'), finishedAt: '2026-11-01T00:00:00.000Z', detail: record }
    let hashed = 0
    const digest = (d: string) => () => (hashed++, d)
    const want = { commit: LOCK.commit, depsPatches, inputs: H('i'), fresh: true, prefixDigest: digest(H('p')) }
    expect(depsStampProblems(stamp, want)).toEqual([])
    expect(hashed).toBe(1)
    expect(depsStampProblems(undefined, want)).toEqual(['no finished dependency build is recorded (state/deps.json)'])
    // A stamp of the earlier pipeline: only the hash of the inputs, nothing to check the prefix by.
    expect(depsStampProblems({ inputs: H('i') }, want)).toEqual(['the dependency build was recorded by an earlier pipeline, without what it was built from or a digest of what it installed'])
    // The audit's case: a patch of deps/ changed, and build --step orca (or stage) would have taken the old dependencies.
    hashed = 0
    expect(depsStampProblems(stamp, { ...want, depsPatches: [{ name: '0001-deps.patch', sha256: H('2') }] })).toEqual(['the dependencies were built with other patches of deps/ than patches/ has now'])
    expect(depsStampProblems(stamp, { ...want, depsPatches: [] })).toEqual(['the dependencies were built with other patches of deps/ than patches/ has now'])
    expect(depsStampProblems(stamp, { ...want, commit: 'f'.repeat(40) })).toEqual([`the dependencies were built at ${LOCK.commit}, the lock is at ${'f'.repeat(40)}`])
    expect(depsStampProblems(stamp, { ...want, inputs: H('j') })).toEqual(['the dependencies were built from other inputs (their configuration, CMake or the compiler settings changed since)'])
    // The prefix is read only when nothing else is wrong.
    expect(hashed).toBe(0)
    // An edit, an added or a removed file in the prefix after the build.
    expect(depsStampProblems(stamp, { ...want, prefixDigest: digest(H('q')) })).toEqual(['the dependency prefix is not what the dependency build installed (a file changed, added or removed since)'])
    // A build that resumed one that stopped does for build, not for stage.
    const resumed = { ...stamp, detail: { ...record, fresh: false } }
    expect(depsStampProblems(resumed, { ...want, fresh: false })).toEqual([])
    expect(depsStampProblems(resumed, want)).toEqual(['the dependency build was resumed after it stopped, so an unpacked source may have been edited in between'])
    // stage has no toolchain to work the inputs out: commit, patches, fresh and prefix are checked.
    const { inputs: _, ...stageWant } = want
    expect(depsStampProblems({ ...stamp, inputs: H('j') }, stageWant)).toEqual([])
  })

  test('verifyStaged checks the folder against its manifest, the manifest aside', () => {
    put('bin/sketch-orca.exe', 'exe')
    put('resources/info/nozzle_info.json', '{}')
    const m = buildManifest(parts({ files: treeEntries(dir) }))
    writeText(join(dir, MANIFEST_FILE), serializeManifest(m))
    expect(readManifest(dir)).toEqual(m)
    expect(verifyStaged(dir, m)).toEqual([])
    put('resources/info/nozzle_info.json', '{"x":1}')
    put('bin/extra.dll', 'x')
    expect(verifyStaged(dir, m)).toEqual(['unexpected file bin/extra.dll', 'changed resources/info/nozzle_info.json'])
  })
})

describe('what goes in', () => {
  test('licence files by name', () => {
    for (const n of ['LICENSE', 'LICENSE.txt', 'LICENSE_1_0.txt', 'License.md', 'COPYING', 'COPYING.LESSER', 'COPYRIGHT', 'NOTICE.md', 'OCCT_LGPL_EXCEPTION.txt', 'LICENSE_LGPL_21.txt', 'README.ijg', 'gmp.COPYING', 'mpfr.COPYING.LESSER', 'PATENTS']) {
      expect(isLicenseFile(n)).toBe(true)
    }
    for (const n of ['README.md', 'CMakeLists.txt', 'INSTALL', 'boost.png', 'README']) expect(isLicenseFile(n)).toBe(false)
  })

  test('builtDependencies: the dependencies with an ExternalProject done stamp', () => {
    put('deps/dep_Boost-prefix/src/dep_Boost-stamp/dep_Boost-done', '')
    put('deps/dep_CURL-prefix/src/dep_CURL-stamp/dep_CURL-download', '')
    put('deps/dep_OCCT-prefix/src/dep_OCCT-stamp/Release/dep_OCCT-done', '')
    mkdirSync(join(dir, 'deps', 'CMakeFiles'), { recursive: true })
    expect(builtDependencies(join(dir, 'deps'))).toEqual(['Boost', 'OCCT'])
    expect(builtDependencies(join(dir, 'none'))).toEqual([])
  })

  test('dependencySourceDir: the unpacked package, else the SOURCE_DIR its recipe names in Orca\'s tree', () => {
    const w = workPaths(dir)
    put('deps/dep_Boost-prefix/src/dep_Boost/LICENSE_1_0.txt')
    put('src/deps/EXPAT/EXPAT.cmake', 'orcaslicer_add_cmake_project(EXPAT\n  # GIT_REPOSITORY https://example.invalid/x.git\n  SOURCE_DIR          ${CMAKE_CURRENT_LIST_DIR}/expat\n)\n')
    put('src/deps/EXPAT/expat/COPYING')
    put('src/deps/TBB/TBB.cmake', 'orcaslicer_add_cmake_project(TBB URL https://example.invalid/tbb.zip)\n')
    expect(dependencySourceDir(w, 'Boost')).toBe(join(w.deps, 'dep_Boost-prefix', 'src', 'dep_Boost'))
    expect(dependencySourceDir(w, 'EXPAT')).toBe(join(w.src, 'deps', 'EXPAT', 'expat'))
    expect(dependencySourceDir(w, 'TBB')).toBeUndefined()
    expect(dependencySourceDir(w, 'Nope')).toBeUndefined()
  })

  test('licenseCopies: Orca\'s licence, the built dependencies\' (with the texts FreeType points to, and ours for libnoise), deps_src (files and notices), OpenCV\'s third-party notices from the prefix, and the prebuilt GMP (with its README and the LGPL 3) and MPFR on Windows', () => {
    const w = workPaths(dir)
    const own = join(dir, 'engine-licenses')
    put('engine-licenses/LGPL-2.1.txt')
    put('engine-licenses/unused.txt')
    put('src/LICENSE.txt', 'AGPL')
    put('src/README.md')
    put('src/deps_src/clipper/clipper.hpp', '/* License: Boost Software License */\nnamespace ClipperLib {}\n')
    put('src/deps_src/libigl/igl/igl_inline.h', '// This Source Code Form is subject to the terms of the Mozilla Public License v. 2.0.\n#pragma once\n')
    put('deps/dep_Eigen-prefix/src/dep_Eigen-stamp/dep_Eigen-done', '')
    put('deps/dep_Eigen-prefix/src/dep_Eigen/COPYING.MPL2')
    for (const n of ['Boost', 'FREETYPE', 'Blosc', 'CURL', 'libnoise']) put(`deps/dep_${n}-prefix/src/dep_${n}-stamp/${n === 'CURL' ? 'x' : `dep_${n}-done`}`, '')
    put('deps/dep_libnoise-prefix/src/dep_libnoise/README.md')
    put('deps/dep_libnoise-prefix/src/dep_libnoise/src/noise.h')
    put('deps/dep_Boost-prefix/src/dep_Boost/LICENSE_1_0.txt')
    put('deps/dep_Boost-prefix/src/dep_Boost/README.md')
    put('deps/dep_FREETYPE-prefix/src/dep_FREETYPE/LICENSE.TXT')
    put('deps/dep_FREETYPE-prefix/src/dep_FREETYPE/docs/FTL.TXT')
    put('deps/dep_FREETYPE-prefix/src/dep_FREETYPE/docs/GPLv2.TXT')
    put('deps/dep_Blosc-prefix/src/dep_Blosc/LICENSES/BLOSC.txt')
    put('deps/dep_Blosc-prefix/src/dep_Blosc/LICENSES/LZ4.txt')
    put('deps/dep_CURL-prefix/src/dep_CURL/COPYING')
    put('src/deps_src/qhull/COPYING.txt')
    put('src/deps_src/clipper/clipper.cpp')
    put('src/deps/GMP/gmp/gmp.COPYING')
    put('src/deps/GMP/gmp/gmp.README')
    put('src/deps/MPFR/mpfr/mpfr.COPYING.LESSER')
    // OpenCV puts the notices of its third-party code into the install prefix (etc/licenses on Windows).
    put('deps/dep_OpenCV-prefix/src/dep_OpenCV-stamp/dep_OpenCV-done', '')
    put('deps/dep_OpenCV-prefix/src/dep_OpenCV/LICENSE')
    put('deps/OrcaSlicer_dep/usr/local/etc/licenses/SoftFloat-COPYING.txt')
    put('deps/OrcaSlicer_dep/usr/local/etc/licenses/libpng-LICENSE')
    put('deps/OrcaSlicer_dep/usr/local/etc/licenses/libpng-README')
    const win = licenseCopies(w, 'win32', own)
    expect(win.map((c) => c.to)).toEqual([
      'LICENSES/OrcaSlicer/LICENSE.txt',
      'LICENSES/deps/Blosc/LICENSES/BLOSC.txt',
      'LICENSES/deps/Blosc/LICENSES/LZ4.txt',
      'LICENSES/deps/Boost/LICENSE_1_0.txt',
      'LICENSES/deps/Eigen/COPYING.MPL2',
      'LICENSES/deps/FREETYPE/LICENSE.TXT',
      'LICENSES/deps/FREETYPE/docs/FTL.TXT',
      'LICENSES/deps/FREETYPE/docs/GPLv2.TXT',
      'LICENSES/deps/GMP/LGPL-3.0.txt',
      'LICENSES/deps/GMP/gmp.COPYING',
      'LICENSES/deps/GMP/gmp.README',
      'LICENSES/deps/MPFR/mpfr.COPYING.LESSER',
      'LICENSES/deps/OpenCV/LICENSE',
      'LICENSES/deps/OpenCV/third-party/SoftFloat-COPYING.txt',
      'LICENSES/deps/OpenCV/third-party/libpng-LICENSE',
      'LICENSES/deps/OpenCV/third-party/libpng-README',
      'LICENSES/deps/libnoise/LGPL-2.1.txt',
      'LICENSES/deps/libnoise/src/noise.h',
      'LICENSES/deps_src/clipper/clipper.hpp.notice.txt',
      'LICENSES/deps_src/libigl/MPL-2.0.txt',
      'LICENSES/deps_src/libigl/igl_inline.h.notice.txt',
      'LICENSES/deps_src/qhull/COPYING.txt',
    ].sort())
    expect(win[0]!.from).toBe(join(dir, 'src', 'LICENSE.txt'))
    expect(win.find((c) => c.to === 'LICENSES/deps/libnoise/LGPL-2.1.txt')!.from).toBe(join(own, 'LGPL-2.1.txt'))
    // GMP's LGPL 3 text is MPFR's copy; libigl's MPL 2.0 text is Eigen's.
    expect(win.find((c) => c.to === 'LICENSES/deps/GMP/LGPL-3.0.txt')!.from).toBe(join(dir, 'src', 'deps', 'MPFR', 'mpfr', 'mpfr.COPYING.LESSER'))
    expect(win.find((c) => c.to === 'LICENSES/deps_src/libigl/MPL-2.0.txt')!.from).toBe(join(dir, 'deps', 'dep_Eigen-prefix', 'src', 'dep_Eigen', 'COPYING.MPL2'))
    expect(win.find((c) => c.to === 'LICENSES/deps/OpenCV/third-party/SoftFloat-COPYING.txt')!.from).toBe(join(dir, 'deps', 'OrcaSlicer_dep', 'usr', 'local', 'etc', 'licenses', 'SoftFloat-COPYING.txt'))
    const notice = win.find((c) => c.to === 'LICENSES/deps_src/clipper/clipper.hpp.notice.txt')!
    expect(notice.notice).toBe('SPDX-License-Identifier: BSL-1.0\nThe licence notice at the top of deps_src/clipper/clipper.hpp in the OrcaSlicer source tree:')
    expect(noticeText(notice.notice!, notice.from)).toBe(`${notice.notice}\n\n/* License: Boost Software License */\n`)
    expect(requiredLicenses('win32').every((to) => win.some((c) => c.to === to))).toBe(true)
    expect(requiredLicenses('linux')).toEqual([])
    expect(licenseCopies(w, 'linux', own).map((c) => c.to)).not.toContain('LICENSES/deps/GMP/gmp.COPYING')
    expect(unlicensedDependencies(w, win)).toEqual([])
  })

  test('DEPS_SRC_NOTICES: the parts of deps_src the engine has code of and that have no licence file', () => {
    expect(Object.keys(DEPS_SRC_NOTICES).sort()).toEqual(['Shiny', 'admesh', 'ankerl', 'clipper', 'clipper2', 'fast_float', 'glu-libtess', 'libigl', 'nanosvg', 'nlohmann', 'qoi', 'semver'])
    expect(DEPS_SRC_NOTICES['glu-libtess']).toEqual({ file: 'src/tess.c', spdx: 'SGI-B-2.0' })
    expect(DEPS_SRC_NOTICES.admesh!.spdx).toBe('GPL-2.0-or-later')
  })

  test('leadingComment: the comment block or // lines at the top, up to the first code; noticeText refuses a file without a licence notice', () => {
    expect(leadingComment('\uFEFF\n/*\n * SGI FREE SOFTWARE LICENSE B\n * Copyright (C) 1991\n */\n#include <x.h>\n/* later */\n')).toBe('/*\n * SGI FREE SOFTWARE LICENSE B\n * Copyright (C) 1991\n */')
    expect(leadingComment('// fast_float\r\n\r\n// Permission is hereby granted\r\n#pragma once\r\n// not this\r\n')).toBe('// fast_float\n\n// Permission is hereby granted')
    expect(leadingComment('/* one line */ \nint x;\n')).toBe('/* one line */')
    expect(leadingComment('int x;\n')).toBe('')
    put('plain.h', '#pragma once\n// MIT licence below the code does not count\n')
    expect(() => noticeText('x', join(dir, 'plain.h'))).toThrow(/starts with no licence notice/)
  })

  test('linkedLibraries reads LINK_LIBRARIES of the sketch-orca target from build.ninja', () => {
    const ninja = [
      'build other.exe: LINK a.obj',
      '  LINK_LIBRARIES = other.lib',
      '# Link the executable sketch-orca\\sketch-orca.exe',
      '',
      'build sketch-orca\\sketch-orca.exe: CXX_EXECUTABLE_LINKER main.cpp.obj | src\\libslic3r\\libslic3r.lib',
      '  FLAGS = /O2',
      '  LINK_LIBRARIES = src\\libslic3r\\libslic3r.lib  deps_src\\clipper\\clipper.lib  "C:\\Users\\John Smith\\w\\deps\\usr\\local\\lib\\zlib.lib"  lib\\semver.lib  kernel32.lib',
      '  TARGET_FILE = sketch-orca\\sketch-orca.exe',
      'build x: phony',
      '  LINK_LIBRARIES = later.lib',
    ].join('\r\n')
    expect(linkedLibraries(ninja)).toEqual(['src\\libslic3r\\libslic3r.lib', 'deps_src\\clipper\\clipper.lib', 'C:\\Users\\John Smith\\w\\deps\\usr\\local\\lib\\zlib.lib', 'lib\\semver.lib', 'kernel32.lib'])
    expect(linkedLibraries('build sketch-orca/sketch-orca: CXX_EXECUTABLE_LINKER m.o\n  LINK_LIBRARIES = -Wl,-rpath,/x  deps_src/qoi/libqoi.a  /w/deps/usr/local/lib/libz.a  -lpthread\n')).toEqual(['deps_src/qoi/libqoi.a', '/w/deps/usr/local/lib/libz.a'])
    expect(linkedLibraries('build other: phony\n')).toEqual([])
  })

  test('linkedPartProblems: every linked deps_src library needs a licence text, Intel IPP and unknown libraries are refused', () => {
    const names = ['clipper', 'clipper2', 'qhull', 'semver', 'stb_dxt']
    const copies = [
      { from: 'a', to: 'LICENSES/deps_src/clipper/clipper.hpp.notice.txt' },
      { from: 'b', to: 'LICENSES/deps_src/qhull/COPYING.txt' },
      { from: 'c', to: 'LICENSES/deps_src/semver/semver.h.notice.txt' },
    ]
    const ok = ['src\\libslic3r\\libslic3r.lib', 'deps_src\\clipper\\clipper.lib', 'deps_src\\qhull\\qhullstatic.lib', 'lib\\semver.lib', 'kernel32.lib', 'C:\\w\\deps\\usr\\local\\lib\\zlib.lib']
    expect(linkedPartProblems(ok, names, copies)).toEqual([])
    expect(linkedPartProblems([...ok, 'deps_src\\clipper2\\Clipper2.lib'], names, copies)).toEqual(['no licence text for deps_src/clipper2, which the engine links; add it to DEPS_SRC_NOTICES (scripts/lib/stage.ts)'])
    expect(linkedPartProblems([...ok, 'C:\\w\\deps\\usr\\local\\x64\\vc17\\staticlib\\ippicvmt.lib', 'C:\\w\\x\\ippiw.lib'], names, copies)).toEqual(['the engine links Intel IPP (ippicvmt.lib, ippiw.lib); build the dependencies with DEP_OPENCV_IPP=OFF'])
    expect(linkedPartProblems([...ok, 'lib\\mystery.lib'], names, copies)).toEqual(["the engine links lib/mystery.lib, which is neither Orca's own (src/) nor a deps_src part"])
  })

  test('personalPathNeedles and findPersonalPaths: the home folder and user name in paths, any case, any slash, 8-bit or UTF-16LE', () => {
    const needles = personalPathNeedles('C:\\Users\\Me\\', 'Me')
    expect(needles).toEqual(['/home/me', '/users/me', '\\home\\me', '\\users\\me', 'c:/users/me', 'c:\\users\\me'])
    expect(personalPathNeedles('/home/me', 'me')).toEqual(['/home/me', '/users/me', '\\home\\me', '\\users\\me'])
    const found = (text: string, enc: BufferEncoding = 'latin1') => findPersonalPaths(Buffer.from(`\0\0${text}\0\0`, enc), needles)
    expect(found('C:\\USERS\\ME\\AppData\\Local\\x.cxx')).toEqual(['\\users\\me', 'c:\\users\\me'])
    expect(found('D:/Users/me/src/a.h')).toEqual(['/users/me'])
    expect(found('C:\\Users\\Me\\x', 'utf16le')).toEqual(['\\users\\me', 'c:\\users\\me'])
    expect(found('deps\\OrcaSlicer_dep\\usr\\local\\include\\boost-1_84\\boost/exception')).toEqual([])
    // Only paths: the bare user name is too common a word to refuse on.
    expect(found('memory meme')).toEqual([])
  })

  test('findPersonalPaths folds only ASCII letters, in UTF-16LE a whole code unit at either alignment', () => {
    // U+674E is the bytes 4E 67 in UTF-16LE: folding the 4E (N) byte to 6E would lose the path.
    const needles = personalPathNeedles('C:\\Users\\李', '李')
    expect(needles).toEqual(['/home/李', '/users/李', '\\home\\李', '\\users\\李', 'c:/users/李', 'c:\\users\\李'])
    for (const pad of ['\0\0', '\0']) {
      expect(findPersonalPaths(Buffer.concat([Buffer.from(pad, 'latin1'), Buffer.from('C:/Users/李/build/engine.pdb\0', 'utf16le')]), needles)).toEqual(['/users/李', 'c:/users/李'])
      expect(findPersonalPaths(Buffer.concat([Buffer.from(pad, 'latin1'), Buffer.from('C:\\USERS\\李\\x.cxx', 'utf8')]), needles)).toEqual(['\\users\\李', 'c:\\users\\李'])
    }
    // A UTF-16LE match starts on a code unit of the alignment it was folded at: U+474E U+0100 at an
    // odd offset is not 李 U+0100, though folding the even units (47 00 is G) would make its bytes so.
    expect(findPersonalPaths(Buffer.from([0x00, 0x4e, 0x47, 0x00, 0x01]), ['李Ā'])).toEqual([])
    expect(findPersonalPaths(Buffer.from([0x00, 0x4e, 0x67, 0x00, 0x01]), ['李Ā'])).toEqual(['李Ā'])
    // A needle Latin-1 cannot hold is not looked for as Latin-1 (Buffer would keep only the low bytes: "N").
    expect(findPersonalPaths(Buffer.from('\\Users\\N', 'latin1'), ['\\users\\李'])).toEqual([])
    // Other letters are not folded, in the needles or in the files.
    expect(personalPathNeedles('C:\\Users\\Émile', 'Émile')).toContain('c:\\users\\Émile')
    expect(findPersonalPaths(Buffer.from('C:\\USERS\\Émile', 'utf8'), ['c:\\users\\Émile'])).toEqual(['c:\\users\\Émile'])
  })

  test('personalPathNeedles: the build folders and git e-mail too; USER_FOLDER_NEEDLES find any Windows user folder', () => {
    expect(personalPathNeedles('/home/me', 'me', ['D:\\Build-Cache\\orca\\', 'E:\\'], ['Me@Example.org', 'not an address'])).toEqual([
      '/home/me',
      '/users/me',
      '\\home\\me',
      '\\users\\me',
      'd:/build-cache/orca',
      'd:\\build-cache\\orca',
      'me@example.org',
    ])
    expect(findPersonalPaths(Buffer.from('C:/Users/PreviousBuilder/build/engine.pdb'), [...USER_FOLDER_NEEDLES])).toEqual([':/users/'])
    expect(findPersonalPaths(Buffer.from('D:\\USERS\\x', 'utf16le'), [...USER_FOLDER_NEEDLES])).toEqual([':\\users\\'])
    expect(findPersonalPaths(Buffer.from('https://example.org/users/x'), [...USER_FOLDER_NEEDLES])).toEqual([])
  })

  test('cliChangedSinceBuild: the cli/ hash the build recorded must match cli/ now', () => {
    const w = workPaths(join(dir, 'work'))
    put('cli/main.cpp', 'int main() {}\n')
    const cli = join(dir, 'cli')
    expect(cliChangedSinceBuild(w, cli)).toMatch(/recorded no hash/)
    writeStamp(w.state, 'orca', 'x', { exe: 'sketch-orca.exe' })
    expect(cliChangedSinceBuild(w, cli)).toMatch(/recorded no hash/)
    writeStamp(w.state, 'orca', 'x', { exe: 'sketch-orca.exe', cliSha256: hashTree(cli) })
    expect(cliChangedSinceBuild(w, cli)).toBeUndefined()
    // An edit made while the build ran but before it finished: the file times would not tell.
    writeFileSync(join(cli, 'main.cpp'), 'int main() { return 1; }\n')
    expect(cliChangedSinceBuild(w, cli)).toMatch(/not what the engine was built from/)
  })

  test('modifiedDate and LICENSES/README.txt: the newest date of the patch headers', () => {
    const patches = listPatches(join(import.meta.dir, '..', 'patches'))
    expect(modifiedDate(patches)).toMatch(/^\d{4}-\d{2}-\d{2}$/)
    put('p/0001-a.patch', 'Title\nModified for Sketch, 2026-10-29.\n\n---\ndiff --git a/x b/x\n')
    put('p/0002-b.patch', 'Title\nModified for Sketch, 2026-10-30.\n\n---\n+Modified for Sketch, 2027-01-01.\n')
    put('p/0003-c.patch', 'Title without a date\n---\n')
    expect(modifiedDate(listPatches(join(dir, 'p')))).toBe('2026-10-30')
    expect(modifiedDate([])).toBeUndefined()
    expect(licensesReadme(LOCK, '2026-10-30')).toContain('Modified for Sketch, 2026-10-30: the changes are the patches of the source bundle')
    expect(licensesReadme(LOCK)).not.toContain('Modified for Sketch')
  })

  test("ownNoticeCopies: the repository's NOTICE and AUTHORS go into LICENSES/; a missing one stops staging", () => {
    const engine = enginePaths(join(import.meta.dir, '..'))
    expect(ownNoticeCopies(engine)).toEqual([
      { from: engine.notice, to: 'LICENSES/NOTICE.txt' },
      { from: engine.authors, to: 'LICENSES/AUTHORS.txt' },
    ])
    put('e/NOTICE', 'notice')
    expect(() => ownNoticeCopies(enginePaths(join(dir, 'e')))).toThrow(/AUTHORS is missing; every staged engine carries it as LICENSES\/AUTHORS\.txt/)
    const readme = licensesReadme(LOCK)
    expect(readme).toContain('NOTICE.txt  what sketch-orca is based on')
    expect(readme).toContain('AUTHORS.txt the copyright holders of the changes')
  })

  test('unlicensedDependencies: a built dependency with no licence text found stops staging', () => {
    const w = workPaths(dir)
    for (const n of ['Boost', 'Quiet']) put(`deps/dep_${n}-prefix/src/dep_${n}-stamp/dep_${n}-done`, '')
    put('deps/dep_Boost-prefix/src/dep_Boost/LICENSE_1_0.txt')
    put('deps/dep_Quiet-prefix/src/dep_Quiet/README.md')
    const copies = licenseCopies(w, 'linux', join(dir, 'no-own-licenses'))
    expect(copies.map((c) => c.to)).toEqual(['LICENSES/deps/Boost/LICENSE_1_0.txt'])
    expect(unlicensedDependencies(w, copies)).toEqual(['Quiet'])
  })

  test('resourceCopies: profiles, info and flush of the checkout; a missing one is refused', () => {
    put('src/resources/profiles/BBL.json')
    put('src/resources/profiles/BBL/machine/a.json')
    put('src/resources/info/nozzle_info.json')
    put('src/resources/flush/flush_data_standard.txt')
    put('src/resources/fonts/big.ttf')
    const list = (d: string) => treeEntries(d).map((e) => e.path)
    expect(resourceCopies(join(dir, 'src'), list).map((c) => c.to)).toEqual([
      'resources/profiles/BBL.json',
      'resources/profiles/BBL/machine/a.json',
      'resources/info/nozzle_info.json',
      'resources/flush/flush_data_standard.txt',
    ])
    rmSync(join(dir, 'src', 'resources', 'flush'), { recursive: true })
    expect(() => resourceCopies(join(dir, 'src'), list)).toThrow(/no resources\/flush/)
  })
})

describe('runtime libraries', () => {
  test('Windows: the import closure; the MSVC runtime must be found, never taken from System32, and every import of the dynamic C runtime is named', () => {
    const files = new Map<string, string[]>([
      ['/bin/sketch-orca.exe', ['KERNEL32.dll', 'TKernel.dll', 'VCRUNTIME140.dll', 'api-ms-win-crt-runtime-l1-1-0.dll']],
      ['/bin/TKernel.dll', ['KERNEL32.dll', 'vcruntime140.dll', 'MSVCP140.dll', 'nowhere.dll']],
      ['/deps/vcruntime140.dll', ['KERNEL32.dll']],
    ])
    const io: RuntimeIo = {
      importsOf: (p) => files.get(p) ?? [],
      find: (d, n) => [...files.keys()].find((p) => p.toLowerCase() === `${d}/${n}`.toLowerCase()),
      isSystem: (n) => ['kernel32.dll', 'msvcp140.dll', 'vcruntime140.dll'].includes(n.toLowerCase()),
    }
    expect(windowsRuntime('/bin/sketch-orca.exe', ['/bin', '/deps'], io)).toEqual({
      copy: [
        { name: 'TKernel.dll', path: '/bin/TKernel.dll' },
        { name: 'VCRUNTIME140.dll', path: '/deps/vcruntime140.dll' },
      ],
      system: ['api-ms-win-crt-runtime-l1-1-0.dll', 'KERNEL32.dll'],
      missing: ['MSVCP140.dll (needed by TKernel.dll)', 'nowhere.dll (needed by TKernel.dll)'],
      crt: [
        'VCRUNTIME140.dll (needed by sketch-orca.exe)',
        'api-ms-win-crt-runtime-l1-1-0.dll (needed by sketch-orca.exe)',
        'vcruntime140.dll (needed by TKernel.dll)',
        'MSVCP140.dll (needed by TKernel.dll)',
      ],
    })
  })

  test("Windows: an engine built with the static runtime imports none of it; GMP's msvcrt.dll is the system's", () => {
    const files = new Map<string, string[]>([
      ['/bin/sketch-orca.exe', ['KERNEL32.dll', 'TKernel.dll', 'libgmp-10.dll', 'freetype.dll']],
      ['/bin/TKernel.dll', ['KERNEL32.dll', 'ADVAPI32.dll', 'freetype.dll']],
      ['/bin/freetype.dll', ['KERNEL32.dll']],
      ['/bin/libgmp-10.dll', ['KERNEL32.dll', 'msvcrt.dll']],
    ])
    const io: RuntimeIo = {
      importsOf: (p) => files.get(p) ?? [],
      find: (d, n) => [...files.keys()].find((p) => p.toLowerCase() === `${d}/${n}`.toLowerCase()),
      isSystem: (n) => ['kernel32.dll', 'advapi32.dll', 'msvcrt.dll'].includes(n.toLowerCase()),
    }
    expect(windowsRuntime('/bin/sketch-orca.exe', ['/bin'], io)).toEqual({
      copy: [
        { name: 'TKernel.dll', path: '/bin/TKernel.dll' },
        { name: 'libgmp-10.dll', path: '/bin/libgmp-10.dll' },
        { name: 'freetype.dll', path: '/bin/freetype.dll' },
      ],
      system: ['ADVAPI32.dll', 'KERNEL32.dll', 'msvcrt.dll'],
      missing: [],
      crt: [],
    })
  })

  test('Windows: stage refuses an engine whose dependency loads a debug or an older Visual C++ runtime, also one System32 has', () => {
    const files = new Map<string, string[]>([
      ['/bin/sketch-orca.exe', ['KERNEL32.dll', 'TKernel.dll', 'libgmp-10.dll']],
      ['/bin/TKernel.dll', ['KERNEL32.dll', 'ucrtbased.dll', 'msvcr120.dll']],
      ['/bin/libgmp-10.dll', ['KERNEL32.dll', 'msvcrt.dll']],
    ])
    const io: RuntimeIo = {
      importsOf: (p) => files.get(p) ?? [],
      find: (d, n) => [...files.keys()].find((p) => p.toLowerCase() === `${d}/${n}`.toLowerCase()),
      isSystem: (n) => ['kernel32.dll', 'msvcrt.dll', 'msvcr120.dll'].includes(n.toLowerCase()),
    }
    const plan = windowsRuntime('/bin/sketch-orca.exe', ['/bin'], io)
    expect(plan.crt).toEqual(['ucrtbased.dll (needed by TKernel.dll)', 'msvcr120.dll (needed by TKernel.dll)'])
    expect(plan.system).toEqual(['KERNEL32.dll', 'msvcrt.dll'])
    const refusal = runtimeRefusal(plan)
    expect(refusal).toContain("MSVC's dynamic C runtime")
    expect(refusal).toContain('ucrtbased.dll (needed by TKernel.dll), msvcr120.dll (needed by TKernel.dll)')
    // The static runtime's engine, with GMP's msvcrt.dll, passes.
    expect(runtimeRefusal({ copy: [], system: ['KERNEL32.dll', 'msvcrt.dll'], missing: [], crt: [] })).toBeUndefined()
    expect(runtimeRefusal({ copy: [], system: [], missing: ['zlib1.dll (needed by TKernel.dll)'], crt: [] })).toContain('not all found: zlib1.dll')
  })

  test('ldd output: resolved, not found, and the lines without =>', () => {
    const text = [
      '\tlinux-vdso.so.1 (0x00007ffc8d5f2000)',
      '\tlibTKernel.so.7 => /home/me/.local/share/SketchDev/orca-engine/deps/OrcaSlicer_dep/usr/local/lib/libTKernel.so.7 (0x00007f0a1c000000)',
      '\tlibm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x00007f0a1bf00000)',
      '\tlibgone.so.1 => not found',
      '\t/lib64/ld-linux-x86-64.so.2 (0x00007f0a1d000000)',
    ].join('\n')
    expect(parseLdd(text)).toEqual([
      { name: 'libTKernel.so.7', path: '/home/me/.local/share/SketchDev/orca-engine/deps/OrcaSlicer_dep/usr/local/lib/libTKernel.so.7' },
      { name: 'libm.so.6', path: '/lib/x86_64-linux-gnu/libm.so.6' },
      { name: 'libgone.so.1', path: undefined },
    ])
    expect(isUnixSystemLibrary('/lib/x86_64-linux-gnu/libm.so.6', '/home/me/.local/share/SketchDev/orca-engine')).toBe(true)
    expect(isUnixSystemLibrary('/home/me/.local/share/SketchDev/orca-engine/deps/x.so', '/home/me/.local/share/SketchDev/orca-engine')).toBe(false)
    expect(isUnixSystemLibrary('/home/me/.local/share/SketchDev/orca-engine-old/x.so', '/home/me/.local/share/SketchDev/orca-engine')).toBe(true)
  })

  test('otool -L output: the install names after the first line', () => {
    const text = [
      '/w/build/sketch-orca/sketch-orca:',
      '\t@rpath/libTKernel.7.6.dylib (compatibility version 7.6.0, current version 7.6.0)',
      '\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 1700.255.0)',
      '\t/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 2420.0.0)',
    ].join('\n')
    expect(parseOtoolL(text)).toEqual(['@rpath/libTKernel.7.6.dylib', '/usr/lib/libc++.1.dylib', '/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation'])
  })
})

describe('running the staged engine', () => {
  test('parseVersionOutput takes the JSON line, even after log lines', () => {
    const json = '{"engine":"sketch-orca","engineVersion":"0.1.0","orcaVersion":"2.4.2","orcaCommit":"x"}'
    expect(parseVersionOutput(json)).toEqual(JSON.parse(json))
    expect(parseVersionOutput(`[info] loading\r\n${json}\r\n`)).toEqual(JSON.parse(json))
    expect(parseVersionOutput('not json')).toBeUndefined()
    expect(parseVersionOutput('[1,2]')).toBeUndefined()
  })

  test('isolatedEnv: the system folders only', () => {
    expect(isolatedEnv({ SystemRoot: 'C:\\Windows', TEMP: 'T', Path: 'C:\\deps\\bin;C:\\Windows', SECRET: 'x' }, 'win32')).toEqual({
      SystemRoot: 'C:\\Windows',
      windir: 'C:\\Windows',
      PATH: `${join('C:\\Windows', 'System32')};C:\\Windows`,
      TEMP: 'T',
    })
    expect(isolatedEnv({ HOME: '/home/me', LD_LIBRARY_PATH: '/w/lib' }, 'linux')).toEqual({ PATH: '/usr/bin:/bin', HOME: '/home/me', TMPDIR: '/tmp' })
  })

  test("stagedEnv: the system folders, plus the engine's bin/ as the library path on Linux and macOS", () => {
    const base = { SystemRoot: 'C:\\Windows', Path: 'C:\\deps\\bin;C:\\Windows', HOME: '/home/me', LD_LIBRARY_PATH: '/w/lib', DYLD_LIBRARY_PATH: '/w/lib' }
    expect(stagedEnv(base, 'win32', 'C:\\dist\\engine\\bin')).toEqual(isolatedEnv(base, 'win32'))
    expect(stagedEnv(base, 'linux', '/dist/engine/bin')).toEqual({ PATH: '/usr/bin:/bin', HOME: '/home/me', TMPDIR: '/tmp', LD_LIBRARY_PATH: '/dist/engine/bin' })
    expect(stagedEnv(base, 'darwin', '/dist/engine/bin')).toEqual({ PATH: '/usr/bin:/bin', HOME: '/home/me', TMPDIR: '/tmp', DYLD_LIBRARY_PATH: '/dist/engine/bin' })
  })

  test('the staged copy slices smoke cases: a plain G-code printer and a Bambu printer with its .gcode.3mf', () => {
    const ids = SMOKE_CASES.map((c) => c.id)
    for (const id of STAGED_CASES) expect(ids).toContain(id)
    expect([...STAGED_CASES]).toEqual(['cube-fixed', 'bambu-3mf'])
  })
})
