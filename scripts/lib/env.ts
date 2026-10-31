// The build environment: which tools, and a PATH that holds only them.
//
// On Windows the build runs inside the MSVC x64 environment (vcvars64.bat), with Strawberry
// Perl's perl.exe for OpenSSL's Configure and git for the dependency patches. Two things on a
// typical machine break it and are kept off the PATH: Strawberry's MinGW folder (`C:\Strawberry\c\bin`,
// gcc, mingw32-make and its own libraries) and the MSYS tools that ship with Git (`usr\bin`, whose
// perl is not a Windows perl).

import { existsSync } from 'node:fs'
import { dirname, join, posix } from 'node:path'

export type Platform = 'win32' | 'linux' | 'darwin'

/** Lower-case, backslashes, no trailing separator: for comparing Windows directories. */
export function normWinDir(dir: string): string {
  let d = dir.trim().replace(/\//g, '\\').toLowerCase()
  while (d.length > 3 && d.endsWith('\\')) d = d.slice(0, -1)
  return d
}

export function splitPathList(value: string | undefined, platform: Platform): string[] {
  if (!value) return []
  return value.split(platform === 'win32' ? ';' : ':').map((s) => s.trim()).filter((s) => s !== '')
}

/**
 * Directories that must never be on the build PATH on Windows:
 * - Strawberry's MinGW toolchain folder (`...\Strawberry\c\bin`) and anything else under `...\Strawberry\c`;
 * - Git's MSYS folders (`...\Git\usr\bin`, `...\Git\mingw64\bin`, `...\Git\bin`), whose perl is an MSYS perl;
 * - MSYS2 and Cygwin installs.
 */
export function isBannedWinDir(dir: string): boolean {
  const d = normWinDir(dir)
  if (/\\strawberry\\c(\\|$)/.test(d)) return true
  if (/\\git\\(usr|mingw64|mingw32|clangarm64)(\\|$)/.test(d)) return true
  if (/\\git\\bin$/.test(d)) return true
  if (/(^|\\)(msys64|msys32|msys2|cygwin|cygwin64)(\\|$)/.test(d)) return true
  return false
}

/**
 * The build PATH on Windows: the vcvars entries and the system folders, without banned
 * folders and duplicates (first one wins), then the extra tool folders (perl, git).
 */
export function cleanWinBuildPath(vcvarsPath: string[], extraDirs: string[]): string[] {
  const out: string[] = []
  const seen = new Set<string>()
  const add = (dir: string) => {
    const key = normWinDir(dir)
    if (key === '' || seen.has(key) || isBannedWinDir(dir)) return
    seen.add(key)
    out.push(/^[A-Za-z]:[\\/]?$/.test(dir.trim()) ? dir.trim() : dir.trim().replace(/[\\/]+$/, ''))
  }
  for (const dir of vcvarsPath) add(dir)
  for (const dir of extraDirs) {
    if (isBannedWinDir(dir)) throw new Error(`refusing to put ${dir} on the build PATH`)
    add(dir)
  }
  return out
}

/**
 * The build PATH on Linux and macOS (not yet tried): the folders of the tools found, then the
 * system's (Homebrew's on macOS, for autoconf, automake, libtool and m4), without duplicates.
 */
export function unixBuildPath(platform: Platform, toolDirs: string[]): string[] {
  const system = [...(platform === 'darwin' ? ['/opt/homebrew/bin'] : []), '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin']
  return [...new Set([...toolDirs, ...system].map((d) => (d.length > 1 ? d.replace(/\/+$/, '') : d)))]
}

/**
 * Programs the dependency build runs by name on Linux and macOS: the C and C++ compilers (CMake's
 * defaults), make, and the autotools GMP and MPFR are configured with. Each entry lists accepted names.
 */
export function unixBuildPrograms(platform: Platform): string[][] {
  return [['cc'], ['c++'], ['make'], ['m4'], ['autoreconf'], ['automake'], platform === 'darwin' ? ['glibtoolize', 'libtoolize'] : ['libtoolize']]
}

/** The first `dir/name` that exists, over the folders in order (POSIX paths: Linux and macOS). */
export function findOnPath(names: string[], dirs: string[], exists: (p: string) => boolean = existsSync): string | undefined {
  for (const d of dirs) {
    for (const n of names) {
      const p = posix.join(d, n)
      if (exists(p)) return p
    }
  }
  return undefined
}

/** The minimal PATH vcvars64.bat is run with, so its result holds only its own entries and these. */
export function winBaselinePath(systemRoot: string): string[] {
  return [join(systemRoot, 'System32'), systemRoot, join(systemRoot, 'System32', 'Wbem'), join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0')]
}

/** Parses the output of `set` (cmd.exe): `NAME=value` lines; the `=C:` style entries are skipped. */
export function parseSetOutput(text: string): Record<string, string> {
  const env: Record<string, string> = {}
  for (const raw of text.split(/\r?\n/)) {
    if (raw === '' || raw.startsWith('=')) continue
    const eq = raw.indexOf('=')
    if (eq <= 0) continue
    env[raw.slice(0, eq)] = raw.slice(eq + 1)
  }
  return env
}

/**
 * Variables that would steer CMake, compilers or perl away from what the build chose. Every
 * <Package>_ROOT is dropped too (LEAKY_ENV_SUFFIXES): find_package reads them.
 */
export const LEAKY_ENV = [
  'CC', 'CXX', 'CFLAGS', 'CXXFLAGS', 'CPPFLAGS', 'LDFLAGS', 'LIBS',
  // MSVC's own: options cl.exe and link.exe add to every command line.
  'CL', '_CL_', 'LINK', '_LINK_',
  'CMAKE_GENERATOR', 'CMAKE_GENERATOR_PLATFORM', 'CMAKE_GENERATOR_TOOLSET', 'CMAKE_GENERATOR_INSTANCE',
  'CMAKE_PREFIX_PATH', 'CMAKE_TOOLCHAIN_FILE', 'CMAKE_BUILD_TYPE', 'CMAKE_CONFIGURATION_TYPES',
  'CMAKE_INSTALL_PREFIX', 'CMAKE_BUILD_PARALLEL_LEVEL', 'CMAKE_APPBUNDLE_PATH', 'CMAKE_FRAMEWORK_PATH',
  'CMAKE_LIBRARY_PATH', 'CMAKE_INCLUDE_PATH', 'CMAKE_PROGRAM_PATH', 'DESTDIR',
  'PERL5LIB', 'PERL5OPT', 'PERLLIB', 'PERL_MB_OPT', 'PERL_MM_OPT', 'PERL_LOCAL_LIB_ROOT',
  'MSYSTEM', 'MSYS', 'CHERE_INVOKING', 'OPENSSL_CONF', 'OPENSSL_DIR', 'BOOST_ROOT', 'BOOSTROOT',
  'TBBROOT', 'EIGEN3_ROOT', 'CGAL_DIR', 'WXWIN', 'VCPKG_ROOT', 'Qt5_DIR', 'Qt6_DIR', 'PKG_CONFIG_PATH',
  'GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE',
]

/** Name endings of variables that steer the build: find_package reads <Package>_ROOT. */
export const LEAKY_ENV_SUFFIXES = ['_ROOT']

/** Whether `name` is a LEAKY_ENV variable or ends in a LEAKY_ENV_SUFFIXES (case-insensitively on Windows). */
export function isLeakyEnv(name: string, platform: Platform): boolean {
  const key = platform === 'win32' ? name.toUpperCase() : name
  if (LEAKY_ENV.some((k) => (platform === 'win32' ? k.toUpperCase() : k) === key)) return true
  return LEAKY_ENV_SUFFIXES.some((s) => key.endsWith(s) && key.length > s.length)
}

/**
 * What vcvars64.bat replaces: it appends its own folders to INCLUDE, LIB, LIBPATH and
 * EXTERNAL_INCLUDE, so a caller's values would stay in the build's search paths.
 */
export const VCVARS_REPLACES = ['INCLUDE', 'LIB', 'LIBPATH', 'EXTERNAL_INCLUDE']

/** The environment vcvars64.bat starts from: the caller's, without PATH, VCVARS_REPLACES and the leaky variables, with `path` as Path. */
export function vcvarsBaseEnv(env: NodeJS.ProcessEnv, path: string[]): Record<string, string> {
  const base: Record<string, string> = {}
  for (const [k, v] of Object.entries(env)) {
    const key = k.toUpperCase()
    if (v === undefined || key === 'PATH' || VCVARS_REPLACES.includes(key) || isLeakyEnv(k, 'win32')) continue
    base[k] = v
  }
  base.Path = path.join(';')
  return base
}

/** Removes the leaky variables (LEAKY_ENV and LEAKY_ENV_SUFFIXES, case-insensitively on Windows) and sets PATH to `path`. */
export function finishBuildEnv(env: Record<string, string>, path: string[], platform: Platform): Record<string, string> {
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(env)) {
    const key = platform === 'win32' ? k.toUpperCase() : k
    if (isLeakyEnv(k, platform) || key === 'PATH') continue
    out[k] = v
  }
  out.PATH = path.join(platform === 'win32' ? ';' : ':')
  // Orca's own scripts set this for CMake 4; older CMake ignores it.
  out.CMAKE_POLICY_VERSION_MINIMUM = '3.5'
  return out
}

/**
 * What keeps the build machine's folders out of the binaries. __FILE__, which Boost, NLopt and
 * OCCT put into their assertion and error messages, is a source file's full path, and with it the
 * builder's user name. MSVC (Windows): /d1trimfile:"<prefix>\" for each prefix, longest first, in
 * CL, which cl.exe adds to every command line (CMake's, Boost's b2 and nmake alike); the doubled
 * backslash keeps the closing quote, and the quotes keep a path with spaces in one piece. Linux
 * and macOS (not yet tried): nothing; build there in a work folder whose path holds no user name.
 * Ninja does not see the environment, so the pipeline starts a build folder over when this
 * changes (orca.ts).
 */
export function sourcePathTrimEnv(platform: Platform, prefixes: string[]): Record<string, string> {
  if (platform !== 'win32') return {}
  const dirs = [...new Set(prefixes.map((p) => p.replace(/\//g, '\\').replace(/\\+$/, '')).filter((p) => p !== ''))]
  dirs.sort((a, b) => b.length - a.length || (a < b ? -1 : a > b ? 1 : 0))
  return dirs.length === 0 ? {} : { CL: dirs.map((d) => `/d1trimfile:"${d}\\\\"`).join(' ') }
}

/** First path for which `exists` is true. */
export function firstExisting(candidates: (string | undefined)[], exists: (p: string) => boolean = existsSync): string | undefined {
  for (const c of candidates) if (c && exists(c)) return c
  return undefined
}

/**
 * CMake adds its own install prefix (the folder above `bin`) to its search prefixes. That is
 * harmless for a CMake-only install but not for one inside a toolchain folder such as
 * `C:\Strawberry\c`, whose `include` and `lib` hold MinGW libraries; such a prefix is returned
 * for CMAKE_IGNORE_PREFIX_PATH.
 */
export function cmakeIgnorePrefixes(cmakeExe: string, exists: (p: string) => boolean = existsSync): string[] {
  const prefix = dirname(dirname(cmakeExe))
  if (exists(join(prefix, 'include')) || exists(join(prefix, 'lib'))) return [prefix.replace(/\\/g, '/')]
  return []
}

/** Candidate locations of CMake, in order of preference. */
export function cmakeCandidates(platform: Platform, vsInstall: string | undefined, env: NodeJS.ProcessEnv): string[] {
  const c: string[] = []
  if (env.SKETCH_ORCA_CMAKE) c.push(env.SKETCH_ORCA_CMAKE)
  if (platform === 'win32') {
    if (vsInstall) c.push(join(vsInstall, 'Common7', 'IDE', 'CommonExtensions', 'Microsoft', 'CMake', 'CMake', 'bin', 'cmake.exe'))
    const pf = env.ProgramFiles ?? 'C:\\Program Files'
    c.push(join(pf, 'CMake', 'bin', 'cmake.exe'))
    // Last resort: the CMake that Strawberry Perl ships. It is run by absolute path; its folder
    // stays off the PATH and its prefix is ignored by find_* (see cmakeIgnorePrefixes).
    c.push('C:\\Strawberry\\c\\bin\\cmake.exe')
  } else {
    c.push('/usr/local/bin/cmake', '/usr/bin/cmake', '/opt/homebrew/bin/cmake')
  }
  return c
}

export function ninjaCandidates(platform: Platform, vsInstall: string | undefined, env: NodeJS.ProcessEnv): string[] {
  const c: string[] = []
  if (env.SKETCH_ORCA_NINJA) c.push(env.SKETCH_ORCA_NINJA)
  if (platform === 'win32') {
    if (vsInstall) c.push(join(vsInstall, 'Common7', 'IDE', 'CommonExtensions', 'Microsoft', 'CMake', 'Ninja', 'ninja.exe'))
    const pf = env.ProgramFiles ?? 'C:\\Program Files'
    c.push(join(pf, 'CMake', 'bin', 'ninja.exe'))
    c.push('C:\\Strawberry\\c\\bin\\ninja.exe')
  } else {
    c.push('/usr/local/bin/ninja', '/usr/bin/ninja', '/opt/homebrew/bin/ninja')
  }
  return c
}

export function perlCandidates(platform: Platform, env: NodeJS.ProcessEnv): string[] {
  const c: string[] = []
  if (env.SKETCH_ORCA_PERL) c.push(env.SKETCH_ORCA_PERL)
  if (platform === 'win32') c.push('C:\\Strawberry\\perl\\bin\\perl.exe')
  else c.push('/usr/bin/perl', '/usr/local/bin/perl')
  return c
}

export function gitCandidates(platform: Platform, env: NodeJS.ProcessEnv): string[] {
  const c: string[] = []
  if (env.SKETCH_ORCA_GIT) c.push(env.SKETCH_ORCA_GIT)
  if (platform === 'win32') {
    const pf = env.ProgramFiles ?? 'C:\\Program Files'
    c.push(join(pf, 'Git', 'cmd', 'git.exe'))
  } else {
    c.push('/usr/bin/git', '/usr/local/bin/git', '/opt/homebrew/bin/git')
  }
  return c
}
