#!/usr/bin/env bun
// The engine pipeline: OrcaSlicer from its GitHub repository to our command-line engine
// `sketch-orca`. Programmatic and repeatable; see ../README.md.
//
//   bun scripts/orca.ts <command>        (or: bun run engine <command>)
//
//   check            latest stable OrcaSlicer release vs the lock (exit 0 nothing to do, 1 newer, 2 tag moved)
//   update [--write] move the lock to a newer stable release (prints the new lock without --write)
//   fetch            shallow clone of the locked tag; the commit must match the lock
//   build [--step deps|orca] [--fresh]
//                    Orca's dependencies without the GUI ones (on Windows, GMP and MPFR with the
//                    pinned MinGW-w64 toolchain, unpacked into <work>/mingw), then libslic3r and
//                    sketch-orca (SLIC3R_GUI=OFF, Release); --fresh starts the dependency build over
//   smoke [--case <id,...>]
//                    slices the test models with the built engine and checks the results
//   stage [--without-smoke]
//                    engine, runtime libraries, resources, licences and a manifest -> dist/engine
//   source [--no-deps] [--allow-missing]
//                    the AGPL source bundle for the staged engine -> dist/source
//   release [--offline] [--no-deps] [--allow-missing]
//                    check, update (only to a newer stable release), fetch, build, smoke, stage
//                    and source, stopping at the first failure
//   env              prints the toolchain and the cleaned build PATH
//
// Exit codes: 0 done; check also 1 (a newer stable release is out) and 2 (the locked tag moved),
// and update and release stop with 2 when the tag moved; 3 the command failed; 64 a command line
// the pipeline does not take (run.ts, EXIT).
//
// Work folder: %LOCALAPPDATA%\SketchDev\orca-engine (SKETCH_ORCA_HOME overrides). Every command
// but env logs to <work>/logs/<command>.log, and each skips what is already done and verified. The
// commands that write there hold <work>/run.lock.d, so only one of them runs at a time. It runs
// locally; there is no CI job.

import { existsSync, mkdirSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { applyPatches, depsPatchesOf, fetchSource, headCommit, listPatches, patchedDigest, sourceDigest, type PatchInfo } from './lib/checkout.ts'
import { defaultDepsJobs, defaultJobs, depsBuildArgs, depsConfigureArgs, depsParallelEnv, jobsCap, orcaBuildArgs, orcaConfigureArgs } from './lib/cmake.ts'
import { firstExisting, gitCandidates, sourcePathTrimEnv } from './lib/env.ts'
import { hashTree } from './lib/files.ts'
import { githubGetter, resolveLatestRelease } from './lib/github.ts'
import { compareLock, lockForRelease, readLock, releaseDecision, serializeLock, writeLock, type OrcaLock, type ReleaseDecision } from './lib/lock.ts'
import { ensureMingw, mingwPinsDigest, mingwRoot } from './lib/mingw.ts'
import { depsStampProblems, enginePaths, readStamp, sha256File, sha256Text, workHome, workPaths, writeStamp, type DepsRecord, type WorkPaths } from './lib/paths.ts'
import { runSteps, stepArgs, type ReleaseStep } from './lib/release.ts'
import { EXIT, Log, runLogged, StepError, UsageError } from './lib/run.ts'
import { takeRunLock } from './lib/runlock.ts'
import type { BuildRecord } from './lib/stage.ts'
import { checkToolchain, currentPlatform, findToolchain, type Toolchain } from './lib/toolchain.ts'

const ENGINE_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const E = enginePaths(ENGINE_DIR)

export interface Context {
  lock: OrcaLock
  work: WorkPaths
  engine: typeof E
  args: string[]
}

function logFor(work: WorkPaths, step: string): Log {
  mkdirSync(work.logs, { recursive: true })
  return new Log(join(work.logs, `${step}.log`))
}

function flag(args: string[], name: string): boolean {
  return args.includes(name)
}

function option(args: string[], name: string): string | undefined {
  const i = args.indexOf(name)
  if (i >= 0 && i + 1 < args.length) return args[i + 1]
  const eq = args.find((a) => a.startsWith(`${name}=`))
  return eq ? eq.slice(name.length + 1) : undefined
}

async function latestDecision(lock: OrcaLock): Promise<{ latest: Awaited<ReturnType<typeof resolveLatestRelease>>; decision: ReleaseDecision }> {
  const latest = await resolveLatestRelease(lock.repo, githubGetter())
  return { latest, decision: releaseDecision(compareLock(lock, latest), lock, latest) }
}

async function cmdCheck(ctx: Context): Promise<number> {
  const log = logFor(ctx.work, 'check')
  const { latest, decision } = await latestDecision(ctx.lock)
  log.line(`latest stable: ${latest.tag} (${latest.commit}, published ${latest.publishedAt})`)
  log.line(`lock:          ${ctx.lock.tag} (${ctx.lock.commit})`)
  log.line(`check: ${decision.message}${decision.update ? '. Run "release" (or "update --write", then fetch, build, smoke, stage and source).' : ''}`)
  // 0 nothing to do, 1 a newer release is out, 2 the tag moved: scripts can branch on it.
  return decision.checkCode
}

async function cmdUpdate(ctx: Context): Promise<number> {
  const log = logFor(ctx.work, 'update')
  const { latest, decision } = await latestDecision(ctx.lock)
  if (!decision.update) {
    log.line(`update: ${decision.message}; the lock stays at ${ctx.lock.tag}`)
    return decision.proceed ? EXIT.ok : EXIT.tagMoved
  }
  const next = lockForRelease(ctx.lock, latest)
  if (!flag(ctx.args, '--write')) {
    log.line(`update: would move the lock from ${ctx.lock.tag} to ${latest.tag}; run with --write to write it:`)
    process.stdout.write(serializeLock(next))
    return 0
  }
  writeLock(E.lock, next)
  log.line(`update: lock moved from ${ctx.lock.tag} (${ctx.lock.commit}) to ${next.tag} (${next.commit})`)
  return 0
}

async function cmdFetch(ctx: Context, tools?: Pick<Toolchain, 'git'>): Promise<number> {
  const log = logFor(ctx.work, 'fetch')
  const git = tools?.git ?? findGitOnly()
  const what = await fetchSource(git, ctx.lock, ctx.work.src, log)
  log.line(`fetch: ${what}; ${ctx.work.src} is at ${ctx.lock.tag} = ${ctx.lock.commit}`)
  return 0
}

function findGitOnly(): string {
  const git = firstExisting(gitCandidates(currentPlatform(), process.env))
  if (!git) throw new StepError('git not found (set SKETCH_ORCA_GIT)')
  return git
}

async function cmdEnv(ctx: Context): Promise<number> {
  const t = findToolchain()
  const notes = checkToolchain(t)
  console.log(JSON.stringify({ work: ctx.work.home, vs: t.vsInstall, vcvars: t.vcvars, cmake: t.cmake, ninja: t.ninja, perl: t.perl, git: t.git, ignorePrefixes: t.ignorePrefixes, notes, PATH: t.env.PATH?.split(t.platform === 'win32' ? ';' : ':') }, null, 2))
  return 0
}

/**
 * The compiler settings of both builds that come from the environment: the work folder and this
 * checkout are trimmed from __FILE__, so the binaries do not carry the builder's user name
 * (env.ts, sourcePathTrimEnv).
 */
function compilerEnv(t: Toolchain, w: WorkPaths): Record<string, string> {
  return sourcePathTrimEnv(t.platform, [w.home, E.engine])
}

/**
 * The environment of a build step. CMake runs inside the work folder, and git may not look above
 * it: Orca's deps decide how to apply their patches by asking git whether the current folder is
 * inside a repository (it must not find this repo, or a repo in a parent folder).
 */
function stepEnv(t: Toolchain, w: WorkPaths): Record<string, string> {
  return { ...t.env, ...compilerEnv(t, w), GIT_CEILING_DIRECTORIES: w.home }
}

/**
 * Starts the build folder `dir` over when it was built with another compiler environment: Ninja
 * and ExternalProject track command lines and files, not the environment, so what was compiled
 * before a change of CL would stay as it was. state/<name>-env.json names the environment the
 * folder is being built with; it is written before the build starts, so a failed build that runs
 * again keeps what it already compiled.
 */
function matchCompilerEnv(w: WorkPaths, name: string, dir: string, env: Record<string, string>, log: Log): void {
  const want = sha256Text(JSON.stringify(env))
  if (existsSync(dir) && readStamp(w.state, `${name}-env`)?.inputs !== want) {
    log.line(`${name}: ${dir} was built with another compiler environment; starting it over`)
    rmSync(dir, { recursive: true, force: true })
  }
  writeStamp(w.state, `${name}-env`, want, { compilerEnv: env })
}

function depsConfigure(t: Toolchain, w: WorkPaths): string[] {
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

/**
 * What the dependency build is of, as build works it out now: the locked commit, the patches of
 * deps/, its configuration, CMake and the compiler settings, and on Windows the pinned MinGW-w64
 * toolchain that builds GMP and MPFR (mingw.ts). The engine is built only on a finished dependency
 * build of these (depsStampProblems).
 */
function depsWant(ctx: Context, t: Toolchain, depsPatches: PatchInfo[]): { commit: string; depsPatches: { name: string; sha256: string }[]; inputs: string } {
  const patchesHash = sha256Text(JSON.stringify(depsPatches.map((p) => [p.name, p.sha256])))
  const mingw = t.platform === 'win32' ? { mingw: mingwPinsDigest() } : {}
  const inputs = sha256Text(JSON.stringify({ commit: ctx.lock.commit, patchesHash, configure: depsConfigure(t, ctx.work), cmake: t.cmake, compilerEnv: compilerEnv(t, ctx.work), ...mingw }))
  return { commit: ctx.lock.commit, depsPatches: depsPatches.map((p) => ({ name: p.name, sha256: p.sha256 })), inputs }
}

/**
 * Orca's dependencies, into <work>/deps. A build starts from an empty folder, so each dependency
 * is unpacked from its downloaded package and built as it is, unless it resumes a run of the same
 * inputs that stopped (state/deps-run.json): that keeps what was unpacked and built, and the build
 * it finishes is not fresh (stage refuses it; --fresh starts over). The finished build records what
 * it was built from and a digest of the prefix it installed (DepsRecord).
 */
async function buildDeps(ctx: Context, t: Toolchain, want: ReturnType<typeof depsWant>, fresh: boolean, log: Log): Promise<void> {
  const w = ctx.work
  const configure = depsConfigure(t, w)
  const cenv = compilerEnv(t, w)
  const { inputs } = want
  const stamp = readStamp(w.state, 'deps')
  if (!fresh) {
    const problems = depsStampProblems(stamp, { ...want, prefixDigest: () => hashTree(w.depsPrefix) })
    if (problems.length === 0) {
      log.line(`deps: already built for these inputs (${stamp!.finishedAt}); skipping`)
      return
    }
    if (stamp) log.line(`deps: building again: ${problems.join('; ')}`)
  }
  const run = readStamp(w.state, 'deps-run')
  const resume = !fresh && run?.inputs === inputs && existsSync(w.deps)
  // Until this build finishes there is no finished dependency build on record, and the engine
  // built on the dependencies as they were is no longer the engine of these.
  rmSync(join(w.state, 'deps.json'), { force: true })
  forgetEngineBuild(w)
  if (resume) {
    log.line(`deps: resuming the build of these inputs that stopped (started ${run!.finishedAt}); it will not count as fresh for stage (build --fresh starts over)`)
  } else if (existsSync(w.deps)) {
    log.line(`deps: starting ${w.deps} over`)
    rmSync(w.deps, { recursive: true, force: true })
  }
  writeStamp(w.state, 'deps-run', inputs)
  // Windows: the pinned MinGW-w64 toolchain that builds GMP and MPFR (patch 0007), unpacked into
  // the work folder or checked there.
  if (t.platform === 'win32') await ensureMingw(w, log)
  matchCompilerEnv(w, 'deps', w.deps, cenv, log)
  mkdirSync(w.downloads, { recursive: true })
  mkdirSync(w.deps, { recursive: true })
  // SKETCH_ORCA_JOBS caps the compile jobs here too (depsParallelEnv); configure sees it as well,
  // since Orca's deps/CMakeLists.txt reads CMAKE_BUILD_PARALLEL_LEVEL there.
  const env = { ...stepEnv(t, w), ...depsParallelEnv(t.platform) }
  log.line(`deps: configuring ${w.deps}`)
  await runLogged(t.cmake, configure, { env, log, cwd: w.deps })
  const jobs = defaultDepsJobs()
  const cap = jobsCap()
  log.line(`deps: building (${jobs} ${jobs === 1 ? 'dependency' : 'dependencies'} at a time${cap === undefined ? '' : `, at most ${cap} compile jobs`}); this takes a while`)
  await runLogged(t.cmake, depsBuildArgs(w.deps, jobs), { env, log, cwd: w.deps })
  const record: DepsRecord = { commit: want.commit, depsPatches: want.depsPatches, prefixDigest: hashTree(w.depsPrefix), fresh: !resume }
  writeStamp(w.state, 'deps', inputs, record)
  rmSync(join(w.state, 'deps-run.json'), { force: true })
  log.line(`deps: done${resume ? ' (resumed, so not fresh)' : ''}`)
}

/** Removes the record of a finished engine build (state/orca.json): stage and source take no engine without one. */
function forgetEngineBuild(w: WorkPaths): void {
  rmSync(join(w.state, 'orca.json'), { force: true })
}

async function buildOrca(ctx: Context, t: Toolchain, patches: PatchInfo[], patchesHash: string, deps: ReturnType<typeof depsWant>, log: Log): Promise<void> {
  const w = ctx.work
  // The engine is built only on a finished dependency build of the inputs there are now, whose
  // prefix is still what it installed (also with --step orca).
  const depsStamp = readStamp(w.state, 'deps')
  let depsPrefixDigest = ''
  const depsProblems = depsStampProblems(depsStamp, { ...deps, prefixDigest: () => (depsPrefixDigest = hashTree(w.depsPrefix)) })
  if (depsProblems.length > 0) throw new StepError(`${depsProblems.join('; ')}; run build, which builds the dependencies first`)
  const configure = orcaConfigureArgs({
    ninja: t.ninja,
    sourceDir: w.src,
    buildDir: w.build,
    depsPrefix: w.depsPrefix,
    cliDir: E.cli,
    commit: ctx.lock.commit,
    tag: ctx.lock.tag,
    ignorePrefixes: t.ignorePrefixes,
    platform: t.platform,
  })
  const cenv = compilerEnv(t, w)
  const inputs = sha256Text(JSON.stringify({ commit: ctx.lock.commit, patchesHash, configure, cmake: t.cmake, compilerEnv: cenv }))
  matchCompilerEnv(w, 'orca', w.build, cenv, log)
  const stamp = readStamp(w.state, 'orca-configure')
  mkdirSync(w.build, { recursive: true })
  const env = stepEnv(t, w)
  if (stamp?.inputs !== inputs || !existsSync(join(w.build, 'CMakeCache.txt'))) {
    log.line(`orca: configuring ${w.build}`)
    await runLogged(t.cmake, configure, { env, log, cwd: w.build })
    writeStamp(w.state, 'orca-configure', inputs)
  } else {
    log.line('orca: configuration is current')
  }
  const jobs = defaultJobs()
  log.line(`orca: building libslic3r and sketch-orca (-j${jobs})`)
  // What the engine is built from: cli/ and the checkout as they are now, and as they still are
  // when the build ends (an edit in between may or may not have been compiled). stage and source
  // compare them. Until the build ends there is no finished build on record.
  const cliSha256 = hashTree(E.cli)
  const source = sourceDigest(t.git, w.src, ctx.lock.commit)
  // Still the commit with patches/ applied and nothing else, as build left it before the
  // dependency build (which writes nothing into the checkout).
  if (source !== patchedDigest(t.git, w.src, ctx.lock.commit, patches)) throw new StepError(`${w.src} is not the locked commit with patches/ applied (a file changed, added or removed since build applied them); run build again`)
  forgetEngineBuild(w)
  // Ninja itself skips what is up to date.
  await runLogged(t.cmake, orcaBuildArgs(w.build, jobs), { env, log, cwd: w.build })
  const exe = engineExe(w, t.platform)
  if (!existsSync(exe)) throw new StepError(`build finished but ${exe} is missing`)
  if (hashTree(E.cli) !== cliSha256) throw new StepError('cli/ changed while the engine was being built; run build again')
  if (sourceDigest(t.git, w.src, ctx.lock.commit) !== source) throw new StepError(`${w.src} changed while the engine was being built; run build again`)
  if (hashTree(w.depsPrefix) !== depsPrefixDigest) throw new StepError(`${w.depsPrefix} changed while the engine was being built; run build again`)
  const { engineRuntime } = await import('./lib/smoke.ts')
  const record: BuildRecord = {
    commit: ctx.lock.commit,
    patches: patches.map((p) => ({ name: p.name, sha256: p.sha256 })),
    sourceDigest: source,
    exeSha256: sha256File(exe),
    runtime: engineRuntime(w, exe, t.platform),
    depsInputs: depsStamp!.inputs,
    depsPrefixDigest,
  }
  writeStamp(w.state, 'orca', inputs, { exe, cliSha256, compilerEnv: cenv, ...record })
  log.line(`orca: built ${exe}`)
}

export function engineExe(work: WorkPaths, platform = currentPlatform()): string {
  return join(work.build, 'sketch-orca', platform === 'win32' ? 'sketch-orca.exe' : 'sketch-orca')
}

async function cmdBuild(ctx: Context): Promise<number> {
  const log = logFor(ctx.work, 'build')
  const step = option(ctx.args, '--step')
  if (step !== undefined && step !== 'deps' && step !== 'orca') throw new UsageError(`--step must be deps or orca, got ${step}`)
  const fresh = flag(ctx.args, '--fresh')
  if (fresh && step === 'orca') throw new UsageError('--fresh starts the dependency build over; it does not go with --step orca')
  const t = findToolchain()
  for (const n of checkToolchain(t)) log.line(`toolchain: ${n}`)
  log.line(`toolchain: cmake ${t.cmake}, ninja ${t.ninja}, git ${t.git}${t.ignorePrefixes.length ? `, ignoring prefix ${t.ignorePrefixes.join(', ')}` : ''}`)
  if (headCommit(t.git, ctx.work.src) !== ctx.lock.commit) {
    log.line('build: the checkout is missing or not at the locked commit; fetching first')
    await cmdFetch(ctx, t)
  }
  const patches = await applyPatches(t.git, ctx.lock, ctx.work.src, E.patches, ctx.work.state, log)
  const patchesHash = sha256Text(JSON.stringify(patches.map((p) => [p.name, p.sha256])))
  // The dependency build depends only on the patches that change deps/.
  const deps = depsWant(ctx, t, depsPatchesOf(patches))
  if (step === undefined || step === 'deps') await buildDeps(ctx, t, deps, fresh, log)
  if (step === undefined || step === 'orca') await buildOrca(ctx, t, patches, patchesHash, deps, log)
  return 0
}

async function cmdSmoke(ctx: Context): Promise<number> {
  const { runSmoke } = await import('./lib/smoke.ts')
  return runSmoke(ctx, engineExe(ctx.work), logFor(ctx.work, 'smoke'))
}

async function cmdStage(ctx: Context): Promise<number> {
  const { runStage } = await import('./lib/stage.ts')
  return runStage(ctx, engineExe(ctx.work), logFor(ctx.work, 'stage'))
}

async function cmdSource(ctx: Context): Promise<number> {
  const { runSource } = await import('./lib/bundle.ts')
  return runSource(ctx, logFor(ctx.work, 'source'))
}

/**
 * check, update (only to a newer stable release), fetch, build, smoke, stage and source, in order,
 * stopping at the first failure. Each step skips what is already done and verified. --offline
 * leaves out check and update and releases the lock as it is.
 */
async function cmdRelease(ctx: Context): Promise<number> {
  const log = logFor(ctx.work, 'release')
  let current = ctx
  let decision: ReleaseDecision | undefined
  let latest: Awaited<ReturnType<typeof resolveLatestRelease>> | undefined
  const sub = (step: string): Context => ({ ...current, args: stepArgs(step, ctx.args) })
  const steps: ReleaseStep[] = []
  if (flag(ctx.args, '--offline')) {
    log.line(`release: --offline: no check against GitHub; releasing the lock as it is (${ctx.lock.tag})`)
  } else {
    steps.push(
      {
        name: 'check',
        run: async () => {
          ;({ latest, decision } = await latestDecision(current.lock))
          log.line(`check: ${decision.message}`)
          return decision.proceed ? EXIT.ok : EXIT.tagMoved
        },
      },
      {
        name: 'update',
        run: async () => {
          if (!decision?.update || !latest) {
            log.line(`update: not needed; the lock stays at ${current.lock.tag}`)
            return 0
          }
          const next = lockForRelease(current.lock, latest)
          writeLock(E.lock, next)
          current = { ...current, lock: readLock(E.lock) }
          log.line(`update: lock moved from ${ctx.lock.tag} (${ctx.lock.commit}) to ${next.tag} (${next.commit})`)
          return 0
        },
      },
    )
  }
  steps.push(
    { name: 'fetch', run: () => cmdFetch(sub('fetch')) },
    { name: 'build', run: () => cmdBuild(sub('build')) },
    { name: 'smoke', run: () => cmdSmoke(sub('smoke')) },
    { name: 'stage', run: () => cmdStage(sub('stage')) },
    { name: 'source', run: () => cmdSource(sub('source')) },
  )
  const outcome = await runSteps(steps, (msg) => log.line(msg))
  return outcome.code
}

const COMMANDS: Record<string, (ctx: Context) => Promise<number>> = {
  check: cmdCheck,
  update: cmdUpdate,
  fetch: (ctx) => cmdFetch(ctx),
  build: cmdBuild,
  smoke: cmdSmoke,
  stage: cmdStage,
  source: cmdSource,
  release: cmdRelease,
  env: cmdEnv,
}

/** The commands that write in the work folder: they hold <work>/run.lock.d while they run. */
export const LOCKED_COMMANDS = ['update', 'fetch', 'build', 'smoke', 'stage', 'source', 'release']

function usage(): string {
  return `usage: bun ${join('scripts', 'orca.ts')} <${Object.keys(COMMANDS).join('|')}> [options]`
}

/**
 * A command's exit code as the pipeline gives it (EXIT): 0, check's 1 and 2, and the tag-moved 2
 * of update and release pass through; any other code a command returns is a failure (3).
 */
export function exitCodeFor(command: string, code: number): number {
  if (code === EXIT.ok) return EXIT.ok
  if (command === 'check' && (code === EXIT.newer || code === EXIT.tagMoved)) return code
  if ((command === 'update' || command === 'release') && code === EXIT.tagMoved) return code
  return EXIT.failed
}

export async function main(argv: string[]): Promise<number> {
  const [command, ...args] = argv
  if (command === '--help' || command === '-h') {
    console.log(usage())
    return EXIT.ok
  }
  if (!command || !COMMANDS[command]) {
    console.error(usage())
    return EXIT.usage
  }
  const platform = currentPlatform()
  const work = workPaths(workHome(process.env, platform))
  const ctx: Context = { lock: readLock(E.lock), work, engine: E, args }
  let release: (() => void) | undefined
  try {
    if (LOCKED_COMMANDS.includes(command)) release = takeRunLock(work.home, command)
    return exitCodeFor(command, await COMMANDS[command]!(ctx))
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    try {
      logFor(work, command).line(`${command}: FAILED: ${msg}`)
    } catch {
      console.error(`${command}: FAILED: ${msg}`)
    }
    return err instanceof UsageError ? EXIT.usage : EXIT.failed
  } finally {
    release?.()
  }
}

if (import.meta.main) {
  main(process.argv.slice(2)).then((code) => process.exit(code))
}

export { listPatches }
