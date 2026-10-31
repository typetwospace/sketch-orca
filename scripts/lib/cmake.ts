// Command lines for the two CMake builds: Orca's dependency bundle (`deps/`) and Orca itself
// (libslic3r plus our front end), both Release, both with the GUI off.

import { cpus, totalmem } from 'node:os'
import type { Platform } from './env.ts'

/** CMake wants forward slashes in -D values, also on Windows. */
export function cmakePath(p: string): string {
  return p.replace(/\\/g, '/')
}

export interface DepsArgs {
  ninja: string
  /** `<src>/deps` */
  sourceDir: string
  buildDir: string
  /** Where ExternalProject keeps the downloaded source packages. */
  downloadDir: string
  ignorePrefixes: string[]
  platform: Platform
}

export function depsConfigureArgs(a: DepsArgs): string[] {
  const args = [
    '-S', a.sourceDir,
    '-B', a.buildDir,
    '-G', 'Ninja',
    `-DCMAKE_MAKE_PROGRAM=${cmakePath(a.ninja)}`,
    '-DCMAKE_BUILD_TYPE=Release',
    `-DDEP_DOWNLOAD_DIR=${cmakePath(a.downloadDir)}`,
    // Our patch 0001: skip wxWidgets, GLEW, GLFW, OpenCSG and CURL.
    '-DDEP_WITH_GUI=OFF',
    // Also patch 0001: OpenCV without Intel's IPP-ICV, which OpenCV's build would download.
    '-DDEP_OPENCV_IPP=OFF',
  ]
  if (a.platform === 'win32') args.push('-DCMAKE_C_COMPILER=cl', '-DCMAKE_CXX_COMPILER=cl')
  // Orca's deps pass CMAKE_IGNORE_PREFIX_PATH on to every dependency; one prefix only, since
  // a list would be split by ExternalProject.
  if (a.ignorePrefixes.length > 1) throw new Error(`only one ignored prefix can be passed to Orca's deps, got ${a.ignorePrefixes.join(', ')}`)
  if (a.ignorePrefixes.length === 1) args.push(`-DCMAKE_IGNORE_PREFIX_PATH=${cmakePath(a.ignorePrefixes[0]!)}`)
  return args
}

export function depsBuildArgs(buildDir: string, jobs: number): string[] {
  return ['--build', buildDir, '--target', 'deps', '--', `-j${jobs}`]
}

export interface OrcaArgs {
  ninja: string
  sourceDir: string
  buildDir: string
  /** The dependency bundle: `<deps build>/OrcaSlicer_dep/usr/local`. */
  depsPrefix: string
  /** Our front end's CMake folder (`cli/` of this repository, `engine/cli` of a source bundle). */
  cliDir: string
  commit: string
  tag: string
  ignorePrefixes: string[]
  platform: Platform
}

export function orcaConfigureArgs(a: OrcaArgs): string[] {
  const args = [
    '-S', a.sourceDir,
    '-B', a.buildDir,
    '-G', 'Ninja',
    `-DCMAKE_MAKE_PROGRAM=${cmakePath(a.ninja)}`,
    '-DCMAKE_BUILD_TYPE=Release',
    '-DSLIC3R_GUI=OFF',
    '-DSLIC3R_STATIC=ON',
    '-DSLIC3R_PCH=ON',
    '-DBUILD_TESTS=OFF',
    '-DORCA_TOOLS=OFF',
    `-DCMAKE_PREFIX_PATH=${cmakePath(a.depsPrefix)}`,
    `-DSKETCH_ORCA_CLI_DIR=${cmakePath(a.cliDir)}`,
    `-DSKETCH_ORCA_COMMIT=${a.commit}`,
    `-DSKETCH_ORCA_TAG=${a.tag}`,
  ]
  if (a.platform === 'win32') args.push('-DCMAKE_C_COMPILER=cl', '-DCMAKE_CXX_COMPILER=cl')
  if (a.ignorePrefixes.length > 0) args.push(`-DCMAKE_IGNORE_PREFIX_PATH=${a.ignorePrefixes.map(cmakePath).join(';')}`)
  return args
}

export function orcaBuildArgs(buildDir: string, jobs: number): string[] {
  return ['--build', buildDir, '--target', 'sketch-orca', '--', `-j${jobs}`]
}

/**
 * Parallel compile jobs for the Orca build: one per core, but no more than one per 2.5 GB of
 * memory (libslic3r's CGAL units need a lot each). SKETCH_ORCA_JOBS overrides.
 */
export function defaultJobs(env: NodeJS.ProcessEnv = process.env, cores = cpus().length, memBytes = totalmem()): number {
  const forced = Number(env.SKETCH_ORCA_JOBS)
  if (Number.isInteger(forced) && forced > 0) return forced
  const byMem = Math.floor(memBytes / (2.5 * 1024 ** 3))
  return Math.max(1, Math.min(cores, byMem))
}

/**
 * Dependencies built at the same time. Each one already builds in parallel (MSBuild /m or
 * make -jN), so the top level runs only a few. SKETCH_ORCA_DEPS_JOBS overrides.
 */
export function defaultDepsJobs(env: NodeJS.ProcessEnv = process.env): number {
  const forced = Number(env.SKETCH_ORCA_DEPS_JOBS)
  if (Number.isInteger(forced) && forced > 0) return forced
  return 3
}

/** For logs: a command line with quoted arguments where needed. */
export function formatCommand(exe: string, args: string[]): string {
  const q = (s: string) => (/[\s"]/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s)
  return [exe, ...args].map(q).join(' ')
}
