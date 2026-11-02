import { describe, expect, test } from 'bun:test'
import { join } from 'node:path'
import { depsConfigureArgs } from '../scripts/lib/cmake.ts'
import { compilerEnv, depsConfigure, depsWant, stagedDepsProblems, type DepsTools } from '../scripts/lib/depsinputs.ts'
import { sourcePathTrimEnv } from '../scripts/lib/env.ts'
import { MINGW_PACKAGES, mingwPinsDigest, mingwRoot, MSYS2_BASE } from '../scripts/lib/mingw.ts'
import { sha256Text, workPaths } from '../scripts/lib/paths.ts'

const LOCK = { commit: '8500fcdccaa10b5099ac20d252af3a7c560046f1' }
const W = workPaths('C:\\w')
const ENGINE = 'C:\\src\\sketch-orca'
const WIN: DepsTools = { platform: 'win32', cmake: 'C:\\cmake\\bin\\cmake.exe', ninja: 'C:\\ninja\\ninja.exe', ignorePrefixes: ['C:/Strawberry/c'] }
const PATCHES = [{ name: '0001-deps-without-gui.patch', sha256: '1'.repeat(64) }]

describe('the dependency build inputs build, stage and source share', () => {
  test('depsWant: the digest build has always recorded, so a dependency build on record stays current', () => {
    const want = depsWant(LOCK, W, WIN, ENGINE, PATCHES)
    const configure = depsConfigureArgs({ ninja: WIN.ninja, sourceDir: join(W.src, 'deps'), buildDir: W.deps, downloadDir: W.downloads, ignorePrefixes: WIN.ignorePrefixes, platform: 'win32', mingwRoot: mingwRoot(W) })
    expect(depsConfigure(WIN, W)).toEqual(configure)
    expect(compilerEnv(WIN, W, ENGINE)).toEqual(sourcePathTrimEnv('win32', [W.home, ENGINE]))
    const patchesHash = sha256Text(JSON.stringify(PATCHES.map((p) => [p.name, p.sha256])))
    expect(want).toEqual({
      commit: LOCK.commit,
      depsPatches: PATCHES,
      inputs: sha256Text(JSON.stringify({ commit: LOCK.commit, patchesHash, configure, cmake: WIN.cmake, compilerEnv: sourcePathTrimEnv('win32', [W.home, ENGINE]), mingw: mingwPinsDigest() })),
      mingw: mingwPinsDigest(),
    })
    // Off Windows there is no MinGW-w64 toolchain, and no key for it.
    const linux = depsWant(LOCK, workPaths('/w'), { ...WIN, platform: 'linux', cmake: '/usr/bin/cmake', ninja: '/usr/bin/ninja', ignorePrefixes: [] }, '/src', PATCHES)
    expect(linux.mingw).toBeNull()
    const linuxConfigure = depsConfigureArgs({ ninja: '/usr/bin/ninja', sourceDir: join('/w', 'src', 'deps'), buildDir: workPaths('/w').deps, downloadDir: workPaths('/w').downloads, ignorePrefixes: [], platform: 'linux' })
    expect(linux.inputs).toBe(sha256Text(JSON.stringify({ commit: LOCK.commit, patchesHash, configure: linuxConfigure, cmake: '/usr/bin/cmake', compilerEnv: {} })))
  })

  test('depsWant: changes with each input, the MinGW-w64 pins included', () => {
    const base = depsWant(LOCK, W, WIN, ENGINE, PATCHES).inputs
    for (const other of [
      depsWant({ commit: 'f'.repeat(40) }, W, WIN, ENGINE, PATCHES),
      depsWant(LOCK, workPaths('C:\\x'), WIN, ENGINE, PATCHES),
      depsWant(LOCK, W, { ...WIN, cmake: 'C:\\other\\cmake.exe' }, ENGINE, PATCHES),
      depsWant(LOCK, W, { ...WIN, ninja: 'C:\\other\\ninja.exe' }, ENGINE, PATCHES),
      depsWant(LOCK, W, { ...WIN, ignorePrefixes: [] }, ENGINE, PATCHES),
      depsWant(LOCK, W, WIN, 'C:\\elsewhere', PATCHES),
      depsWant(LOCK, W, WIN, ENGINE, []),
    ])
      expect(other.inputs).not.toBe(base)
    // The pins' digest names every pinned package by file, size and SHA-256.
    expect(mingwPinsDigest(MSYS2_BASE, MINGW_PACKAGES.slice(1))).not.toBe(mingwPinsDigest())
    expect(mingwPinsDigest(MSYS2_BASE, MINGW_PACKAGES.map((p, i) => (i === 0 ? { ...p, sha256: '0'.repeat(64) } : p)))).not.toBe(mingwPinsDigest())
  })

  test('stagedDepsProblems: a staged engine is taken only with the inputs there are now', () => {
    const now = depsWant(LOCK, W, WIN, ENGINE, PATCHES)
    expect(stagedDepsProblems({ inputs: now.inputs, mingw: now.mingw }, now)).toEqual([])
    expect(stagedDepsProblems(undefined, now)).toEqual(['the staged engine does not name the inputs of its dependency build (an earlier stage)'])
    expect(stagedDepsProblems({ inputs: 'a'.repeat(64), mingw: 'b'.repeat(64) }, now)).toEqual(["the staged engine's GMP and MPFR were built with other MinGW-w64 pins than scripts/lib/mingw.ts has now"])
    expect(stagedDepsProblems({ inputs: 'a'.repeat(64), mingw: now.mingw }, now)[0]).toContain('other inputs')
  })
})
