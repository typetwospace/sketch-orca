// The environment every parity run gives an engine: the one a client that downloads the staged
// engine and runs it on its user's machine sets up (Sketch's slicer does it this way), and nothing
// else from the shell the harness was started in.
//
// On Windows: SystemRoot and windir; PATH with the engine's libraryPath first, then System32, then
// the Windows folder itself; and TEMP, TMP, LOCALAPPDATA, APPDATA and USERPROFILE when they are set.
// Elsewhere: PATH /usr/bin:/bin, HOME, TMPDIR, and the libraryPath on LD_LIBRARY_PATH or
// DYLD_LIBRARY_PATH. There is no USER or USERNAME, so Orca's [user] placeholder reads 'unknown'
// (PlaceholderParser.cpp:105 reads USER, then USERNAME), and no SLIC3R_* variable (the names the
// Slic3r family has used for settings from the environment), so what a run gives does not depend
// on who runs it or how their shell is set up.

import type { Platform } from '../env.ts'

export type EngineEnv = Record<string, string>

/** The variables Windows runs pass on from the harness's own environment, when set. */
export const PASSED_ON_WIN32 = ['TEMP', 'TMP', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE'] as const

/** A variable's value, its name in any letter case on Windows (where names are not case sensitive). */
function lookup(base: NodeJS.ProcessEnv, name: string, platform: Platform): string | undefined {
  if (base[name] !== undefined) return base[name]
  if (platform !== 'win32') return undefined
  const key = Object.keys(base).find((k) => k.toUpperCase() === name.toUpperCase())
  return key === undefined ? undefined : base[key]
}

/** The environment an engine whose libraries are in `libraryPath` (absolute) runs in. */
export function parityEngineEnv(engine: { libraryPath: string }, base: NodeJS.ProcessEnv, platform: Platform): EngineEnv {
  if (platform === 'win32') {
    const root = lookup(base, 'SystemRoot', platform) ?? 'C:\\Windows'
    const env: EngineEnv = { SystemRoot: root, windir: root, PATH: `${engine.libraryPath};${root}\\System32;${root}` }
    for (const key of PASSED_ON_WIN32) {
      const v = lookup(base, key, platform)
      if (v) env[key] = v
    }
    return env
  }
  const env: EngineEnv = { PATH: '/usr/bin:/bin', HOME: base.HOME ?? '/tmp', TMPDIR: base.TMPDIR ?? '/tmp' }
  env[platform === 'darwin' ? 'DYLD_LIBRARY_PATH' : 'LD_LIBRARY_PATH'] = engine.libraryPath
  return env
}

/**
 * What would make a run's output depend on the person or shell that ran it: a user name (USER,
 * USERNAME, LOGNAME) or a SLIC3R_* variable, in any letter case. Empty for parityEngineEnv's
 * environments; the harness checks every environment it runs an engine in (test hooks added).
 */
export function engineEnvProblems(env: EngineEnv): string[] {
  const problems: string[] = []
  for (const k of Object.keys(env)) {
    const u = k.toUpperCase()
    if (u === 'USER' || u === 'USERNAME' || u === 'LOGNAME') problems.push(`${k} is set: Orca's [user] would name whoever ran it`)
    if (u.startsWith('SLIC3R_')) problems.push(`${k} is set: a SLIC3R_* variable is not part of the environment a client gives the engine`)
  }
  return problems
}
