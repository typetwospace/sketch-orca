// The cargo runner of the Rust translation: which cargo command line a request becomes, which
// environment it runs in, and what it refuses. `bun scripts/rust.ts <build|test|clippy|fmt> [args]`
// (scripts/rust.ts) runs it; the rules are here so test/rust.test.ts can check them without running
// cargo.
//
// The program is built with Rust 1.94.1 and the static C runtime, in the environment of the same
// Visual Studio the C++ engine is built in (VS 2022 17.12, MSVC 14.42.34433, Windows SDK
// 10.0.22000.0), so link.exe takes the same libucrt.lib the C++ engine links. The runner refuses a
// machine, a Visual Studio or a rustc that is not that one, and an argument that would steer cargo
// somewhere else: another toolchain (`+nightly`), another target, a RUSTFLAGS of the caller's.
//
// Every cargo command it runs but fmt also remaps cargo's home folder to the relative `cargo`
// (rustc's --remap-path-prefix, given through cargo's --config as one more flag of
// target.x86_64-pc-windows-msvc.rustflags; cargo appends it to the +crt-static of
// .cargo/config.toml, as it joins arrays). A crate from cargo's registry records its source files'
// paths in the program (the panic locations of rayon-core, crossbeam-deque and crossbeam-epoch,
// which orca-tbb brings in). Without the remap those are C:\Users\<name>\.cargo\registry\src\...,
// which stage refuses to ship (stage.ts privacyProblemsIn); with it they are
// cargo\registry\src\.... The target is relative because stage also refuses an absolute POSIX
// path to a source file. Every command gets the same flags, so rs:test:release and stage build one
// program and cargo rebuilds nothing when one follows the other.
//
// The pipeline uses it too: `stage` builds the program it ships with buildRustRelease (cargo build
// --locked --release, target/release/sketch-orca-rs.exe), and `source` vendors the crates the
// program is built from with vendorRustCrates (cargo vendor --locked --versioned-dirs). The release
// build also writes a program database (target/release/sketch_orca_rs.pdb). stage does not ship
// it: the program database names the build folder, which is one reason stage never ships it. The
// program names it without its folder (its CodeView record is sketch_orca_rs.pdb).

import { spawn, spawnSync } from 'node:child_process'
import { appendFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { BUILT_WITH } from './bundle.ts'
import { cleanWinBuildPath, findOnPath, isLeakyEnv, parseSetOutput, splitPathList, vcvarsBaseEnv, winBaselinePath } from './env.ts'
import { listFiles } from './files.ts'
import { capture, StepError, UsageError } from './run.ts'
import { findVisualStudio } from './toolchain.ts'

/** The Rust program's name, and where `cargo build --release` puts it. */
export const RUST_PROGRAM = 'sketch-orca-rs'
export function rustReleaseExe(root: string): string {
  return join(root, 'target', 'release', `${RUST_PROGRAM}.exe`)
}

/** How long one cargo run of the pipeline may take: SKETCH_ORCA_RUST_TIMEOUT_MS, else 45 minutes. */
export function rustTimeoutMs(env: NodeJS.ProcessEnv = process.env): number {
  const v = env.SKETCH_ORCA_RUST_TIMEOUT_MS
  return v !== undefined && /^[1-9]\d*$/.test(v) ? Number(v) : 45 * 60 * 1000
}

/**
 * Runs cargo with the pinned toolchain (parseRustArgs's command line for `argv`), hidden, in
 * `root`, its output appended to `logPath`; stopped with the processes it started when the time
 * limit runs out. Throws a StepError when it fails.
 */
export async function runCargo(root: string, argv: string[], o: { logPath: string; timeoutMs?: number; env?: NodeJS.ProcessEnv }): Promise<void> {
  const inv = parseRustArgs(argv, cargoHomeDir(o.env ?? process.env))
  const tool = findRustToolchain(root, o.env ?? process.env)
  mkdirSync(dirname(o.logPath), { recursive: true })
  appendFileSync(o.logPath, `$ cargo ${inv.cargoArgs.join(' ')}\n${tool.notes.join('; ')}\n`)
  const limit = o.timeoutMs ?? rustTimeoutMs(o.env ?? process.env)
  const code = await new Promise<number | null>((resolve, reject) => {
    const child = spawn(tool.cargo, inv.cargoArgs, { cwd: root, env: tool.env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
    child.stdout.on('data', (d: Buffer) => appendFileSync(o.logPath, d))
    child.stderr.on('data', (d: Buffer) => appendFileSync(o.logPath, d))
    let timedOut = false
    const timer = setTimeout(() => {
      timedOut = true
      if (child.pid !== undefined) spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' })
    }, limit)
    child.on('error', (e) => {
      clearTimeout(timer)
      reject(e)
    })
    child.on('close', (c) => {
      clearTimeout(timer)
      if (timedOut) reject(new StepError(`cargo ${inv.command} was stopped after ${Math.round(limit / 60000)} minutes (log: ${o.logPath})`))
      else resolve(c)
    })
  })
  if (code !== 0) throw new StepError(`cargo ${inv.cargoArgs.join(' ')} exited with ${code} (log: ${o.logPath})`)
}

/**
 * The source of every crate Cargo.lock names, in `dir` (cargo vendor --locked --versioned-dirs:
 * <dir>/<name>-<version>/), with the licence files each crate's package carries. Returns the
 * crates' folders, sorted. Throws when a crate has no licence file or holds a library a linker
 * would take (windows-sys 0.61 links through windows-link's raw-dylib, so none is expected).
 */
export async function vendorRustCrates(root: string, dir: string, logPath: string): Promise<string[]> {
  await runCargo(root, ['vendor', '--versioned-dirs', dir], { logPath })
  const crates = existsSync(dir) ? readdirSync(dir, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name).sort() : []
  if (crates.length === 0) throw new StepError(`cargo vendor put no crates in ${dir} (log: ${logPath})`)
  const problems = vendoredProblems(crates.map((c) => ({ name: c, files: listFiles(join(dir, c)) })))
  if (problems.length > 0) throw new StepError(`the vendored crates are not complete: ${problems.join('; ')}`)
  return crates
}

/** Why vendored crates (each with its files, relative) cannot ship: no licence file, or a prebuilt library. */
export function vendoredProblems(crates: { name: string; files: string[] }[]): string[] {
  const out: string[] = []
  for (const c of crates) {
    if (!c.files.some((f) => !f.includes('/') && /^(licen[cs]e|copying|copyright|unlicense)/i.test(f))) out.push(`${c.name} has no licence file`)
    const libs = c.files.filter((f) => /\.(lib|a|dll|so|dylib)$/i.test(f))
    if (libs.length > 0) out.push(`${c.name} holds prebuilt libraries (${libs.slice(0, 3).join(', ')})`)
  }
  return out
}

/** Builds the Rust program in release with --locked, and returns it (target/release/sketch-orca-rs.exe). */
export async function buildRustRelease(root: string, logPath: string): Promise<string> {
  await runCargo(root, ['build', '--release', '--bin', RUST_PROGRAM], { logPath })
  const exe = rustReleaseExe(root)
  if (!existsSync(exe)) throw new StepError(`cargo build --release made no ${exe} (log: ${logPath})`)
  return exe
}

/** What the Rust program is built with. rustc, Visual Studio, MSVC and the SDK are BUILT_WITH's. */
export const RUST_PINS = {
  rustc: BUILT_WITH.rust,
  host: 'x86_64-pc-windows-msvc',
  /** Visual Studio's major.minor, from BUILT_WITH.visualStudio. */
  visualStudio: BUILT_WITH.visualStudio.replace(/^.*\b(\d+\.\d+)$/, '$1'),
  msvc: BUILT_WITH.msvc,
  windowsSdk: BUILT_WITH.windowsSdk,
} as const

/** The commands of the runner, and the cargo subcommand each one runs. */
export const RUST_COMMANDS = { build: 'build', test: 'test', clippy: 'clippy', fmt: 'fmt', vendor: 'vendor' } as const
export type RustCommand = keyof typeof RUST_COMMANDS

export interface RustInvocation {
  command: RustCommand
  /** The arguments to cargo, subcommand first. */
  cargoArgs: string[]
}

/** Cargo's selection of what to build: with one of them clippy does not add --all-targets. */
const TARGET_SELECTORS = ['--lib', '--bins', '--bin', '--examples', '--example', '--tests', '--test', '--benches', '--bench', '--all-targets']

/** Lint flags: clippy takes them after the `--`, as -D warnings, -Dwarnings, -W clippy::all, -A and so on. */
const LINT_FLAG = /^-[DWA]/

/** libtest's own options, which `cargo test` hands to the test program after the `--`. */
const LIBTEST_FLAGS = ['--nocapture', '--show-output', '--test-threads', '--ignored', '--include-ignored', '--exact', '--skip', '--list', '--format', '--report-time', '--shuffle', '--shuffle-seed', '--logfile']

function isLibtestFlag(arg: string): boolean {
  const name = arg.split('=')[0]!
  return LIBTEST_FLAGS.includes(name)
}

/**
 * The cargo command line for a request, `argv` being what follows the script's name:
 * `<build|test|clippy|fmt|vendor> [cargo arguments] [-- arguments for the tool]`.
 *
 * - Every cargo run gets `--locked`, so the lock file is never changed behind anyone's back
 *   (fmt is the one exception: `cargo fmt` takes no --locked and resolves no dependencies).
 * - Bun can hand a script its arguments without the `--` that separated them (`bun run rs:clippy
 *   -- -D warnings`), so the runner puts it back: clippy's lint flags (-D, -W, -A) and test's
 *   libtest options (--nocapture, --test-threads and the rest) and everything after the first of
 *   them go after one `--`. A `--` the caller wrote stays where it is.
 * - fmt formats the whole workspace (--all); clippy checks every target unless one is selected.
 * - Every command but fmt gets `--config` cargoHomeRemapConfig(cargoHome) right after its
 *   subcommand, which remaps cargo's home folder out of what it builds. fmt compiles nothing,
 *   and cargo-fmt would not take the flag. clippy hands it on to the cargo check it runs.
 *
 * Throws a UsageError for no command, an unknown command, a cargo toolchain override (`+name`), a
 * `--target` other than the host's, and `--manifest-path` (the workspace is the one in the
 * repository's root).
 */
export function parseRustArgs(argv: string[], cargoHome: string = cargoHomeDir()): RustInvocation {
  const [first, ...rest] = argv
  if (first === undefined) throw new UsageError(`rust.ts needs a command: ${Object.keys(RUST_COMMANDS).join(', ')}`)
  if (!Object.hasOwn(RUST_COMMANDS, first)) throw new UsageError(`unknown rust command ${first} (${Object.keys(RUST_COMMANDS).join(', ')})`)
  const command = first as RustCommand

  let own = rest
  let tool: string[] = []
  const dashes = rest.indexOf('--')
  if (dashes >= 0) {
    own = rest.slice(0, dashes)
    tool = rest.slice(dashes + 1)
  } else if (command === 'clippy' || command === 'test') {
    const at = own.findIndex((a) => (command === 'clippy' ? LINT_FLAG.test(a) : isLibtestFlag(a)))
    if (at >= 0) {
      tool = own.slice(at)
      own = own.slice(0, at)
    }
  }

  for (let i = 0; i < own.length; i++) {
    const a = own[i]!
    if (a.startsWith('+')) throw new UsageError(`${a}: the toolchain is fixed by rust-toolchain.toml (${RUST_PINS.rustc})`)
    if (a === '--manifest-path' || a.startsWith('--manifest-path=')) throw new UsageError('--manifest-path: the workspace is the one in the repository root')
    const target = a === '--target' ? own[i + 1] : a.startsWith('--target=') ? a.slice('--target='.length) : undefined
    if ((a === '--target' || a.startsWith('--target=')) && target !== RUST_PINS.host) {
      throw new UsageError(`--target ${target ?? ''}: only ${RUST_PINS.host} is built`)
    }
  }

  const args: string[] = [RUST_COMMANDS[command]]
  if (command !== 'fmt') args.push('--config', cargoHomeRemapConfig(cargoHome))
  if (command !== 'fmt' && !own.some((a) => a === '--locked' || a === '--frozen')) args.push('--locked')
  if (command === 'fmt' && !own.includes('--all') && !own.includes('-p') && !own.includes('--package')) args.push('--all')
  if (command === 'clippy' && !own.some((a) => TARGET_SELECTORS.includes(a))) args.push('--all-targets')
  args.push(...own)
  if (tool.length > 0 || dashes >= 0) args.push('--', ...tool)
  return { command, cargoArgs: args }
}

/** Environment variables that would steer cargo or rustc away from the pinned build. */
const RUST_STEERING_ENV = [
  'RUSTFLAGS', 'RUSTDOCFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_ENCODED_RUSTDOCFLAGS',
  'RUSTC', 'RUSTDOC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTC_BOOTSTRAP',
  'RUSTUP_TOOLCHAIN', 'RUSTUP_OVERRIDE', 'CARGO_BUILD_TARGET', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_BUILD_RUSTC',
  'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_TARGET_DIR', 'CARGO_TARGET_DIR', 'CARGO_BUILD_RUSTDOCFLAGS',
  'CARGO_PROFILE_RELEASE_LTO', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_CODEGEN_UNITS',
]

/** Whether a variable of the caller's is dropped from the Rust build's environment. */
export function isRustSteeringEnv(name: string): boolean {
  const key = name.toUpperCase()
  if (RUST_STEERING_ENV.includes(key)) return true
  // CARGO_TARGET_<triple>_RUSTFLAGS, _LINKER and _RUNNER; CARGO_PROFILE_*; CARGO_BUILD_* that name a flag.
  if (/^CARGO_TARGET_.*_(RUSTFLAGS|LINKER|RUNNER|RUSTDOCFLAGS)$/.test(key)) return true
  if (/^CARGO_PROFILE_/.test(key)) return true
  if (/^CARGO_(CFG|ENCODED)_/.test(key)) return true
  return false
}

/**
 * The environment of a cargo run: the one vcvars64.bat made, without the variables that steer
 * compilers (env.ts, LEAKY_ENV), without those that steer cargo (RUST_STEERING_ENV), and with a
 * PATH of the vcvars entries, the system folders and cargo's own folder. `cargoBin` is where cargo
 * and the rustup proxies are.
 */
export function rustBuildEnv(vc: Record<string, string>, cargoBin: string): Record<string, string> {
  const out: Record<string, string> = {}
  let path: string[] = []
  for (const [k, v] of Object.entries(vc)) {
    if (k.toUpperCase() === 'PATH') {
      path = splitPathList(v, 'win32')
      continue
    }
    if (isLeakyEnv(k, 'win32') || isRustSteeringEnv(k)) continue
    out[k] = v
  }
  out.PATH = cleanWinBuildPath(path, [cargoBin]).join(';')
  return out
}

/**
 * Why a Visual Studio and its environment are not the pinned ones; an empty list when they are.
 * `vsVersion` is the install's version as vswhere gives it (17.12.35707.178); vcvars64.bat's own
 * VSCMD_VER says only 17.0.
 */
export function pinnedEnvProblems(vc: Record<string, string>, vsVersion: string | undefined): string[] {
  const get = (name: string) => Object.entries(vc).find(([k]) => k.toUpperCase() === name.toUpperCase())?.[1]
  const problems: string[] = []
  if (vsVersion === undefined || !(vsVersion === RUST_PINS.visualStudio || vsVersion.startsWith(`${RUST_PINS.visualStudio}.`))) {
    problems.push(`Visual Studio is ${vsVersion || '(unknown)'}, expected ${RUST_PINS.visualStudio}`)
  }
  const msvc = get('VCToolsVersion')
  if (msvc !== RUST_PINS.msvc) problems.push(`MSVC is ${msvc ?? '(unknown)'}, expected ${RUST_PINS.msvc}`)
  const sdk = get('WindowsSDKVersion')?.replace(/[\\/]+$/, '')
  if (sdk !== RUST_PINS.windowsSdk) problems.push(`the Windows SDK is ${sdk ?? '(unknown)'}, expected ${RUST_PINS.windowsSdk}`)
  const arch = get('VSCMD_ARG_TGT_ARCH')
  if (arch !== 'x64') problems.push(`the target architecture is ${arch ?? '(unknown)'}, expected x64`)
  return problems
}

/** Why the output of `rustc -vV` is not the pinned compiler on the pinned host; an empty list when it is. */
export function rustcProblems(vv: string): string[] {
  const version = /^release: (\S+)$/m.exec(vv)?.[1]
  const host = /^host: (\S+)$/m.exec(vv)?.[1]
  const problems: string[] = []
  if (version !== RUST_PINS.rustc) problems.push(`rustc is ${version ?? '(unknown)'}, expected ${RUST_PINS.rustc}`)
  if (host !== RUST_PINS.host) problems.push(`rustc's host is ${host ?? '(unknown)'}, expected ${RUST_PINS.host}`)
  return problems
}

/** Cargo's home folder, as cargo finds it: CARGO_HOME, or .cargo in the home folder; absolute. */
export function cargoHomeDir(env: NodeJS.ProcessEnv = process.env, home: string = homedir()): string {
  return resolve(env.CARGO_HOME && env.CARGO_HOME !== '' ? env.CARGO_HOME : join(home, '.cargo'))
}

/** Cargo's folder: CARGO_HOME\bin, or .cargo\bin in the home folder. */
export function cargoBinDir(env: NodeJS.ProcessEnv = process.env, home: string = homedir()): string {
  return join(env.CARGO_HOME && env.CARGO_HOME !== '' ? env.CARGO_HOME : join(home, '.cargo'), 'bin')
}

/** What cargo's home folder becomes in the paths the program records: relative, never `/cargo`. */
export const CARGO_HOME_REMAP_TO = 'cargo'

/**
 * The `--config` value that remaps `cargoHome` to CARGO_HOME_REMAP_TO: rustc's
 * --remap-path-prefix as one more flag of the host target's rustflags, the table .cargo/config.toml
 * gives +crt-static in. Cargo joins an array given with --config to the config files' (theirs
 * first), so +crt-static stays; build.rustflags would not do, as cargo ignores it when a
 * target's rustflags are set. The value is a TOML basic string (JSON's escapes of `\` and `"` are
 * TOML's), and rustc splits the flag at its last `=`, so a folder with `=` in it stays whole.
 */
export function cargoHomeRemapConfig(cargoHome: string): string {
  return `target.${RUST_PINS.host}.rustflags=[${JSON.stringify(`--remap-path-prefix=${cargoHome}=${CARGO_HOME_REMAP_TO}`)}]`
}

/**
 * Runs vcvars64.bat for the pinned MSVC and Windows SDK (`vcvars64.bat <sdk> -vcvars_ver=<msvc>`)
 * in a cmd.exe with a minimal PATH and returns the environment it made. Throws a StepError when
 * the batch file fails, which it does when either version is not installed.
 */
export function pinnedVcvarsEnvironment(vcvars: string, env: NodeJS.ProcessEnv): Record<string, string> {
  const systemRoot = env.SystemRoot ?? env.SYSTEMROOT ?? 'C:\\Windows'
  const base = vcvarsBaseEnv(env, winBaselinePath(systemRoot))
  const cmdExe = join(systemRoot, 'System32', 'cmd.exe')
  const r = capture(cmdExe, ['/u', '/d', '/s', '/c', `"call "${vcvars}" ${RUST_PINS.windowsSdk} -vcvars_ver=${RUST_PINS.msvc} >nul 2>&1 && set"`], {
    env: base,
    verbatim: true,
    encoding: 'utf16le',
  })
  if (r.code !== 0) {
    throw new StepError(`vcvars64.bat for MSVC ${RUST_PINS.msvc} and Windows SDK ${RUST_PINS.windowsSdk} failed (${r.code}): is that Visual Studio component installed?`)
  }
  const out = parseSetOutput(r.stdout)
  if (!Object.keys(out).some((k) => k.toUpperCase() === 'VCTOOLSINSTALLDIR')) throw new StepError('vcvars64.bat did not set up the MSVC environment')
  return out
}

/** The version of the Visual Studio install at `vsInstall`, by vswhere (17.12.35707.178); undefined when vswhere does not say. */
export function visualStudioVersion(vsInstall: string, env: NodeJS.ProcessEnv): string | undefined {
  const pf86 = env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)'
  const vswhere = join(pf86, 'Microsoft Visual Studio', 'Installer', 'vswhere.exe')
  if (!existsSync(vswhere)) return undefined
  const r = capture(vswhere, ['-path', vsInstall, '-property', 'installationVersion'])
  const v = r.stdout.trim().split(/\r?\n/)[0]?.trim()
  return v ? v : undefined
}

export interface RustToolchain {
  cargo: string
  env: Record<string, string>
  /** The notes `rust.ts` prints: the compiler, MSVC and the SDK in use. */
  notes: string[]
}

/**
 * Finds the pinned toolchain: Visual Studio and its environment, cargo, and rustc as
 * rust-toolchain.toml in `root` selects it. Throws a StepError (a refusal) naming everything that
 * is not the pinned one.
 */
export function findRustToolchain(root: string, env: NodeJS.ProcessEnv = process.env): RustToolchain {
  if (process.platform !== 'win32') throw new StepError(`the Rust program is built on Windows x64 (MSVC ${RUST_PINS.msvc}); this is ${process.platform}`)
  const vsInstall = findVisualStudio(env)
  const vcvars = vsInstall ? join(vsInstall, 'VC', 'Auxiliary', 'Build', 'vcvars64.bat') : undefined
  if (!vcvars || !existsSync(vcvars)) throw new StepError('Visual Studio 2022 with the MSVC x64 tools (vcvars64.bat) is missing')
  const vc = pinnedVcvarsEnvironment(vcvars, env)
  const wrong = pinnedEnvProblems(vc, visualStudioVersion(vsInstall!, env))
  if (wrong.length > 0) throw new StepError(`not the pinned toolchain: ${wrong.join('; ')}`)

  const cargoBin = cargoBinDir(env)
  const buildEnv = rustBuildEnv(vc, cargoBin)
  const dirs = splitPathList(buildEnv.PATH, 'win32')
  const cargo = findOnPath(['cargo.exe'], dirs)
  if (!cargo) throw new StepError(`cargo is not in ${cargoBin}: install rustup (BUILDING.md)`)

  // rustc as the toolchain file selects it: run in the repository's root, with the steering
  // variables gone, so a RUSTUP_TOOLCHAIN of the caller's does not choose another one.
  const rustc = findOnPath(['rustc.exe'], dirs)
  if (!rustc) throw new StepError(`rustc is not in ${cargoBin}: install rustup (BUILDING.md)`)
  const r = capture(rustc, ['-vV'], { cwd: root, env: buildEnv })
  if (r.code !== 0) throw new StepError(`rustc -vV failed (${r.code}): ${(r.stderr || r.stdout).trim()}`)
  const bad = rustcProblems(r.stdout)
  if (bad.length > 0) throw new StepError(`not the pinned toolchain: ${bad.join('; ')}`)

  return {
    cargo,
    env: buildEnv,
    notes: [`rustc ${RUST_PINS.rustc} (${RUST_PINS.host})`, `MSVC ${RUST_PINS.msvc}, Windows SDK ${RUST_PINS.windowsSdk}, Visual Studio ${RUST_PINS.visualStudio}`],
  }
}
