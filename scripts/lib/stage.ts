// `stage`: the engine as it ships, in <work>/dist/engine:
//
//   bin/              sketch-orca(.exe) and the libraries it loads, next to it
//   resources/        what libslic3r reads at run time (STAGED_RESOURCES)
//   LICENSES/         Orca's licence (AGPL-3.0) and the licences of the code built into the engine
//   manifest.json     sketch-orca-engine/1: versions, lock, patches, and every file with its size and sha256
//
// Only an engine that passed smoke is staged (--without-smoke stages a dev build and says so in the
// manifest), and only one built from cli/ as it is now (the hash the build recorded). The
// engine must report the locked commit. Every library on its link line needs a licence text (Orca's
// own, the dependency bundle's, deps_src's files or DEPS_SRC_NOTICES), and none may be Intel IPP.
// The folder is built next to the old one; no staged file may name the builder's home folder or
// user name; the staged copy runs with no library folder on its PATH, and (after a passed smoke)
// slices smoke's STAGED_CASES with the staged resources, which proves the folder is complete; then
// it is swapped in. Run again, staging skips when its inputs are unchanged and every file still
// matches the manifest.

import { existsSync, readdirSync, readFileSync, renameSync, rmSync } from 'node:fs'
import { homedir, userInfo } from 'node:os'
import { basename, isAbsolute, join } from 'node:path'
import type { Context } from '../orca.ts'
import { headCommit, listPatches, type PatchInfo } from './checkout.ts'
import { firstExisting, gitCandidates, type Platform } from './env.ts'
import { copyFile, hashTree, listFiles, nativePath, sha256Bytes, treeEntries, verifyTree, writeText, type FileEntry } from './files.ts'
import type { OrcaLock } from './lock.ts'
import { builtCliSha256, readStamp, sha256File, sha256Text, writeStamp, type WorkPaths } from './paths.ts'
import { readPeDebugPaths, resolveRuntime, windowsRuntimeIo, type RuntimeIo } from './pe.ts'
import { capture, Log, StepError } from './run.ts'
import { checkVersion, engineEnv, engineLibraryDirs, runCases, runEngine, SMOKE_CASES, smokePassedFor, smokePresetSets, smokeTimeoutMs, SMOKE_VERSION } from './smoke.ts'
import { currentPlatform } from './toolchain.ts'

export const ENGINE_FORMAT = 'sketch-orca-engine/1'
/** Bump when the staged layout or the manifest changes: a stage of an older version is redone. */
export const STAGE_VERSION = 2
/** Folders of Orca's resources/ that libslic3r reads (resources_dir()): presets, nozzle tables, flush volumes. */
export const STAGED_RESOURCES = ['profiles', 'info', 'flush'] as const
/**
 * Smoke cases the staged copy slices before it is swapped in, with the staged resources and only
 * the system folders on its PATH: a plain G-code printer, and a Bambu printer with its .gcode.3mf.
 */
export const STAGED_CASES = ['cube-fixed', 'bambu-3mf'] as const

export interface EngineManifest {
  format: typeof ENGINE_FORMAT
  stageVersion: number
  engine: {
    name: string
    version: string
    /** Relative to the engine folder. */
    exe: string
    /** The folder a job's `resources` names, relative to the engine folder. */
    resources: string
    /** The folder holding the engine's libraries (Linux and macOS: put it on LD_LIBRARY_PATH or DYLD_LIBRARY_PATH). */
    libraryPath: string
  }
  orca: { repo: string; tag: string; version: string; commit: string; publishedAt: string; license: string }
  platform: Platform
  arch: string
  patches: { name: string; sha256: string }[]
  /** Hash of the front end's sources, cli/ (files.ts hashTree). */
  cli: string
  /** Libraries shipped in bin/ next to the engine. */
  runtime: string[]
  /** Libraries the system provides. */
  system: string[]
  smoke: { passed: boolean; smokeVersion: number }
  /** Hash of what staging read; with the same inputs and matching files, staging has nothing to do. */
  inputs: string
  stagedAt: string
  /** Every file of the engine folder except manifest.json, sorted by path. */
  files: FileEntry[]
}

export const MANIFEST_FILE = 'manifest.json'

const HEX64 = /^[0-9a-f]{64}$/

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

/** A relative path with forward slashes that stays inside its folder. */
export function isSafeRelPath(p: string): boolean {
  return p !== '' && !p.startsWith('/') && !/^[A-Za-z]:/.test(p) && !p.includes('\\') && p.split('/').every((s) => s !== '' && s !== '.' && s !== '..')
}

/** Checks a parsed manifest and returns it typed; throws naming the bad field. */
export function validateManifest(value: unknown): EngineManifest {
  if (!isRecord(value)) throw new Error('manifest: not a JSON object')
  if (value.format !== ENGINE_FORMAT) throw new Error(`manifest: format must be "${ENGINE_FORMAT}", got ${JSON.stringify(value.format)}`)
  const rec = (v: unknown, f: string) => {
    if (!isRecord(v)) throw new Error(`manifest: ${f} must be an object`)
    return v
  }
  const str = (o: Record<string, unknown>, k: string, f: string) => {
    if (typeof o[k] !== 'string') throw new Error(`manifest: ${f}.${k} must be text`)
    return o[k] as string
  }
  const strings = (v: unknown, f: string) => {
    if (!Array.isArray(v) || !v.every((x) => typeof x === 'string')) throw new Error(`manifest: ${f} must be a list of text`)
    return v as string[]
  }
  if (typeof value.stageVersion !== 'number') throw new Error('manifest: stageVersion must be a number')
  const e = rec(value.engine, 'engine')
  const o = rec(value.orca, 'orca')
  const s = rec(value.smoke, 'smoke')
  if (typeof s.passed !== 'boolean' || typeof s.smokeVersion !== 'number') throw new Error('manifest: smoke needs passed and smokeVersion')
  const platform = value.platform
  if (platform !== 'win32' && platform !== 'linux' && platform !== 'darwin') throw new Error(`manifest: unknown platform ${JSON.stringify(platform)}`)
  if (!Array.isArray(value.patches)) throw new Error('manifest: patches must be a list')
  const patches = value.patches.map((p, i) => {
    if (!isRecord(p) || typeof p.name !== 'string' || typeof p.sha256 !== 'string' || !HEX64.test(p.sha256)) throw new Error(`manifest: patches[${i}] needs name and sha256`)
    return { name: p.name, sha256: p.sha256 }
  })
  if (!Array.isArray(value.files)) throw new Error('manifest: files must be a list')
  const files = value.files.map((f, i): FileEntry => {
    if (!isRecord(f) || typeof f.path !== 'string' || !isSafeRelPath(f.path)) throw new Error(`manifest: files[${i}].path must be a relative path inside the engine folder`)
    if (typeof f.size !== 'number' || !Number.isInteger(f.size) || f.size < 0) throw new Error(`manifest: files[${i}].size must be a whole number`)
    if (typeof f.sha256 !== 'string' || !HEX64.test(f.sha256)) throw new Error(`manifest: files[${i}].sha256 must be 64 hex characters`)
    return { path: f.path, size: f.size, sha256: f.sha256 }
  })
  const exe = str(e, 'exe', 'engine')
  if (!isSafeRelPath(exe) || !files.some((f) => f.path === exe)) throw new Error(`manifest: engine.exe ${JSON.stringify(exe)} is not one of the files`)
  const commit = str(o, 'commit', 'orca')
  if (!/^[0-9a-f]{40}$/.test(commit)) throw new Error('manifest: orca.commit must be 40 hex characters')
  if (typeof value.inputs !== 'string' || !HEX64.test(value.inputs)) throw new Error('manifest: inputs must be a sha256')
  return {
    format: ENGINE_FORMAT,
    stageVersion: value.stageVersion,
    engine: { name: str(e, 'name', 'engine'), version: str(e, 'version', 'engine'), exe, resources: str(e, 'resources', 'engine'), libraryPath: str(e, 'libraryPath', 'engine') },
    orca: { repo: str(o, 'repo', 'orca'), tag: str(o, 'tag', 'orca'), version: str(o, 'version', 'orca'), commit, publishedAt: str(o, 'publishedAt', 'orca'), license: str(o, 'license', 'orca') },
    platform,
    arch: typeof value.arch === 'string' ? value.arch : '',
    patches,
    cli: typeof value.cli === 'string' ? value.cli : '',
    runtime: strings(value.runtime, 'runtime'),
    system: strings(value.system, 'system'),
    smoke: { passed: s.passed, smokeVersion: s.smokeVersion },
    inputs: value.inputs,
    stagedAt: typeof value.stagedAt === 'string' ? value.stagedAt : '',
    files,
  }
}

export function readManifest(engineDir: string): EngineManifest {
  return validateManifest(JSON.parse(readFileSync(join(engineDir, MANIFEST_FILE), 'utf8')))
}

export function serializeManifest(m: EngineManifest): string {
  return JSON.stringify(m, null, 2) + '\n'
}

/** Whether a staged engine folder is exactly what its manifest lists (manifest.json itself aside). */
export function verifyStaged(engineDir: string, m: EngineManifest): string[] {
  return verifyTree(engineDir, m.files, [MANIFEST_FILE])
}

// ---------------------------------------------------------------- what goes in

/** One file to copy into the engine folder. */
export interface StageCopy {
  from: string
  /** Relative to the engine folder, forward slashes. */
  to: string
  /**
   * Only the licence notice at the top of `from` (leadingComment) is staged, after this heading:
   * for code whose licence is stated in its sources (DEPS_SRC_NOTICES).
   */
  notice?: string
}

/** Top-level names of licence texts in a source tree. */
const LICENSE_NAME = /licen[cs]e|copying|copyright|notice|exception|patents|^readme\.ijg$/i

export function isLicenseFile(name: string): boolean {
  return LICENSE_NAME.test(name)
}

/** Licence texts a package keeps below its top level (FreeType's LICENSE.TXT points to docs/). */
export const EXTRA_LICENSE_FILES: Record<string, string[]> = {
  FREETYPE: ['docs/FTL.TXT', 'docs/GPLv2.TXT'],
  // The libnoise package Orca pins dropped libnoise's doc/ folder, and COPYING.txt with it. Its
  // sources carry the grant (LGPL 2.1 or later); the licence text comes from OWN_LICENSE_TEXTS.
  libnoise: ['src/noise.h'],
}

/** Licence texts licenses/ supplies for a dependency whose package lacks them. */
export const OWN_LICENSE_TEXTS: Record<string, string[]> = {
  libnoise: ['LGPL-2.1.txt'],
}

/**
 * Code in Orca's deps_src/ that the engine is built with but that has no licence file of its own:
 * its licence is stated at the top of a source file, and that notice is staged as
 * LICENSES/deps_src/<name>/<file>.notice.txt. Parts with a licence file (qhull, libnest2d, mcut,
 * miniz, ...) are found by topLevelLicenses. Not here, because the engine has none of their code:
 * hidapi, hints and stb_dxt (GUI only). linkedPartProblems refuses an engine that links a deps_src
 * library with neither.
 */
export const DEPS_SRC_NOTICES: Record<string, { file: string; spdx: string; also?: { dependency: string; file: string; to: string } }> = {
  admesh: { file: 'stl.h', spdx: 'GPL-2.0-or-later' },
  ankerl: { file: 'unordered_dense.h', spdx: 'MIT' },
  clipper: { file: 'clipper.hpp', spdx: 'BSL-1.0' },
  clipper2: { file: 'Clipper2Lib/include/clipper2/clipper.h', spdx: 'BSL-1.0' },
  fast_float: { file: 'fast_float.h', spdx: 'MIT' },
  'glu-libtess': { file: 'src/tess.c', spdx: 'SGI-B-2.0' },
  // The MPL 2.0 text itself is Eigen's copy (Eigen is MPL 2.0 too).
  libigl: { file: 'igl/igl_inline.h', spdx: 'MPL-2.0', also: { dependency: 'Eigen', file: 'COPYING.MPL2', to: 'MPL-2.0.txt' } },
  nanosvg: { file: 'nanosvg.h', spdx: 'Zlib' },
  nlohmann: { file: 'json.hpp', spdx: 'MIT' },
  qoi: { file: 'qoi.h', spdx: 'MIT' },
  semver: { file: 'semver.h', spdx: 'MIT' },
  Shiny: { file: 'Shiny.h', spdx: 'MIT' },
}

/**
 * The comment at the top of a source file, where its licence notice stands: the leading lines that
 * are blank, // comments or inside a block comment, up to the first line of code.
 */
export function leadingComment(text: string): string {
  const out: string[] = []
  let inBlock = false
  for (const raw of text.replace(/^\uFEFF/, '').split(/\r?\n/)) {
    const line = raw.trimEnd()
    const t = line.trim()
    if (inBlock) {
      out.push(line)
      if (t.includes('*/')) inBlock = false
      continue
    }
    if (t === '' || t.startsWith('//')) {
      out.push(line)
      continue
    }
    if (t.startsWith('/*')) {
      out.push(line)
      inBlock = !t.slice(2).includes('*/')
      continue
    }
    break
  }
  while (out.length > 0 && out[out.length - 1]!.trim() === '') out.pop()
  while (out.length > 0 && out[0]!.trim() === '') out.shift()
  return out.join('\n')
}

/** A staged notice: the heading, then the leading comment of the source; refused when it names no licence. */
export function noticeText(heading: string, from: string): string {
  const comment = leadingComment(readFileSync(from, 'utf8'))
  if (!/licen[cs]e|copyright|permission is (hereby )?granted/i.test(comment)) throw new StepError(`${from} starts with no licence notice; update DEPS_SRC_NOTICES (scripts/lib/stage.ts)`)
  return `${heading}\n\n${comment}\n`
}

function topLevelLicenses(dir: string): string[] {
  if (!existsSync(dir)) return []
  const out: string[] = []
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    if (ent.isFile() && isLicenseFile(ent.name)) out.push(ent.name)
    // A folder of licence texts (Blosc's LICENSES/).
    if (ent.isDirectory() && /^licen[cs]es$/i.test(ent.name)) {
      for (const sub of readdirSync(join(dir, ent.name), { withFileTypes: true })) if (sub.isFile()) out.push(`${ent.name}/${sub.name}`)
    }
  }
  return out.sort()
}

/** The dependencies Orca's deps build finished (ExternalProject's done stamp), by name. */
export function builtDependencies(depsBuildDir: string): string[] {
  if (!existsSync(depsBuildDir)) return []
  const out: string[] = []
  for (const ent of readdirSync(depsBuildDir, { withFileTypes: true })) {
    const m = /^dep_(.+)-prefix$/.exec(ent.name)
    if (!ent.isDirectory() || !m) continue
    const name = m[1]!
    const stampDir = join(depsBuildDir, ent.name, 'src', `dep_${name}-stamp`)
    const done = [join(stampDir, `dep_${name}-done`), join(stampDir, 'Release', `dep_${name}-done`)]
    if (done.some((p) => existsSync(p))) out.push(name)
  }
  return out.sort()
}

/**
 * Where a dependency's sources were unpacked: ExternalProject's `src/dep_<Name>`, or the SOURCE_DIR
 * its recipe names inside Orca's tree (EXPAT builds from deps/EXPAT/expat).
 */
export function dependencySourceDir(work: WorkPaths, name: string): string | undefined {
  const unpacked = join(work.deps, `dep_${name}-prefix`, 'src', `dep_${name}`)
  if (existsSync(unpacked)) return unpacked
  const recipe = join(work.src, 'deps', name, `${name}.cmake`)
  if (!existsSync(recipe)) return undefined
  const m = /^\s*SOURCE_DIR\s+\$\{CMAKE_CURRENT_LIST_DIR\}\/(\S+)/m.exec(readFileSync(recipe, 'utf8'))
  if (!m) return undefined
  const dir = join(work.src, 'deps', name, ...m[1]!.split('/'))
  return existsSync(dir) ? dir : undefined
}

/** The licence files the engine ships, from the checkout, the dependency build and licenses/ (`ownDir`). */
export function licenseCopies(work: WorkPaths, platform: Platform, ownDir: string): StageCopy[] {
  const out: StageCopy[] = []
  const add = (dir: string, files: string[], to: string) => {
    for (const f of files) {
      const from = nativePath(dir, f)
      if (existsSync(from)) out.push({ from, to: `LICENSES/${to}/${f}` })
    }
  }
  add(work.src, ['LICENSE.txt'], 'OrcaSlicer')
  const built = builtDependencies(work.deps)
  for (const name of built) {
    const dir = dependencySourceDir(work, name)
    if (dir) add(dir, [...topLevelLicenses(dir), ...(EXTRA_LICENSE_FILES[name] ?? [])], `deps/${name}`)
    add(ownDir, OWN_LICENSE_TEXTS[name] ?? [], `deps/${name}`)
  }
  // OpenCV installs the notices of the third-party code it builds in (SoftFloat in its core, its own
  // libpng) into the prefix rather than its source's top level: etc/licenses on Windows,
  // share/licenses/opencv4 elsewhere.
  if (built.includes('OpenCV')) {
    for (const rel of ['etc/licenses', 'share/licenses/opencv4']) {
      const d = nativePath(work.depsPrefix, rel)
      if (!existsSync(d)) continue
      const files = readdirSync(d, { withFileTypes: true }).filter((e) => e.isFile()).map((e) => e.name)
      add(d, files.sort(), 'deps/OpenCV/third-party')
    }
  }
  // Code Orca keeps in its own tree (deps_src/<name>): licence files, and the notices in the
  // sources of the parts that have none (DEPS_SRC_NOTICES).
  const depsSrc = join(work.src, 'deps_src')
  if (existsSync(depsSrc)) {
    for (const ent of readdirSync(depsSrc, { withFileTypes: true })) {
      if (ent.isDirectory()) add(join(depsSrc, ent.name), topLevelLicenses(join(depsSrc, ent.name)), `deps_src/${ent.name}`)
    }
    for (const [name, n] of Object.entries(DEPS_SRC_NOTICES)) {
      const from = join(depsSrc, name, ...n.file.split('/'))
      if (!existsSync(from)) continue
      out.push({ from, to: `LICENSES/deps_src/${name}/${basename(n.file)}.notice.txt`, notice: `SPDX-License-Identifier: ${n.spdx}\nThe licence notice at the top of deps_src/${name}/${n.file} in the OrcaSlicer source tree:` })
      const dir = n.also ? dependencySourceDir(work, n.also.dependency) : undefined
      if (n.also && dir && existsSync(join(dir, n.also.file))) out.push({ from: join(dir, n.also.file), to: `LICENSES/deps_src/${name}/${n.also.to}` })
    }
  }
  // Windows ships Orca's prebuilt GMP and MPFR. GMP 5.0.1 is under the LGPL 3 or later: its folder
  // has the GPL 3 text (which the LGPL 3 adds to) and the README that says so; the LGPL 3 text is
  // MPFR's copy.
  if (platform === 'win32') {
    const gmp = join(work.src, 'deps', 'GMP', 'gmp')
    const mpfr = join(work.src, 'deps', 'MPFR', 'mpfr')
    add(gmp, [...topLevelLicenses(gmp), 'gmp.README'], 'deps/GMP')
    if (existsSync(join(mpfr, 'mpfr.COPYING.LESSER'))) out.push({ from: join(mpfr, 'mpfr.COPYING.LESSER'), to: 'LICENSES/deps/GMP/LGPL-3.0.txt' })
    add(mpfr, topLevelLicenses(mpfr), 'deps/MPFR')
  }
  return dedupe(out)
}

/**
 * This repository's own notices, which every staged engine carries next to the licences (AGPL-3.0
 * sections 4 and 5: the copyright notices and the attribution chain): NOTICE as LICENSES/NOTICE.txt
 * and AUTHORS as LICENSES/AUTHORS.txt. Refused when either file is missing.
 */
export function ownNoticeCopies(engine: { notice: string; authors: string }): StageCopy[] {
  const out: StageCopy[] = [
    { from: engine.notice, to: 'LICENSES/NOTICE.txt' },
    { from: engine.authors, to: 'LICENSES/AUTHORS.txt' },
  ]
  for (const c of out) if (!existsSync(c.from)) throw new StepError(`${c.from} is missing; every staged engine carries it as ${c.to}`)
  return out
}

/**
 * Licence texts a release must carry whatever the folders hold: on Windows, GMP's LGPL 3 text and
 * README, and the notice of the SoftFloat code in OpenCV's core.
 */
export function requiredLicenses(platform: Platform): string[] {
  return platform === 'win32' ? ['LICENSES/deps/GMP/LGPL-3.0.txt', 'LICENSES/deps/GMP/gmp.README', 'LICENSES/deps/OpenCV/third-party/SoftFloat-COPYING.txt'] : []
}

/**
 * The libraries on the engine's link line in the Orca build's build.ninja (LINK_LIBRARIES of the
 * sketch-orca target): paths relative to the build folder, or absolute; flags are left out.
 */
export function linkedLibraries(buildNinja: string): string[] {
  const lines = buildNinja.split(/\r?\n/)
  const start = lines.findIndex((l) => /^build sketch-orca[\\/]sketch-orca(\.exe)?:/.test(l))
  if (start < 0) return []
  for (let i = start + 1; i < lines.length && /^\s/.test(lines[i]!); i++) {
    const m = /^\s+LINK_LIBRARIES = (.*)$/.exec(lines[i]!)
    if (!m) continue
    return [...m[1]!.matchAll(/"([^"]*)"|(\S+)/g)].map((t) => t[1] ?? t[2]!).filter((t) => !t.startsWith('-') && /\.(lib|a|so|dylib)$/i.test(t))
  }
  return []
}

/**
 * What a link line needs before it ships: no Intel IPP (OpenCV built without it, patch 0001), and
 * a licence text for every deps_src library (licence files or DEPS_SRC_NOTICES; build-folder
 * libraries under deps_src/<name>/, or named after a deps_src folder, as lib/semver.lib). Orca's
 * own libraries are under src/; anything else built in the Orca build folder is refused too.
 */
export function linkedPartProblems(libs: string[], depsSrcNames: string[], copies: StageCopy[]): string[] {
  const problems: string[] = []
  const ipp = libs.filter((l) => /^ipp/i.test(basename(l.replace(/\\/g, '/'))))
  if (ipp.length > 0) problems.push(`the engine links Intel IPP (${ipp.map((l) => basename(l.replace(/\\/g, '/'))).join(', ')}); build the dependencies with DEP_OPENCV_IPP=OFF`)
  const byLower = new Map(depsSrcNames.map((n) => [n.toLowerCase(), n]))
  const parts = new Set<string>()
  for (const lib of libs) {
    const rel = lib.replace(/\\/g, '/')
    // Absolute: the dependency bundle (licensed per dependency) or the system. A bare name is a
    // system library found on the linker's path (kernel32.lib, ws2_32.lib).
    if (isAbsolute(lib) || /^[A-Za-z]:\//.test(rel) || rel.startsWith('/') || !rel.includes('/')) continue
    if (rel.startsWith('src/')) continue
    const m = /^deps_src\/([^/]+)\//.exec(rel)
    const name = m ? byLower.get(m[1]!.toLowerCase()) : byLower.get(basename(rel).replace(/\.(lib|a|so|dylib)$/i, '').replace(/^lib/, '').toLowerCase())
    if (!name) {
      problems.push(`the engine links ${rel}, which is neither Orca's own (src/) nor a deps_src part`)
      continue
    }
    parts.add(name)
  }
  for (const name of [...parts].sort()) {
    if (!copies.some((c) => c.to.startsWith(`LICENSES/deps_src/${name}/`))) problems.push(`no licence text for deps_src/${name}, which the engine links; add it to DEPS_SRC_NOTICES (scripts/lib/stage.ts)`)
  }
  return problems
}

/** Built dependencies with no licence text among `copies`: the engine is not staged without them. */
export function unlicensedDependencies(work: WorkPaths, copies: StageCopy[]): string[] {
  return builtDependencies(work.deps).filter((name) => !copies.some((c) => c.to.startsWith(`LICENSES/deps/${name}/`)))
}

/**
 * What would name the builder in a staged file: the home folder (C:\Users\me, /home/me) with
 * either slash, \Users\<name> or /home/<name> on any drive, the build folders `roots` (the work
 * folder and this checkout, which the Windows build trims), and git's user.email, with only the
 * ASCII letters made small (as findPersonalPaths folds them). __FILE__ puts such paths into
 * binaries (env.ts, sourcePathTrimEnv trims them on Windows).
 */
export function personalPathNeedles(home: string, user: string, roots: string[] = [], emails: string[] = []): string[] {
  const out = new Set<string>()
  for (const dir of [home, ...roots]) {
    const h = dir.replace(/[\\/]+$/, '')
    if (h.replace(/^[A-Za-z]:/, '').length > 1) {
      out.add(h.replace(/\//g, '\\'))
      out.add(h.replace(/\\/g, '/'))
    }
  }
  if (user.trim() !== '') {
    for (const sep of ['\\', '/']) for (const dir of ['users', 'home']) out.add(`${sep}${dir}${sep}${user}`)
  }
  for (const e of emails) if (e.trim().includes('@')) out.add(e.trim())
  return [...out].map(asciiLower).sort()
}

/** Any user's folder on a Windows drive (C:\Users\<anyone>): no staged file names one (the source bundle's tests do). */
export const USER_FOLDER_NEEDLES: readonly string[] = [':/users/', ':\\users\\']

function asciiLower(s: string): string {
  return s.replace(/[A-Z]/g, (c) => c.toLowerCase())
}

/**
 * The needles found in `bytes`, without case for the ASCII letters: as UTF-8, as Latin-1 (needles
 * that fit in it) and as UTF-16LE at either byte alignment. Only ASCII letters are folded, in
 * UTF-16LE a whole code unit at a time, so no byte of another character changes (U+674E is the
 * bytes 4E 67, an N and a g).
 */
export function findPersonalPaths(bytes: Uint8Array, needles: string[]): string[] {
  const narrow = Buffer.from(bytes)
  for (let i = 0; i < narrow.length; i++) {
    const c = narrow[i]!
    if (c >= 65 && c <= 90) narrow[i] = c + 32
  }
  const wide = [0, 1].map((start) => {
    const b = Buffer.from(bytes)
    for (let i = start; i + 1 < b.length; i += 2) {
      const c = b[i]!
      if (c >= 65 && c <= 90 && b[i + 1] === 0) b[i] = c + 32
    }
    return b
  })
  const inWide = (n: Buffer): boolean =>
    wide.some((b, start) => {
      for (let at = b.indexOf(n); at >= 0; at = b.indexOf(n, at + 1)) if ((at - start) % 2 === 0) return true
      return false
    })
  return needles.filter((n) => narrow.includes(Buffer.from(n, 'utf8')) || (/^[\0-\xff]*$/.test(n) && narrow.includes(Buffer.from(n, 'latin1'))) || inWide(Buffer.from(n, 'utf16le')))
}

/** A program database path recorded in a binary (readPeDebugPaths) that names a folder: C:\..., \\server\..., /... */
export function isRecordedFolderPath(p: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(p) || /^[\\/]/.test(p)
}

/**
 * What in the files `rels` of `dir` would name the builder or the build machine: the needles
 * (findPersonalPaths), and in Windows programs and libraries a program database recorded with its
 * folder (the linker's /DEBUG without /PDBALTPATH:%_PDB%).
 */
export function privacyProblems(dir: string, rels: string[], needles: string[]): string[] {
  const out: string[] = []
  for (const rel of rels) {
    const bytes = readFileSync(nativePath(dir, rel))
    const found = findPersonalPaths(bytes, needles)
    if (found.length > 0) out.push(`${rel} (${found.join(', ')})`)
    if (/\.(exe|dll)$/i.test(rel)) {
      let pdbs: string[] = []
      try {
        pdbs = readPeDebugPaths(bytes)
      } catch {
        // Not a PE file: the needles were checked.
      }
      for (const p of pdbs.filter(isRecordedFolderPath)) out.push(`${rel} (program database ${p})`)
    }
  }
  return out
}

/** The needles of this build (personalPathNeedles): its home folder, user name, build folders where the build trims them, and git's user.email. */
export function builderNeedles(ctx: Pick<Context, 'work' | 'engine'>, platform: Platform, git: string | undefined): string[] {
  const roots = platform === 'win32' ? [ctx.work.home, ctx.engine.engine] : []
  const email = git ? capture(git, ['-C', ctx.engine.engine, 'config', '--get', 'user.email']) : undefined
  return personalPathNeedles(homedir(), safeUserName(), roots, email && email.code === 0 ? [email.stdout.trim()] : [])
}

/** The message for privacyProblems. */
export function privacyRefusal(what: string, problems: string[]): string {
  return `${what} name the builder or the build machine: ${problems.slice(0, 8).join('; ')}${problems.length > 8 ? `; and ${problems.length - 8} more` : ''}. Build in a work folder outside the home folder (SKETCH_ORCA_HOME), link without the program database's folder (/PDBALTPATH:%_PDB%), or find what puts the text there`
}

function dedupe(copies: StageCopy[]): StageCopy[] {
  const seen = new Map<string, StageCopy>()
  for (const c of copies) if (!seen.has(c.to.toLowerCase())) seen.set(c.to.toLowerCase(), c)
  return [...seen.values()].sort((a, b) => (a.to < b.to ? -1 : a.to > b.to ? 1 : 0))
}

/** The resource files the engine ships (STAGED_RESOURCES of the checkout's resources/). */
export function resourceCopies(srcDir: string, listFilesOf: (dir: string) => string[]): StageCopy[] {
  const out: StageCopy[] = []
  for (const r of STAGED_RESOURCES) {
    const dir = join(srcDir, 'resources', r)
    if (!existsSync(dir)) throw new StepError(`the checkout has no resources/${r}`)
    for (const rel of listFilesOf(dir)) out.push({ from: nativePath(dir, rel), to: `resources/${r}/${rel}` })
  }
  return out
}

// ---------------------------------------------------------------- runtime libraries

export interface RuntimePlan {
  /** Library files to put next to the engine. */
  copy: { name: string; path: string }[]
  system: string[]
  /** Libraries found nowhere, with what needs them. */
  missing: string[]
  /**
   * Windows: the imports of MSVC's dynamic C runtime (pe.ts isDynamicCrt), with what needs them.
   * The engine and the DLLs it loads are built with the static runtime (/MT), so no Visual C++
   * runtime DLL ships with it; stage refuses an engine with any.
   */
  crt: string[]
}

/** `ldd` output: each library and where it resolved (undefined: not found); the vDSO and the loader have no `=>`. */
export function parseLdd(text: string): { name: string; path: string | undefined }[] {
  const out: { name: string; path: string | undefined }[] = []
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    const m = /^(\S+)\s+=>\s+(.*?)\s*(\(0x[0-9a-f]+\))?$/i.exec(line)
    if (!m) continue
    const target = m[2]!
    out.push({ name: m[1]!, path: target === 'not found' || target === '' ? undefined : target })
  }
  return out
}

/** `otool -L` output: the install names a Mach-O file loads (the first line names the file itself). */
export function parseOtoolL(text: string): string[] {
  return text
    .split(/\r?\n/)
    .slice(1)
    .map((l) => /^\s+(\S.*?)\s+\(compatibility version/.exec(l)?.[1])
    .filter((n): n is string => n !== undefined)
}

/** Libraries of the system on Linux and macOS: everything outside the work folder. */
export function isUnixSystemLibrary(path: string, workHome: string): boolean {
  return !path.startsWith(workHome.endsWith('/') ? workHome : `${workHome}/`)
}

/** Why stage refuses the engine's libraries: any import of MSVC's dynamic C runtime, then any library not found. */
export function runtimeRefusal(runtime: RuntimePlan): string | undefined {
  if (runtime.crt.length > 0) {
    return `the engine loads MSVC's dynamic C runtime, which would have to ship with it: ${runtime.crt.join(', ')}. Build the dependencies and the engine with the static runtime (/MT: patch 0005, and the configure lines in scripts/lib/cmake.ts), then stage again`
  }
  if (runtime.missing.length > 0) return `the engine's libraries are not all found: ${runtime.missing.join(', ')}`
  return undefined
}

/** Windows: the PE import closure of the engine (see pe.ts). */
export function windowsRuntime(exe: string, searchDirs: string[], io: RuntimeIo): RuntimePlan {
  const c = resolveRuntime(exe, searchDirs, io)
  const by = (label: string) => (label === exe ? basename(exe) : label)
  return {
    copy: c.copy.map((x) => ({ name: x.name, path: x.path })),
    system: c.system,
    missing: c.missing.map((m) => `${m.name} (needed by ${by(m.neededBy)})`),
    crt: c.crt.map((m) => `${m.name} (needed by ${by(m.neededBy)})`),
  }
}

/** Linux and macOS (not yet tried): the libraries the dynamic linker resolves from the work folder are shipped. */
export function unixRuntime(exe: string, work: WorkPaths, platform: Platform, libDirs: string[]): RuntimePlan {
  const plan: RuntimePlan = { copy: [], system: [], missing: [], crt: [] }
  if (platform === 'linux') {
    const r = capture('ldd', [exe], { env: engineEnv(process.env, libDirs, platform) })
    if (r.code !== 0) throw new StepError(`ldd ${exe} failed: ${r.stderr.trim()}`)
    for (const lib of parseLdd(r.stdout)) {
      if (lib.path === undefined) plan.missing.push(lib.name)
      else if (isUnixSystemLibrary(lib.path, work.home)) plan.system.push(lib.name)
      else plan.copy.push({ name: lib.name, path: lib.path })
    }
  } else {
    const r = capture('otool', ['-L', exe])
    if (r.code !== 0) throw new StepError(`otool -L ${exe} failed: ${r.stderr.trim()}`)
    for (const name of parseOtoolL(r.stdout)) {
      if (name.startsWith('/usr/lib/') || name.startsWith('/System/')) {
        plan.system.push(name)
        continue
      }
      const base = name.split('/').pop()!
      const found = libDirs.map((d) => join(d, base)).find((p) => existsSync(p))
      if (found) plan.copy.push({ name: base, path: found })
      else plan.missing.push(name)
    }
  }
  return plan
}

// ---------------------------------------------------------------- the manifest

export interface ManifestParts {
  lock: OrcaLock
  engineVersion: string
  exeName: string
  platform: Platform
  arch: string
  patches: { name: string; sha256: string }[]
  cli: string
  runtime: RuntimePlan
  smokePassed: boolean
  inputs: string
  files: FileEntry[]
  stagedAt: string
}

export function buildManifest(p: ManifestParts): EngineManifest {
  return {
    format: ENGINE_FORMAT,
    stageVersion: STAGE_VERSION,
    engine: { name: 'sketch-orca', version: p.engineVersion, exe: `bin/${p.exeName}`, resources: 'resources', libraryPath: 'bin' },
    orca: { repo: p.lock.repo, tag: p.lock.tag, version: p.lock.version, commit: p.lock.commit, publishedAt: p.lock.publishedAt, license: p.lock.license },
    platform: p.platform,
    arch: p.arch,
    patches: p.patches.map((x) => ({ name: x.name, sha256: x.sha256 })),
    cli: p.cli,
    runtime: p.runtime.copy.map((c) => c.name).sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())),
    system: [...p.runtime.system].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())),
    smoke: { passed: p.smokePassed, smokeVersion: SMOKE_VERSION },
    inputs: p.inputs,
    stagedAt: p.stagedAt,
    files: [...p.files].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)),
  }
}

/** The hash of what staging reads: everything that changes the folder except the time. */
export function stageInputs(o: {
  lock: OrcaLock
  patches: { name: string; sha256: string }[]
  cli: string
  exeSha256: string
  runtime: { name: string; sha256: string }[]
  licenses: { to: string; sha256: string }[]
  smokePassed: boolean
  platform: Platform
}): string {
  return sha256Text(
    JSON.stringify({
      stageVersion: STAGE_VERSION,
      lock: o.lock,
      patches: o.patches.map((p) => [p.name, p.sha256]),
      cli: o.cli,
      exe: o.exeSha256,
      runtime: o.runtime.map((r) => [r.name, r.sha256]).sort(),
      licenses: o.licenses.map((l) => [l.to, l.sha256]).sort(),
      resources: STAGED_RESOURCES,
      smoke: o.smokePassed,
      smokeVersion: SMOKE_VERSION,
      platform: o.platform,
    }),
  )
}

/** The JSON object `sketch-orca version` prints (the last line that parses). */
export function parseVersionOutput(stdout: string): Record<string, unknown> | undefined {
  const lines = stdout.trim().split(/\r?\n/).reverse()
  for (const l of [stdout.trim(), ...lines]) {
    try {
      const v: unknown = JSON.parse(l)
      if (isRecord(v)) return v
    } catch {
      // Not this line.
    }
  }
  return undefined
}

/** An environment with no library folders: the system's own folders only. */
export function isolatedEnv(base: NodeJS.ProcessEnv, platform: Platform): Record<string, string> {
  if (platform === 'win32') {
    const root = base.SystemRoot ?? base.SYSTEMROOT ?? 'C:\\Windows'
    const env: Record<string, string> = { SystemRoot: root, windir: root, PATH: `${join(root, 'System32')};${root}` }
    for (const k of ['TEMP', 'TMP', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE']) if (base[k]) env[k] = base[k]!
    return env
  }
  return { PATH: '/usr/bin:/bin', HOME: base.HOME ?? '/tmp', TMPDIR: base.TMPDIR ?? '/tmp' }
}

/**
 * The environment a staged engine runs in, as a user of the manifest sets it up: the system's
 * folders only, plus (Linux and macOS) its libraryPath on the library path. On Windows the
 * libraries sit next to the executable, where the loader looks first.
 */
export function stagedEnv(base: NodeJS.ProcessEnv, platform: Platform, binDir: string): Record<string, string> {
  const env = isolatedEnv(base, platform)
  return platform === 'win32' ? env : engineEnv(env, [binDir], platform)
}

// ---------------------------------------------------------------- the step

function findGit(platform: Platform): string {
  const git = firstExisting(gitCandidates(platform, process.env))
  if (!git) throw new StepError('git not found (set SKETCH_ORCA_GIT)')
  return git
}

/** Patches the checkout has applied (state/patches.json), to compare with patches/. */
function appliedPatches(work: WorkPaths): { name: string; sha256: string }[] | undefined {
  const p = join(work.state, 'patches.json')
  if (!existsSync(p)) return undefined
  try {
    const v = JSON.parse(readFileSync(p, 'utf8')) as { applied?: { name: string; sha256: string }[] }
    return v.applied
  } catch {
    return undefined
  }
}

/**
 * Why cli/ (`cliDir`) is not what the last engine build compiled, or undefined when it is:
 * build records hashTree(cli/) before Ninja runs and checks it after (orca.ts).
 */
export function cliChangedSinceBuild(work: WorkPaths, cliDir: string): string | undefined {
  const built = builtCliSha256(work)
  if (!built) return 'the last engine build recorded no hash of cli/ (state/orca.json)'
  if (hashTree(cliDir) !== built) return 'cli/ is not what the engine was built from (it changed after the build)'
  return undefined
}

/** The newest "Modified for Sketch, <date>." of the patch headers (AGPL-3.0 section 5a). */
export function modifiedDate(patches: PatchInfo[]): string | undefined {
  const dates = patches.map((p) => /^Modified for Sketch, (\d{4}-\d{2}-\d{2})\.$/m.exec(readFileSync(p.path, 'utf8').split('\n---\n')[0] ?? '')?.[1]).filter((d): d is string => !!d)
  return dates.sort().at(-1)
}

export async function runStage(ctx: Context, exe: string, log: Log): Promise<number> {
  const w = ctx.work
  const platform = currentPlatform()
  const without = ctx.args.includes('--without-smoke')
  if (!existsSync(exe)) throw new StepError(`no engine at ${exe}; run build first`)
  const git = findGit(platform)
  if (headCommit(git, w.src) !== ctx.lock.commit) throw new StepError(`${w.src} is not at the locked commit ${ctx.lock.commit}; run fetch and build`)
  const patchFiles = listPatches(ctx.engine.patches)
  const patches = patchFiles.map((p) => ({ name: p.name, sha256: p.sha256 }))
  const applied = appliedPatches(w)
  if (!applied || JSON.stringify(applied) !== JSON.stringify(patches)) throw new StepError('the checkout does not have patches/ applied as they are now; run build')
  const modified = modifiedDate(patchFiles)
  if (patchFiles.length > 0 && !modified) throw new StepError('no patch header says "Modified for Sketch, <date>." (AGPL-3.0 section 5a)')
  if (!readStamp(w.state, 'orca')) throw new StepError('no finished engine build is recorded (state/orca.json); run build')
  const cliProblem = cliChangedSinceBuild(w, ctx.engine.cli)
  if (cliProblem) throw new StepError(`${cliProblem}; run build`)
  const smokePassed = smokePassedFor(w, exe)
  if (!smokePassed && !without) throw new StepError('smoke has not passed for this engine build; run smoke (or stage --without-smoke for a dev build, recorded in the manifest)')
  if (!smokePassed) log.line('stage: staging WITHOUT a passed smoke run (--without-smoke); the manifest says so')

  const libDirs = engineLibraryDirs(w, exe, platform)
  const runtime = platform === 'win32' ? windowsRuntime(exe, libDirs, windowsRuntimeIo()) : unixRuntime(exe, w, platform, libDirs)
  const runtimeProblem = runtimeRefusal(runtime)
  if (runtimeProblem) throw new StepError(runtimeProblem)
  const licenses = [...licenseCopies(w, platform, ctx.engine.licenses), ...ownNoticeCopies(ctx.engine)]
  const unlicensed = unlicensedDependencies(w, licenses)
  if (unlicensed.length > 0) {
    throw new StepError(`no licence text found for the built dependencies ${unlicensed.join(', ')}; name their files in EXTRA_LICENSE_FILES or OWN_LICENSE_TEXTS (scripts/lib/stage.ts)`)
  }
  const missingTexts = requiredLicenses(platform).filter((to) => !licenses.some((c) => c.to === to))
  if (missingTexts.length > 0) throw new StepError(`licence texts missing: ${missingTexts.join(', ')}`)
  const buildNinja = join(w.build, 'build.ninja')
  const libs = existsSync(buildNinja) ? linkedLibraries(readFileSync(buildNinja, 'utf8')) : []
  if (libs.length === 0) throw new StepError(`cannot read the engine's link line from ${buildNinja}; run build`)
  const depsSrc = join(w.src, 'deps_src')
  const depsSrcNames = existsSync(depsSrc) ? readdirSync(depsSrc, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name) : []
  const linkProblems = linkedPartProblems(libs, depsSrcNames, licenses)
  if (linkProblems.length > 0) throw new StepError(linkProblems.join('; '))
  // What the engine was built from (the build checked that cli/ did not change meanwhile).
  const cli = builtCliSha256(w)!
  const inputs = stageInputs({
    lock: ctx.lock,
    patches,
    cli,
    exeSha256: sha256File(exe),
    runtime: runtime.copy.map((c) => ({ name: c.name, sha256: sha256File(c.path) })),
    licenses: licenses.map((l) => ({ to: l.to, sha256: sha256File(l.from) })),
    smokePassed,
    platform,
  })

  // Nothing staged names the builder or the build machine (a staged engine that is current too).
  const needles = [...builderNeedles(ctx, platform, git), ...USER_FOLDER_NEEDLES]
  const dist = join(w.dist, 'engine')
  if (existsSync(join(dist, MANIFEST_FILE))) {
    try {
      const old = readManifest(dist)
      if (old.inputs === inputs) {
        const problems = verifyStaged(dist, old)
        const personal = problems.length === 0 ? privacyProblems(dist, [...old.files.map((f) => f.path), MANIFEST_FILE], needles) : []
        if (problems.length === 0 && personal.length === 0) {
          log.line(`stage: ${dist} is current (${old.files.length} files); skipping`)
          return 0
        }
        if (personal.length > 0) log.line(`stage: the staged engine names the builder or the build machine (${personal.slice(0, 3).join('; ')}); staging again`)
        else log.line(`stage: the staged engine no longer matches its manifest (${problems.slice(0, 3).join('; ')}); staging again`)
      }
    } catch (err) {
      log.line(`stage: the old manifest cannot be read (${(err as Error).message}); staging again`)
    }
  }

  // The engine must name the locked release.
  const engineLog = join(w.logs, 'stage-engine.log')
  const env = engineEnv(process.env, libDirs, platform)
  const ver = await runEngine(exe, ['version'], { cwd: w.home, env, logPath: engineLog, timeoutMs: 60_000 })
  if (ver.timedOut || ver.code !== 0) throw new StepError(`${exe} version ${ver.timedOut ? 'did not finish' : `exited with ${ver.code}`} (log: ${engineLog})`)
  const verProblems = checkVersion(ver.stdout, ctx.lock)
  if (verProblems.length > 0) throw new StepError(`the engine does not report the lock: ${verProblems.join('; ')}`)
  const engineVersion = String(parseVersionOutput(ver.stdout)?.engineVersion ?? '')

  const tmp = join(w.dist, 'engine.staging')
  rmSync(tmp, { recursive: true, force: true })
  const exeName = platform === 'win32' ? 'sketch-orca.exe' : 'sketch-orca'
  const copies: StageCopy[] = [
    { from: exe, to: `bin/${exeName}` },
    ...runtime.copy.map((c) => ({ from: c.path, to: `bin/${c.name}` })),
    ...resourceCopies(w.src, (d) => listFiles(d)),
    ...licenses,
  ]
  log.line(`stage: copying ${copies.length} files into ${tmp}`)
  for (const c of copies) {
    if (c.notice !== undefined) writeText(nativePath(tmp, c.to), noticeText(c.notice, c.from))
    else copyFile(c.from, nativePath(tmp, c.to))
  }
  writeText(join(tmp, 'LICENSES', 'README.txt'), licensesReadme(ctx.lock, modified))
  const manifest = buildManifest({
    lock: ctx.lock,
    engineVersion,
    exeName,
    platform,
    arch: process.arch,
    patches,
    cli,
    runtime,
    smokePassed,
    inputs,
    files: treeEntries(tmp),
    stagedAt: new Date().toISOString(),
  })
  writeText(join(tmp, MANIFEST_FILE), serializeManifest(manifest))

  const personal = privacyProblems(tmp, [...manifest.files.map((f) => f.path), MANIFEST_FILE], needles)
  if (personal.length > 0) throw new StepError(privacyRefusal('staged files', personal))

  // The staged copy runs on its own: its libraries all resolve inside bin/, and it starts with no library folder on PATH.
  const stagedExe = join(tmp, 'bin', exeName)
  if (platform === 'win32') {
    const own = windowsRuntime(stagedExe, [join(tmp, 'bin')], windowsRuntimeIo())
    if (own.missing.length > 0) throw new StepError(`the staged engine misses ${own.missing.join(', ')}`)
  }
  const ownEnv = stagedEnv(process.env, platform, join(tmp, 'bin'))
  const staged = await runEngine(stagedExe, ['version'], { cwd: tmp, env: ownEnv, logPath: engineLog, timeoutMs: 60_000 })
  const stagedProblems = staged.code === 0 && !staged.timedOut ? checkVersion(staged.stdout, ctx.lock) : [`it exited with ${staged.code}${staged.timedOut ? ' (timed out)' : ''}`]
  if (stagedProblems.length > 0) throw new StepError(`the staged engine does not run on its own: ${stagedProblems.join('; ')} (log: ${engineLog})`)
  // And it slices with what it ships: the staged resources, read through its own profiles.
  if (smokePassed) {
    const root = join(w.smoke, 'staged')
    rmSync(root, { recursive: true, force: true })
    const resources = join(tmp, 'resources')
    const cases = SMOKE_CASES.filter((c) => (STAGED_CASES as readonly string[]).includes(c.id))
    const done = await runCases(
      cases,
      { exe: stagedExe, resources, env: ownEnv, root, lock: ctx.lock, sets: smokePresetSets(join(resources, 'profiles')), timeoutMs: smokeTimeoutMs() },
      log,
      'stage',
    )
    const failed = done.filter((c) => !c.ok).map((c) => c.id)
    if (failed.length > 0) throw new StepError(`the staged engine fails ${failed.join(', ')} with its own resources (details in ${root})`)
  }

  const old = join(w.dist, 'engine.old')
  rmSync(old, { recursive: true, force: true })
  try {
    if (existsSync(dist)) renameSync(dist, old)
    renameSync(tmp, dist)
  } catch (err) {
    throw new StepError(`cannot replace ${dist} (is the engine running from it?): ${(err as Error).message}`)
  }
  rmSync(old, { recursive: true, force: true })
  const bytes = manifest.files.reduce((n, f) => n + f.size, 0)
  writeStamp(w.state, 'stage', inputs, { files: manifest.files.length, bytes, manifestSha256: sha256Bytes(serializeManifest(manifest)) })
  log.line(`stage: ${dist}: sketch-orca ${engineVersion} on Orca ${ctx.lock.tag}, ${manifest.runtime.length} libraries, ${manifest.files.length} files, ${(bytes / 1048576).toFixed(1)} MB`)
  return 0
}

/** The user name for personalPathNeedles; empty when the system cannot say. */
function safeUserName(): string {
  try {
    return userInfo().username
  } catch {
    return process.env.USERNAME ?? process.env.USER ?? ''
  }
}

/** LICENSES/README.txt; `modified` is the newest date of the patch headers (modifiedDate). */
export function licensesReadme(lock: OrcaLock, modified?: string): string {
  return [
    'Licences of the sketch-orca engine',
    '',
    `sketch-orca is a modified build of OrcaSlicer ${lock.tag} (https://github.com/${lock.repo}),`,
    `licensed under the ${lock.license === 'AGPL-3.0' ? 'GNU Affero General Public License version 3' : lock.license} (OrcaSlicer/LICENSE.txt).`,
    ...(modified ? [`Modified for Sketch, ${modified}: the changes are the patches of the source bundle`, '(engine/patches, each dated in its header) and the command-line front end (engine/cli).'] : []),
    'Its complete corresponding source (OrcaSlicer at the commit the manifest names, the',
    'changes made to it, the command-line front end and the build scripts) is offered under',
    'the same licence wherever the engine is distributed.',
    '',
    'NOTICE.txt  what sketch-orca is based on (OrcaSlicer, Bambu Studio, PrusaSlicer, Slic3r) and',
    '            the code of others it carries',
    'AUTHORS.txt the copyright holders of the changes',
    'deps/       licences of the libraries built from the source packages OrcaSlicer pins',
    'deps_src/   licences of the third-party code kept in the OrcaSlicer tree; for parts whose',
    '            licence is stated in their sources, that notice (<file>.notice.txt)',
    '',
  ].join('\n')
}
