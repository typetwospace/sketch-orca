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
  /**
   * Windows: the MSYS2 tree with the MinGW-w64 toolchain (mingw.ts mingwRoot), with which patch 0007
   * builds GMP and MPFR from their source packages instead of copying Orca's prebuilt DLLs.
   */
  mingwRoot?: string
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
  // Patch 0005: every dependency with MSVC's static runtime (/MT), as the engine (orcaConfigureArgs).
  if (a.platform === 'win32') args.push('-DCMAKE_C_COMPILER=cl', '-DCMAKE_CXX_COMPILER=cl', '-DDEP_MSVC_STATIC_RUNTIME=ON')
  // Patch 0007: GMP and MPFR built from their source packages with MinGW-w64, never the prebuilt
  // DLLs of Orca's tree, whose build recipe is not published.
  if (a.platform === 'win32') {
    if (!a.mingwRoot) throw new Error('a Windows dependency build needs the MinGW-w64 toolchain for GMP and MPFR (mingwRoot)')
    args.push(`-DDEP_MINGW_ROOT=${cmakePath(a.mingwRoot)}`)
  }
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
  if (a.platform === 'win32') {
    args.push('-DCMAKE_C_COMPILER=cl', '-DCMAKE_CXX_COMPILER=cl')
    // MSVC's static runtime (/MT), like the dependencies (patch 0005): the engine and the DLLs it
    // loads import no Visual C++ runtime DLL, so none ships (stage refuses one that does). CMP0091
    // NEW, since Orca's cmake_minimum_required (3.13) predates the setting.
    args.push('-DCMAKE_POLICY_DEFAULT_CMP0091=NEW', '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded')
  }
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

/** SKETCH_ORCA_JOBS as a cap on compile jobs, or undefined when it is not set to a whole number above 0. */
export function jobsCap(env: NodeJS.ProcessEnv = process.env): number | undefined {
  const forced = Number(env.SKETCH_ORCA_JOBS)
  return Number.isInteger(forced) && forced > 0 ? forced : undefined
}

/**
 * Dependencies built at the same time. Each one already builds in parallel (MSBuild /m or
 * make -jN), so the top level runs only a few: 3, or 1 when SKETCH_ORCA_JOBS caps the compile
 * jobs (depsParallelEnv). SKETCH_ORCA_DEPS_JOBS overrides.
 */
export function defaultDepsJobs(env: NodeJS.ProcessEnv = process.env): number {
  const forced = Number(env.SKETCH_ORCA_DEPS_JOBS)
  if (Number.isInteger(forced) && forced > 0) return forced
  return jobsCap(env) === undefined ? 3 : 1
}

/**
 * The environment that holds the dependency build to SKETCH_ORCA_JOBS compile jobs; empty when it
 * is not set, and each dependency then builds with every core (Orca's MSBuild /m, make -j<cores>).
 * Orca's deps/CMakeLists.txt leaves the parallel level of each dependency's build to
 * CMAKE_BUILD_PARALLEL_LEVEL when that is set. On Windows MSBuild then builds one project at a
 * time, and cl compiles up to <jobs> files of it at once: /MP<jobs> through _CL_, which cl adds to
 * every command line, and CL_MPCount, which MSBuild gives the projects that ask for /MP
 * themselves; GMP and MPFR's make runs <jobs> jobs (DEP_MINGW_JOBS, patch 0007's build.sh).
 * Elsewhere make runs <jobs> jobs. With one dependency at a time (defaultDepsJobs), no more than
 * <jobs> compilers run. None of it changes what is compiled, so it is not part of the build's
 * inputs.
 */
export function depsParallelEnv(platform: Platform, env: NodeJS.ProcessEnv = process.env): Record<string, string> {
  const jobs = jobsCap(env)
  if (jobs === undefined) return {}
  if (platform === 'win32') return { CMAKE_BUILD_PARALLEL_LEVEL: '1', CL_MPCount: String(jobs), _CL_: `/MP${jobs}`, DEP_MINGW_JOBS: String(jobs) }
  return { CMAKE_BUILD_PARALLEL_LEVEL: String(jobs) }
}

/** For logs: a command line with quoted arguments where needed. */
export function formatCommand(exe: string, args: string[]): string {
  const q = (s: string) => (/[\s"]/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s)
  return [exe, ...args].map(q).join(' ')
}
