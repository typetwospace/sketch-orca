// Finds the build tools and makes the build environment (see env.ts for the PATH rules).

import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import {
  cleanWinBuildPath,
  cmakeCandidates,
  cmakeIgnorePrefixes,
  findOnPath,
  finishBuildEnv,
  firstExisting,
  gitCandidates,
  ninjaCandidates,
  parseSetOutput,
  perlCandidates,
  splitPathList,
  unixBuildPath,
  unixBuildPrograms,
  vcvarsBaseEnv,
  winBaselinePath,
  type Platform,
} from './env.ts'
import { capture, StepError } from './run.ts'

export interface Toolchain {
  platform: Platform
  vsInstall?: string
  vcvars?: string
  cmake: string
  ninja: string
  perl: string
  git: string
  /** Prefixes CMake must not search (see cmakeIgnorePrefixes). */
  ignorePrefixes: string[]
  env: Record<string, string>
}

export function currentPlatform(): Platform {
  const p = process.platform
  if (p === 'win32' || p === 'linux' || p === 'darwin') return p
  throw new StepError(`unsupported platform ${p}`)
}

/** Visual Studio 2022 (17.x) with the x64 C++ tools, by vswhere. */
export function findVisualStudio(env: NodeJS.ProcessEnv): string | undefined {
  if (env.SKETCH_ORCA_VS) return env.SKETCH_ORCA_VS
  const pf86 = env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)'
  const vswhere = join(pf86, 'Microsoft Visual Studio', 'Installer', 'vswhere.exe')
  if (!existsSync(vswhere)) return undefined
  const r = capture(vswhere, ['-version', '[17.0,18.0)', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-latest', '-property', 'installationPath'])
  const path = r.stdout.trim().split(/\r?\n/)[0]?.trim()
  return path ? path : undefined
}

/** Runs vcvars64.bat in a cmd.exe with a minimal PATH and returns the resulting environment. */
export function vcvarsEnvironment(vcvars: string, env: NodeJS.ProcessEnv): Record<string, string> {
  const systemRoot = env.SystemRoot ?? env.SYSTEMROOT ?? 'C:\\Windows'
  // Without the caller's INCLUDE, LIB, LIBPATH and EXTERNAL_INCLUDE (vcvars appends its own to
  // them) and without the variables that steer compilers and CMake (env.ts, LEAKY_ENV).
  const base = vcvarsBaseEnv(env, winBaselinePath(systemRoot))
  const cmdExe = join(systemRoot, 'System32', 'cmd.exe')
  const r = capture(cmdExe, ['/d', '/s', '/c', `"call "${vcvars}" >nul 2>&1 && set"`], { env: base, verbatim: true })
  if (r.code !== 0) throw new StepError(`vcvars64.bat failed (${r.code}): ${r.stderr.trim()}`)
  const out = parseSetOutput(r.stdout)
  if (!Object.keys(out).some((k) => k.toUpperCase() === 'VCTOOLSINSTALLDIR')) throw new StepError('vcvars64.bat did not set up the MSVC environment')
  return out
}

/** The toolchain for this machine; throws a StepError naming what is missing. */
export function findToolchain(env: NodeJS.ProcessEnv = process.env): Toolchain {
  const platform = currentPlatform()
  const missing: string[] = []
  let vsInstall: string | undefined
  let vcvars: string | undefined
  if (platform === 'win32') {
    vsInstall = findVisualStudio(env)
    vcvars = vsInstall ? join(vsInstall, 'VC', 'Auxiliary', 'Build', 'vcvars64.bat') : undefined
    if (!vcvars || !existsSync(vcvars)) missing.push('Visual Studio 2022 with the MSVC x64 tools (vcvars64.bat)')
  }
  const cmake = firstExisting(cmakeCandidates(platform, vsInstall, env))
  if (!cmake) missing.push('CMake (set SKETCH_ORCA_CMAKE)')
  const ninja = firstExisting(ninjaCandidates(platform, vsInstall, env))
  if (!ninja) missing.push('Ninja (set SKETCH_ORCA_NINJA)')
  const perl = firstExisting(perlCandidates(platform, env))
  if (!perl) missing.push(platform === 'win32' ? 'Strawberry Perl (C:\\Strawberry\\perl\\bin\\perl.exe, or set SKETCH_ORCA_PERL)' : 'perl')
  const git = firstExisting(gitCandidates(platform, env))
  if (!git) missing.push('git (set SKETCH_ORCA_GIT)')
  if (missing.length > 0) throw new StepError(`missing build tools: ${missing.join('; ')}`)

  let buildEnv: Record<string, string>
  if (platform === 'win32') {
    const vc = vcvarsEnvironment(vcvars!, env)
    const vcPathKey = Object.keys(vc).find((k) => k.toUpperCase() === 'PATH') ?? 'Path'
    const path = cleanWinBuildPath(splitPathList(vc[vcPathKey], 'win32'), [dirname(perl!), dirname(git!)])
    delete vc[vcPathKey]
    buildEnv = finishBuildEnv(vc, path, 'win32')
  } else {
    // Linux and macOS (not yet tried): the system's compilers, cc and c++, found on this PATH.
    const base: Record<string, string> = {}
    for (const [k, v] of Object.entries(env)) if (v !== undefined) base[k] = v
    buildEnv = finishBuildEnv(base, unixBuildPath(platform, [dirname(cmake!), dirname(ninja!), dirname(git!), dirname(perl!)]), platform)
  }
  return { platform, vsInstall, vcvars, cmake: cmake!, ninja: ninja!, perl: perl!, git: git!, ignorePrefixes: cmakeIgnorePrefixes(cmake!), env: buildEnv }
}

/**
 * Checks the build environment. Windows: perl and cl resolve to the expected programs and no gcc
 * is in sight. Linux and macOS (not yet tried): the compilers, make and the autotools are on PATH.
 */
export function checkToolchain(t: Toolchain): string[] {
  const notes: string[] = []
  if (t.platform !== 'win32') {
    const dirs = splitPathList(t.env.PATH, t.platform)
    const missing = unixBuildPrograms(t.platform).filter((names) => !findOnPath(names, dirs))
    if (missing.length > 0) {
      throw new StepError(`not on the build PATH: ${missing.map((n) => n.join(' or ')).join(', ')} (builds on ${t.platform} have not been tried yet)`)
    }
    notes.push(`${t.platform}: cc ${findOnPath(['cc'], dirs)}, c++ ${findOnPath(['c++'], dirs)} (builds on ${t.platform} have not been tried yet)`)
    return notes
  }
  const where = (name: string) => capture('where.exe', [name], { env: t.env }).stdout.trim().split(/\r?\n/).filter(Boolean)
  const perls = where('perl')
  if (perls.length === 0 || perls[0]!.toLowerCase() !== t.perl.toLowerCase()) {
    throw new StepError(`perl on the build PATH is ${perls[0] ?? '(none)'}, expected ${t.perl}`)
  }
  const cls = where('cl')
  if (cls.length === 0) throw new StepError('cl.exe is not on the build PATH after vcvars64.bat')
  if (where('gcc').length > 0) throw new StepError(`gcc is on the build PATH (${where('gcc')[0]}); the PATH cleaning failed`)
  notes.push(`perl: ${perls[0]}`, `cl: ${cls[0]}`)
  return notes
}
