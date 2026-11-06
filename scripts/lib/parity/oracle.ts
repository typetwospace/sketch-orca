// The parity oracle: OrcaSlicer at the locked commit with patches/ and then parity/oracle/patches/
// applied, and our front end, built as the engine is (the same dependency build, toolchain,
// configuration and MSVC static runtime) into <work>/oracle, and staged as
// <work>/oracle/dist/bin/sketch-orca-oracle.exe with the linker map beside it and the engine's
// resources. It never ships: it is the reference the Rust translation is checked against. Its
// patches add knobs (SKETCH_ORCA_ORACLE_*, parity/oracle/README.md), each of which acts only when
// it is set, so with none set the oracle is the engine (parity neutral checks that).
//
// <work>/oracle/src is a clone of <work>/src's commit (a local clone: the objects are shared), with
// cli/ copied to src/sketch-orca/cli, where SKETCH_ORCA_CLI_DIR points and the oracle patches find
// it. When the patches or cli/ change, the new tree is made beside it (src.new) and only the files
// that differ are written into src, so Ninja rebuilds what changed and nothing else. The
// dependency build is the engine's (<work>/deps), shared and never rebuilt here: the oracle builds
// only on a finished dependency build of the inputs `build` works out now. What the clone and the
// build were made from is recorded in <work>/oracle/state; what the staged copy is of, in
// <work>/oracle/dist/oracle.json, which parity neutral checks against this checkout.

import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, statfsSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { depsPatchesOf, headCommit, listPatches, sourceDigest, type PatchInfo } from '../checkout.ts'
import { formatCommand, orcaBuildArgs, orcaConfigureArgs, defaultJobs, type OrcaArgs } from '../cmake.ts'
import { compilerEnv as sharedCompilerEnv, depsWant as sharedDepsWant } from '../depsinputs.ts'
import type { Platform } from '../env.ts'
import { copyFile, copyTree, hashTree, listFiles, nativePath } from '../files.ts'
import type { OrcaLock } from '../lock.ts'
import { assertNoIdeProjects } from '../noide.ts'
import { depsStampProblems, parityPathProblem, parityPaths, readStamp, sha256File, sha256Text, workPathProblem, writeStamp, type ParityPaths, type WorkPaths } from '../paths.ts'
import { resolveRuntime, windowsRuntimeIo, type RuntimeIo } from '../pe.ts'
import { captureOk, runLogged, StepError, type Log } from '../run.ts'
import { ENGINE_RESOURCES, engineLibraryDirs } from '../smoke.ts'
import type { Toolchain } from '../toolchain.ts'

export const ORACLE_FORMAT = 'sketch-orca-oracle/1'

/** The free space parity oracle needs on the work folder's drive before it starts. */
export const ORACLE_MIN_FREE_BYTES = 40 * 1024 ** 3

/** Ninja's parallel jobs for the oracle build: at most this many. */
export const ORACLE_MAX_JOBS = 8

/** The knobs (parity/oracle/README.md); each acts only when set. */
export const ORACLE_KNOBS = {
  time: 'SKETCH_ORCA_ORACLE_TIME',
  seed: 'SKETCH_ORCA_ORACLE_SEED',
  threads: 'SKETCH_ORCA_ORACLE_THREADS',
  fma3: 'SKETCH_ORCA_ORACLE_FMA3',
  dump: 'SKETCH_ORCA_ORACLE_DUMP',
} as const

/** The oracle's exit code for a knob set to something it cannot read (cli/main.cpp, patch 0001). */
export const ORACLE_KNOB_EXIT = 78

/** The oracle's program, as staged. */
export const ORACLE_EXE = 'sketch-orca-oracle.exe'
export const ORACLE_MAP = 'sketch-orca-oracle.map'

/** parity/oracle/patches of this repository. */
export function oraclePatchesDir(engineDir: string): string {
  return join(engineDir, 'parity', 'oracle', 'patches')
}

// ---------------------------------------------------------------- the patches

export interface OraclePatchHeader {
  number: string
  title: string
  date: string
}

const PATCH_NAME_RE = /^(\d{4})-oracle-[a-z0-9]+(?:-[a-z0-9]+)*\.patch$/
const PATCH_TITLE_RE = /^sketch-orca oracle patch (\d{4}): (.+)$/
const PATCH_DATE_RE = /^Modified for Sketch, (\d{4}-\d{2}-\d{2})\.$/

function isDate(s: string): boolean {
  const d = new Date(`${s}T00:00:00Z`)
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === s
}

/** The header of an oracle patch: its first two lines. */
export function oraclePatchHeader(text: string): OraclePatchHeader | undefined {
  const [first = '', second = ''] = text.split('\n', 2)
  const t = PATCH_TITLE_RE.exec(first)
  const d = PATCH_DATE_RE.exec(second)
  if (!t || !d) return undefined
  return { number: t[1]!, title: t[2]!, date: d[1]! }
}

/**
 * What is wrong with an oracle patch: its name (NNNN-oracle-<words>.patch), its header
 * ('sketch-orca oracle patch NNNN: <title>' with the name's number, then 'Modified for Sketch,
 * <date>.' with a real date), a '---' line between the description and the diff, and the diff
 * itself (at least one file). Empty when it is right.
 */
export function oraclePatchProblems(name: string, text: string): string[] {
  const problems: string[] = []
  const n = PATCH_NAME_RE.exec(name)
  if (!n) problems.push(`${name}: an oracle patch is named NNNN-oracle-<words>.patch`)
  const h = oraclePatchHeader(text)
  if (!h) {
    problems.push(`${name}: the header is not 'sketch-orca oracle patch NNNN: <title>' then 'Modified for Sketch, <date>.'`)
  } else {
    if (n && h.number !== n[1]) problems.push(`${name}: the header says patch ${h.number}, the name ${n[1]}`)
    if (!isDate(h.date)) problems.push(`${name}: ${h.date} is not a date`)
  }
  if (text.includes('\r')) problems.push(`${name}: has CR bytes (the patches are LF, as the checkout is)`)
  const diffAt = text.search(/^diff --git /m)
  if (diffAt < 0) problems.push(`${name}: has no diff`)
  else if (!/^---\n/m.test(text.slice(0, diffAt))) problems.push(`${name}: no '---' line between the description and the diff`)
  return problems
}

/** parity/oracle/patches, each checked (oraclePatchProblems); throws naming every problem. */
export function oraclePatches(engineDir: string): PatchInfo[] {
  const patches = listPatches(oraclePatchesDir(engineDir))
  if (patches.length === 0) throw new StepError(`no oracle patches in ${oraclePatchesDir(engineDir)}`)
  const problems = patches.flatMap((p) => oraclePatchProblems(p.name, readFileSync(p.path, 'utf8')))
  if (problems.length > 0) throw new StepError(`the oracle patches are not right: ${problems.join('; ')}`)
  return patches
}

// ---------------------------------------------------------------- refusals

/** Why there is not enough room for the oracle (`free` bytes free on the work folder's drive), or undefined. */
export function freeSpaceProblem(free: number, where: string, min = ORACLE_MIN_FREE_BYTES): string | undefined {
  if (free >= min) return undefined
  const gb = (b: number) => (b / 1024 ** 3).toFixed(1)
  return `parity oracle needs ${gb(min)} GB free on the drive of ${where} (the oracle's clone and build); there are ${gb(free)} GB`
}

/** The bytes free for this user on the drive of `dir` (or of its nearest folder that exists). */
export function freeBytes(dir: string): number {
  let d = dir
  while (!existsSync(d) && dirname(d) !== d) d = dirname(d)
  const s = statfsSync(d)
  return Number(s.bavail) * Number(s.bsize)
}

/** Why the oracle cannot be built in this work folder: its path's length, then each parity folder's. */
export function oraclePathProblem(home: string, platform: Platform): string | undefined {
  return workPathProblem(home, platform) ?? parityPathProblem(parityPaths({ home }), home, platform)
}

/** The refusals before anything is cloned or built: the platform, the path lengths and the free space. */
export function oracleRefusals(o: { home: string; platform: Platform; free: number }): string[] {
  const out: string[] = []
  if (o.platform !== 'win32') out.push('parity oracle builds on Windows only: the oracle is the Windows x64 engine with its knobs (the FMA3 knob is the UCRT\'s)')
  const path = oraclePathProblem(o.home, o.platform)
  if (path) out.push(path)
  const space = freeSpaceProblem(o.free, o.home)
  if (space) out.push(space)
  return out
}

// ---------------------------------------------------------------- the source

/** Where the oracle keeps its records: <work>/oracle/state. */
export function oracleStateDir(paths: Pick<ParityPaths, 'oracle'>): string {
  return join(paths.oracle, 'state')
}

/** The front end's folder in the oracle's source: SKETCH_ORCA_CLI_DIR. */
export function oracleCliDir(paths: Pick<ParityPaths, 'oracleSrc'>): string {
  return join(paths.oracleSrc, 'sketch-orca', 'cli')
}

export interface OracleSourceInputs {
  commit: string
  patches: { name: string; sha256: string }[]
  oraclePatches: { name: string; sha256: string }[]
  cli: string
}

export interface OracleSourceRecord extends OracleSourceInputs {
  /** checkout.ts sourceDigest of <work>/oracle/src against the commit, as made. */
  sourceDigest: string
}

const strip = (p: PatchInfo[]) => p.map((x) => ({ name: x.name, sha256: x.sha256 }))

export function oracleSourceInputs(lock: Pick<OrcaLock, 'commit'>, patches: PatchInfo[], oracle: PatchInfo[], cliDir: string): OracleSourceInputs {
  return { commit: lock.commit, patches: strip(patches), oraclePatches: strip(oracle), cli: hashTree(cliDir) }
}

function sameInputs(a: OracleSourceInputs, b: OracleSourceInputs): boolean {
  const k = (x: OracleSourceInputs) => JSON.stringify([x.commit, x.patches, x.oraclePatches, x.cli])
  return k(a) === k(b)
}

/** The files of a checkout that differ from `commit` or that git does not track (as sourceDigest counts them). */
function changedPaths(git: string, src: string, commit: string): string[] {
  const base = ['--no-optional-locks', '--literal-pathspecs', '-C', src, '-c', 'core.quotePath=false']
  const diff = captureOk(git, [...base, 'diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', commit, '--']).split('\0')
  const others = captureOk(git, [...base, 'ls-files', '-z', '--others']).split('\0')
  return [...new Set([...diff, ...others].filter((p) => p !== ''))].sort()
}

/** Makes `dir` a clone of the engine's checkout at the commit, with patches/, cli/ and the oracle patches applied. */
async function makeOracleTree(o: { git: string; from: string; dir: string; commit: string; patches: PatchInfo[]; oracle: PatchInfo[]; cli: string; log: Log }): Promise<void> {
  rmSync(o.dir, { recursive: true, force: true })
  mkdirSync(dirname(o.dir), { recursive: true })
  const env = { ...(process.env as Record<string, string>), GIT_CEILING_DIRECTORIES: dirname(o.dir) }
  await runLogged(o.git, ['-c', 'core.autocrlf=false', '-c', 'core.longpaths=true', 'clone', '--quiet', '--no-checkout', '--config', 'core.autocrlf=false', '--config', 'core.eol=lf', '--config', 'core.longpaths=true', '--config', 'advice.detachedHead=false', o.from, o.dir], { log: o.log, env })
  await runLogged(o.git, ['-C', o.dir, 'checkout', '--quiet', '--detach', o.commit], { log: o.log, env })
  if (headCommit(o.git, o.dir) !== o.commit) throw new StepError(`${o.dir} is not at ${o.commit} after the checkout`)
  for (const p of o.patches) await runLogged(o.git, ['-C', o.dir, 'apply', '--whitespace=nowarn', p.path], { log: o.log, env })
  copyTree(o.cli, join(o.dir, 'sketch-orca', 'cli'))
  for (const p of o.oracle) await runLogged(o.git, ['-C', o.dir, 'apply', '--whitespace=nowarn', p.path], { log: o.log, env })
}

function sameBytes(a: string, b: string): boolean {
  if (!existsSync(a) || !existsSync(b)) return false
  return readFileSync(a).equals(readFileSync(b))
}

/**
 * <work>/oracle/src as the inputs say: kept when it is what the record says it was made as, made
 * when missing, and otherwise made again beside it (src.new) with only the files that differ
 * written into it (those that differ from the commit in either tree are the only ones that can).
 */
export async function prepareOracleSource(o: { git: string; lock: OrcaLock; work: WorkPaths; paths: ParityPaths; patches: PatchInfo[]; oracle: PatchInfo[]; cli: string; log: Log }): Promise<OracleSourceRecord> {
  const say = (m: string) => o.log.line(`parity oracle: ${m}`)
  const inputs = oracleSourceInputs(o.lock, o.patches, o.oracle, o.cli)
  const state = oracleStateDir(o.paths)
  const recorded = readStamp(state, 'source')?.detail as OracleSourceRecord | undefined
  const src = o.paths.oracleSrc
  const isClone = headCommit(o.git, src) === o.lock.commit
  if (isClone && recorded && sameInputs(recorded, inputs) && sourceDigest(o.git, src, o.lock.commit) === recorded.sourceDigest) {
    say(`source: ${src} is the commit with patches/, cli/ and the oracle patches as they are; kept`)
    return recorded
  }
  if (headCommit(o.git, o.work.src) !== o.lock.commit) throw new StepError(`${o.work.src} is not a checkout of ${o.lock.commit}; run build first (the oracle is cloned from the engine's checkout)`)
  rmSync(join(state, 'source.json'), { force: true })
  const fresh = `${src}.new`
  say(`source: cloning ${o.work.src} at ${o.lock.commit} into ${fresh}, applying ${o.patches.length} patches, cli/ and ${o.oracle.length} oracle patches`)
  await makeOracleTree({ git: o.git, from: o.work.src, dir: fresh, commit: o.lock.commit, patches: o.patches, oracle: o.oracle, cli: o.cli, log: o.log })
  const digest = sourceDigest(o.git, fresh, o.lock.commit)
  if (!isClone) {
    rmSync(src, { recursive: true, force: true })
    renameSync(fresh, src)
    say(`source: ${src} made`)
  } else {
    const paths = [...new Set([...changedPaths(o.git, src, o.lock.commit), ...changedPaths(o.git, fresh, o.lock.commit)])].sort()
    let written = 0
    let removed = 0
    for (const rel of paths) {
      const from = nativePath(fresh, rel)
      const to = nativePath(src, rel)
      if (existsSync(from)) {
        if (!sameBytes(from, to)) {
          copyFile(from, to)
          written++
        }
      } else if (existsSync(to)) {
        rmSync(to, { force: true })
        removed++
      }
    }
    rmSync(fresh, { recursive: true, force: true })
    say(`source: ${src} brought up to date: ${written} files written, ${removed} removed, of the ${paths.length} that differ from the commit in either tree`)
  }
  const now = sourceDigest(o.git, src, o.lock.commit)
  if (now !== digest) throw new StepError(`${src} is not the commit with patches/, cli/ and the oracle patches applied after making it (a file there that git does not see?)`)
  const record: OracleSourceRecord = { ...inputs, sourceDigest: digest }
  writeStamp(state, 'source', sha256Text(JSON.stringify(inputs)), record)
  return record
}

// ---------------------------------------------------------------- the build

/** The engine's configuration (cmake.ts orcaConfigureArgs) with the linker map on: /MAP. */
export function oracleConfigureArgs(a: OrcaArgs): string[] {
  const args = orcaConfigureArgs(a)
  // CMake's MSVC platform module appends /machine:x64 to the _INIT flags, so the cache gets
  // "/MAP /machine:x64": the engine's flags plus the map.
  if (a.platform === 'win32') args.push('-DCMAKE_EXE_LINKER_FLAGS_INIT=/MAP')
  return args
}

/** Ninja's jobs for the oracle: the engine's (cmake.ts defaultJobs), never more than ORACLE_MAX_JOBS. */
export function oracleJobs(env: NodeJS.ProcessEnv = process.env): number {
  return Math.max(1, Math.min(ORACLE_MAX_JOBS, defaultJobs(env)))
}

export interface OracleBuild {
  exe: string
  map: string
  configure: string[]
  depsInputs: string
  compilerEnv: Record<string, string>
}

/**
 * Configures (when its inputs changed) and builds the oracle in <work>/oracle/build, with Ninja,
 * on the engine's finished dependency build. The build folder starts over when the compiler
 * environment or the dependency build changes, as the engine's does.
 */
export async function buildOracle(o: { t: Toolchain; lock: OrcaLock; work: WorkPaths; paths: ParityPaths; engineDir: string; patches: PatchInfo[]; source: OracleSourceRecord; log: Log }): Promise<OracleBuild> {
  const say = (m: string) => o.log.line(`parity oracle: ${m}`)
  const w = o.work
  const state = oracleStateDir(o.paths)
  const deps = sharedDepsWant(o.lock, w, o.t, o.engineDir, depsPatchesOf(o.patches))
  const depsStamp = readStamp(w.state, 'deps')
  let prefixDigest = ''
  const depsProblems = depsStampProblems(depsStamp, { ...deps, prefixDigest: () => (prefixDigest = hashTree(w.depsPrefix)) })
  if (depsProblems.length > 0) throw new StepError(`the engine's dependency build does not fit: ${depsProblems.join('; ')}; run build (the oracle never builds the dependencies)`)
  const cenv = sharedCompilerEnv(o.t, w, o.engineDir)
  const configure = oracleConfigureArgs({
    ninja: o.t.ninja,
    sourceDir: o.paths.oracleSrc,
    buildDir: o.paths.oracleBuild,
    depsPrefix: w.depsPrefix,
    cliDir: oracleCliDir(o.paths),
    commit: o.lock.commit,
    tag: o.lock.tag,
    ignorePrefixes: o.t.ignorePrefixes,
    platform: o.t.platform,
  })
  const build = o.paths.oracleBuild
  const envKey = sha256Text(JSON.stringify(cenv))
  if (existsSync(build) && readStamp(state, 'build-env')?.inputs !== envKey) {
    say(`build: ${build} was built with another compiler environment; starting it over`)
    rmSync(build, { recursive: true, force: true })
  }
  writeStamp(state, 'build-env', envKey, { compilerEnv: cenv })
  if (existsSync(build) && readStamp(state, 'build-deps')?.inputs !== depsStamp!.inputs) {
    say(`build: ${build} was configured against another dependency build; starting it over`)
    rmSync(build, { recursive: true, force: true })
  }
  writeStamp(state, 'build-deps', depsStamp!.inputs)
  mkdirSync(build, { recursive: true })
  const env: Record<string, string> = { ...o.t.env, ...cenv, GIT_CEILING_DIRECTORIES: w.home, ...(o.t.platform === 'win32' ? { MSBUILDDISABLENODEREUSE: '1' } : {}) }
  const configureKey = sha256Text(JSON.stringify({ configure, cmake: o.t.cmake, compilerEnv: cenv }))
  if (readStamp(state, 'configure')?.inputs !== configureKey || !existsSync(join(build, 'CMakeCache.txt'))) {
    say(`build: configuring ${build}`)
    await runLogged(o.t.cmake, configure, { env, log: o.log, cwd: build })
    writeStamp(state, 'configure', configureKey)
  } else {
    say('build: the configuration is current')
  }
  assertNoIdeProjects([build], 'parity oracle')
  const cache = readFileSync(join(build, 'CMakeCache.txt'), 'utf8')
  const linker = /^CMAKE_EXE_LINKER_FLAGS:STRING=(.*)$/m.exec(cache)?.[1] ?? ''
  if (o.t.platform === 'win32' && !/(^|\s)\/MAP(\s|$)/i.test(linker)) throw new StepError(`${build} is configured without /MAP (CMAKE_EXE_LINKER_FLAGS is "${linker}"); remove ${build} and run parity oracle again`)
  const jobs = oracleJobs()
  rmSync(join(state, 'build.json'), { force: true })
  say(`build: building libslic3r and the oracle (-j${jobs}): ${formatCommand(o.t.cmake, orcaBuildArgs(build, jobs))}`)
  await runLogged(o.t.cmake, orcaBuildArgs(build, jobs), { env, log: o.log, cwd: build })
  // Ninja, so no step can run devenv.exe; held to it all the same, as the engine's build is.
  assertNoIdeProjects([build], 'parity oracle')
  say('build: no CompilerIdFortran folder or .vfproj file in the build folder')
  const exe = join(build, 'sketch-orca', o.t.platform === 'win32' ? 'sketch-orca.exe' : 'sketch-orca')
  if (!existsSync(exe)) throw new StepError(`the build finished but ${exe} is missing`)
  const map = findMap(build, exe)
  if (!map) throw new StepError(`the build finished but the linker map sketch-orca.map is neither beside ${exe} nor in ${build}`)
  if (sourceDigest(o.t.git, o.paths.oracleSrc, o.lock.commit) !== o.source.sourceDigest) throw new StepError(`${o.paths.oracleSrc} changed while the oracle was being built; run parity oracle again`)
  if (hashTree(w.depsPrefix) !== prefixDigest) throw new StepError(`${w.depsPrefix} changed while the oracle was being built; run parity oracle again`)
  writeStamp(state, 'build', sha256Text(JSON.stringify({ configureKey, source: o.source.sourceDigest })), { exe, exeSha256: sha256File(exe), map })
  return { exe, map, configure, depsInputs: depsStamp!.inputs, compilerEnv: cenv }
}

/** The linker map /MAP wrote: beside the program, or (an older linker) in the build folder. */
function findMap(build: string, exe: string): string | undefined {
  const beside = exe.replace(/\.exe$/i, '') + '.map'
  if (existsSync(beside)) return beside
  const top = join(build, 'sketch-orca.map')
  return existsSync(top) ? top : undefined
}

// ---------------------------------------------------------------- the staged oracle

export interface OracleManifest {
  format: typeof ORACLE_FORMAT
  stagedAt: string
  platform: Platform
  orca: { tag: string; commit: string }
  patches: { name: string; sha256: string }[]
  oraclePatches: { name: string; sha256: string }[]
  /** files.ts hashTree of cli/ the oracle was built from. */
  cli: string
  sourceDigest: string
  depsInputs: string
  configure: string[]
  knobs: string[]
  exe: { path: string; sha256: string }
  map: { path: string; sha256: string }
  /** The libraries staged beside the program (the PE import closure), with their SHA-256. */
  runtime: { name: string; sha256: string }[]
  resources: { name: string; sha256: string }[]
}

/** The staged oracle's parts, as runEngineCases takes an engine. */
export interface OracleUnderTest {
  label: string
  dir: string
  exe: string
  resources: string
  libraryPath: string
  manifest: OracleManifest
}

/**
 * Stages <work>/oracle/dist: bin/sketch-orca-oracle.exe and bin/sketch-orca-oracle.map, the
 * libraries the program loads (resolveRuntime, as stage finds the engine's), the resources the
 * engine reads (smoke.ts ENGINE_RESOURCES, from the oracle's own source), and oracle.json. Made in
 * dist.new and renamed, so a dist is always whole.
 */
export function stageOracle(o: { paths: ParityPaths; work: WorkPaths; lock: OrcaLock; platform: Platform; source: OracleSourceRecord; build: OracleBuild; io?: RuntimeIo }): OracleManifest {
  const dist = o.paths.oracleDist
  const tmp = `${dist}.new`
  rmSync(tmp, { recursive: true, force: true })
  mkdirSync(join(tmp, 'bin'), { recursive: true })
  copyFile(o.build.exe, join(tmp, 'bin', ORACLE_EXE))
  copyFile(o.build.map, join(tmp, 'bin', ORACLE_MAP))
  const closure = resolveRuntime(o.build.exe, engineLibraryDirs(o.work, o.build.exe, o.platform), o.io ?? windowsRuntimeIo())
  if (closure.missing.length > 0) throw new StepError(`the oracle loads libraries found nowhere: ${closure.missing.map((m) => `${m.name} (for ${m.neededBy})`).join(', ')}`)
  if (closure.crt.length > 0) throw new StepError(`the oracle imports the dynamic C runtime: ${closure.crt.map((c) => `${c.name} (by ${c.neededBy})`).join(', ')}; it is built with /MT, as the engine is`)
  const runtime = closure.copy.map((c) => {
    copyFile(c.path, join(tmp, 'bin', c.name))
    return { name: c.name, sha256: sha256File(c.path) }
  })
  const resources = ENGINE_RESOURCES.map((r) => {
    const from = join(o.paths.oracleSrc, 'resources', r)
    if (!existsSync(from)) throw new StepError(`${from} is missing`)
    copyTree(from, join(tmp, 'resources', r), undefined, { link: true })
    return { name: r, sha256: hashTree(from) }
  })
  const manifest: OracleManifest = {
    format: ORACLE_FORMAT,
    stagedAt: new Date().toISOString(),
    platform: o.platform,
    orca: { tag: o.lock.tag, commit: o.lock.commit },
    patches: o.source.patches,
    oraclePatches: o.source.oraclePatches,
    cli: o.source.cli,
    sourceDigest: o.source.sourceDigest,
    depsInputs: o.build.depsInputs,
    configure: o.build.configure,
    knobs: Object.values(ORACLE_KNOBS),
    exe: { path: `bin/${ORACLE_EXE}`, sha256: sha256File(o.build.exe) },
    map: { path: `bin/${ORACLE_MAP}`, sha256: sha256File(o.build.map) },
    runtime,
    resources,
  }
  writeFileSync(join(tmp, 'oracle.json'), JSON.stringify(manifest, null, 2) + '\n')
  rmSync(dist, { recursive: true, force: true })
  renameSync(tmp, dist)
  return manifest
}

/** Reads <work>/oracle/dist/oracle.json. */
export function readOracleManifest(dist: string): OracleManifest {
  const p = join(dist, 'oracle.json')
  if (!existsSync(p)) throw new StepError(`no staged oracle in ${dist} (oracle.json is missing); run parity oracle`)
  const m = JSON.parse(readFileSync(p, 'utf8')) as OracleManifest
  if (m.format !== ORACLE_FORMAT) throw new StepError(`${p} is ${String(m.format)}, not ${ORACLE_FORMAT}; run parity oracle`)
  return m
}

/**
 * Why the staged oracle is not the one this checkout makes now: the commit, patches/, the oracle
 * patches, cli/, and its program, map and libraries as oracle.json lists them. Empty when it is.
 */
export function oracleProblems(dist: string, m: OracleManifest, want: OracleSourceInputs): string[] {
  const out: string[] = []
  if (m.orca.commit !== want.commit) out.push(`it is built from ${m.orca.commit}, the lock is at ${want.commit}`)
  const key = (p: { name: string; sha256: string }[]) => JSON.stringify(p.map((x) => [x.name, x.sha256]))
  if (key(m.patches) !== key(want.patches)) out.push('it is built with other patches than patches/ has now')
  if (key(m.oraclePatches) !== key(want.oraclePatches)) out.push('it is built with other oracle patches than parity/oracle/patches has now')
  if (m.cli !== want.cli) out.push('it is built from another cli/')
  const files = [m.exe, m.map, ...m.runtime.map((r) => ({ path: `bin/${r.name}`, sha256: r.sha256 }))]
  for (const f of files) {
    const p = nativePath(dist, f.path)
    if (!existsSync(p)) out.push(`${f.path} is missing`)
    else if (sha256File(p) !== f.sha256) out.push(`${f.path} is not the file oracle.json lists`)
  }
  for (const r of m.resources) if (!existsSync(join(dist, 'resources', r.name))) out.push(`resources/${r.name} is missing`)
  return out
}

/** The staged oracle, checked against this checkout (oracleProblems); throws naming what is wrong. */
export function oracleUnderTest(dist: string, want: OracleSourceInputs, label = 'oracle'): OracleUnderTest {
  const manifest = readOracleManifest(dist)
  const problems = oracleProblems(dist, manifest, want)
  if (problems.length > 0) throw new StepError(`the staged oracle in ${dist} is not the one this checkout makes: ${problems.join('; ')}; run parity oracle`)
  return { label, dir: dist, exe: nativePath(dist, manifest.exe.path), resources: join(dist, 'resources'), libraryPath: join(dist, 'bin'), manifest }
}

/** The number of files under each staged resource folder (for the log). */
export function stagedResourceCounts(dist: string): Record<string, number> {
  const out: Record<string, number> = {}
  for (const r of ENGINE_RESOURCES) out[r] = listFiles(join(dist, 'resources', r)).length
  return out
}
