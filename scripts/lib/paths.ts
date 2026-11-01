// Where the pipeline keeps things. Everything large lives in the work folder, outside the repo:
// %LOCALAPPDATA%\SketchDev\orca-engine on Windows, ~/.local/share/SketchDev/orca-engine elsewhere
// (XDG_DATA_HOME honoured); SKETCH_ORCA_HOME overrides both.

import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import type { Platform } from './env.ts'

export function workHome(env: NodeJS.ProcessEnv, platform: Platform, home = homedir()): string {
  if (env.SKETCH_ORCA_HOME) return resolve(env.SKETCH_ORCA_HOME)
  if (platform === 'win32') {
    const base = env.LOCALAPPDATA ?? join(home, 'AppData', 'Local')
    return join(base, 'SketchDev', 'orca-engine')
  }
  const base = env.XDG_DATA_HOME ?? join(home, '.local', 'share')
  return join(base, 'SketchDev', 'orca-engine')
}

export interface WorkPaths {
  home: string
  /** The Orca checkout at the locked commit, with our patches applied. */
  src: string
  /** Orca's dependency build (CMake binary dir of `src/deps`). */
  deps: string
  /** The dependency install prefix inside it. */
  depsPrefix: string
  /** Downloaded dependency source packages, kept across rebuilds. */
  downloads: string
  /** Orca's own build (libslic3r and sketch-orca). */
  build: string
  /** Staged engine and source bundle. */
  dist: string
  /**
   * The MinGW-w64 toolchain that builds GMP and MPFR on Windows (mingw.ts): its pinned packages in
   * downloads/, kept across runs, and the tree they unpack to, msys64/.
   */
  mingw: string
  logs: string
  state: string
  smoke: string
}

export function workPaths(home: string): WorkPaths {
  const deps = join(home, 'deps')
  return {
    home,
    src: join(home, 'src'),
    deps,
    depsPrefix: join(deps, 'OrcaSlicer_dep', 'usr', 'local'),
    downloads: join(home, 'downloads'),
    build: join(home, 'build'),
    dist: join(home, 'dist'),
    mingw: join(home, 'mingw'),
    logs: join(home, 'logs'),
    state: join(home, 'state'),
    smoke: join(home, 'smoke'),
  }
}

/** Repository paths, from this file's location (`scripts/lib/paths.ts`): `engineDir` is the repository root. */
export function enginePaths(engineDir: string) {
  return {
    engine: engineDir,
    lock: join(engineDir, 'orca.lock.json'),
    patches: join(engineDir, 'patches'),
    cli: join(engineDir, 'cli'),
    scripts: join(engineDir, 'scripts'),
    /** Licence texts the engine ships for dependencies whose packages lack them (stage.ts OWN_LICENSE_TEXTS). */
    licenses: join(engineDir, 'licenses'),
    readme: join(engineDir, 'README.md'),
    /** The attribution chain and the copyright holders of the changes; staged into LICENSES/ (stage.ts ownNoticeCopies). */
    notice: join(engineDir, 'NOTICE'),
    authors: join(engineDir, 'AUTHORS'),
  }
}

export function sha256File(path: string): string {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function sha256Text(text: string): string {
  return createHash('sha256').update(text).digest('hex')
}

/** A step's stamp: the hash of its inputs when it last finished. */
export interface Stamp {
  inputs: string
  finishedAt: string
  detail?: unknown
}

export function readStamp(stateDir: string, step: string): Stamp | undefined {
  const p = join(stateDir, `${step}.json`)
  if (!existsSync(p)) return undefined
  try {
    return JSON.parse(readFileSync(p, 'utf8')) as Stamp
  } catch {
    return undefined
  }
}

export function writeStamp(stateDir: string, step: string, inputs: string, detail?: unknown): void {
  const p = join(stateDir, `${step}.json`)
  mkdirSync(dirname(p), { recursive: true })
  const stamp: Stamp = { inputs, finishedAt: new Date().toISOString(), detail }
  writeFileSync(p, JSON.stringify(stamp, null, 2) + '\n')
}

/**
 * What a finished dependency build records (state/deps.json's detail; orca.ts): what it was built
 * from, and the digest (files.ts hashTree) of the prefix it installed, which the engine links.
 * `fresh` says it ran from an empty build folder in one run, so every dependency was unpacked from
 * its pinned package and built as it is; a run that resumes one that stopped is not fresh, since an
 * unpacked source may have been edited in between.
 */
export interface DepsRecord {
  commit: string
  depsPatches: { name: string; sha256: string }[]
  prefixDigest: string
  fresh: boolean
}

/**
 * How the dependency build on record (state/deps.json) differs from what the engine needs now:
 * the locked commit and the patches of deps/ (checkout.ts depsPatchesOf), and when given the
 * inputs build works out (orca.ts depsWant), a fresh build, and the prefix's digest. That is
 * worked out (reading the whole prefix) only when nothing else is wrong. Empty when it fits.
 */
export function depsStampProblems(
  stamp: { inputs: string; detail?: unknown } | undefined,
  want: { commit: string; depsPatches: { name: string; sha256: string }[]; inputs?: string; fresh?: boolean; prefixDigest?: () => string },
): string[] {
  if (!stamp) return ['no finished dependency build is recorded (state/deps.json)']
  const d = (stamp.detail ?? {}) as Partial<DepsRecord>
  if (typeof d.commit !== 'string' || !Array.isArray(d.depsPatches) || typeof d.prefixDigest !== 'string' || typeof d.fresh !== 'boolean') {
    return ['the dependency build was recorded by an earlier pipeline, without what it was built from or a digest of what it installed']
  }
  const out: string[] = []
  if (d.commit !== want.commit) out.push(`the dependencies were built at ${d.commit}, the lock is at ${want.commit}`)
  const patches = (x: { name: string; sha256: string }[]) => JSON.stringify(x.map((p) => [p.name, p.sha256]))
  if (patches(d.depsPatches) !== patches(want.depsPatches)) out.push('the dependencies were built with other patches of deps/ than patches/ has now')
  if (want.inputs !== undefined && stamp.inputs !== want.inputs) out.push('the dependencies were built from other inputs (their configuration, CMake or the compiler settings changed since)')
  if (want.fresh && !d.fresh) out.push('the dependency build was resumed after it stopped, so an unpacked source may have been edited in between')
  if (out.length === 0 && want.prefixDigest && want.prefixDigest() !== d.prefixDigest) out.push('the dependency prefix is not what the dependency build installed (a file changed, added or removed since)')
  return out
}

/**
 * The hash (files.ts hashTree) of cli/ that the last finished engine build was compiled
 * from: build records it before Ninja runs and checks it again after (orca.ts). Undefined when the
 * build is older than that.
 */
export function builtCliSha256(work: WorkPaths): string | undefined {
  const detail = readStamp(work.state, 'orca')?.detail as { cliSha256?: unknown } | undefined
  return typeof detail?.cliSha256 === 'string' && /^[0-9a-f]{64}$/.test(detail.cliSha256) ? detail.cliSha256 : undefined
}
