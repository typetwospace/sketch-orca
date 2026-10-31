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
   * The source packages of the libraries Orca ships prebuilt (GMP on Windows), one folder per
   * library, kept across runs; `source` downloads them (bundle.ts PREBUILT_SOURCES).
   */
  prebuilt: string
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
    prebuilt: join(home, 'prebuilt'),
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
 * The hash (files.ts hashTree) of cli/ that the last finished engine build was compiled
 * from: build records it before Ninja runs and checks it again after (orca.ts). Undefined when the
 * build is older than that.
 */
export function builtCliSha256(work: WorkPaths): string | undefined {
  const detail = readStamp(work.state, 'orca')?.detail as { cliSha256?: unknown } | undefined
  return typeof detail?.cliSha256 === 'string' && /^[0-9a-f]{64}$/.test(detail.cliSha256) ? detail.cliSha256 : undefined
}
