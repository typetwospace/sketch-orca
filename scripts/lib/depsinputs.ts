// What the dependency build is of, worked out the same way by build, stage and source
// (sketch-orca, 2026-11-01): the locked commit, the patches of deps/, the configuration, CMake and
// the compiler settings, and on Windows the pinned MinGW-w64 toolchain that builds GMP and MPFR
// (mingw.ts). build records it with the dependency build (state/deps.json); stage takes the
// dependencies only when the record is of these inputs, as they are now, and writes them into the
// staged manifest; source takes the staged engine only when its manifest names these inputs, so
// the toolchain the bundle names is the one the engine's dependencies were built with.

import { join } from 'node:path'
import { depsConfigureArgs } from './cmake.ts'
import { sourcePathTrimEnv } from './env.ts'
import type { OrcaLock } from './lock.ts'
import { mingwPinsDigest, mingwRoot } from './mingw.ts'
import { sha256Text, type WorkPaths } from './paths.ts'
import type { Toolchain } from './toolchain.ts'

/** The parts of the toolchain the dependency build's inputs name. */
export type DepsTools = Pick<Toolchain, 'platform' | 'cmake' | 'ninja' | 'ignorePrefixes'>

/**
 * The compiler settings of both builds that come from the environment: the work folder and the
 * engine's checkout (`engineDir`) are trimmed from __FILE__, so the binaries do not carry the
 * builder's user name (env.ts, sourcePathTrimEnv).
 */
export function compilerEnv(t: Pick<Toolchain, 'platform'>, w: WorkPaths, engineDir: string): Record<string, string> {
  return sourcePathTrimEnv(t.platform, [w.home, engineDir])
}

/** The command line that configures the dependency build. */
export function depsConfigure(t: DepsTools, w: WorkPaths): string[] {
  return depsConfigureArgs({
    ninja: t.ninja,
    sourceDir: join(w.src, 'deps'),
    buildDir: w.deps,
    downloadDir: w.downloads,
    ignorePrefixes: t.ignorePrefixes,
    platform: t.platform,
    ...(t.platform === 'win32' ? { mingwRoot: mingwRoot(w) } : {}),
  })
}

export interface DepsWant {
  commit: string
  depsPatches: { name: string; sha256: string }[]
  /** The digest of everything the build is of: state/deps.json's inputs. */
  inputs: string
  /** The digest of the MinGW-w64 pins (mingwPinsDigest) on Windows; null elsewhere. */
  mingw: string | null
}

/**
 * What the dependency build is of, as build works it out now. The engine is built, staged and
 * bundled only on a finished dependency build of these (depsStampProblems).
 */
export function depsWant(lock: Pick<OrcaLock, 'commit'>, w: WorkPaths, t: DepsTools, engineDir: string, depsPatches: { name: string; sha256: string }[]): DepsWant {
  const patchesHash = sha256Text(JSON.stringify(depsPatches.map((p) => [p.name, p.sha256])))
  const mingw = t.platform === 'win32' ? mingwPinsDigest() : null
  const inputs = sha256Text(
    JSON.stringify({
      commit: lock.commit,
      patchesHash,
      configure: depsConfigure(t, w),
      cmake: t.cmake,
      compilerEnv: compilerEnv(t, w, engineDir),
      ...(mingw === null ? {} : { mingw }),
    }),
  )
  return { commit: lock.commit, depsPatches: depsPatches.map((p) => ({ name: p.name, sha256: p.sha256 })), inputs, mingw }
}

/**
 * How the dependency inputs a staged manifest names (`staged`, undefined for a manifest from
 * before they were recorded) differ from `now`: empty when they are the same.
 */
export function stagedDepsProblems(staged: { inputs: string; mingw: string | null } | undefined, now: DepsWant): string[] {
  if (!staged) return ['the staged engine does not name the inputs of its dependency build (an earlier stage)']
  if (staged.inputs === now.inputs) return []
  if (staged.mingw !== now.mingw) return ['the staged engine\'s GMP and MPFR were built with other MinGW-w64 pins than scripts/lib/mingw.ts has now']
  return ['the staged engine\'s dependencies were built from other inputs than build works out now (their configuration, CMake, the compiler settings or the patches of deps/ changed since)']
}
