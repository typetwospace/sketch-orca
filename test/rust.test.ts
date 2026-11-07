// The cargo runner (scripts/lib/rust.ts, scripts/rust.ts): the cargo command lines it makes, the
// environment it runs in and what it refuses. None of this runs cargo.

import { describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { BUILT_WITH } from '../scripts/lib/bundle.ts'
import { EXIT, UsageError } from '../scripts/lib/run.ts'
import { cargoBinDir, isRustSteeringEnv, parseRustArgs, pinnedEnvProblems, RUST_PINS, RUST_PROGRAM, rustBuildEnv, rustcProblems, rustReleaseExe, rustTimeoutMs, vendoredProblems } from '../scripts/lib/rust.ts'

const ROOT = join(import.meta.dir, '..')

const GOOD_VC: Record<string, string> = {
  VCToolsVersion: RUST_PINS.msvc,
  WindowsSDKVersion: `${RUST_PINS.windowsSdk}\\`,
  VSCMD_ARG_TGT_ARCH: 'x64',
}

describe('the pins', () => {
  test('are the toolchain the C++ engine is built with, and the Rust of rust-toolchain.toml', () => {
    expect(RUST_PINS.msvc).toBe(BUILT_WITH.msvc)
    expect(RUST_PINS.windowsSdk).toBe(BUILT_WITH.windowsSdk)
    expect(RUST_PINS.visualStudio).toBe('17.12')
    expect(RUST_PINS.msvc).toBe('14.42.34433')
    expect(RUST_PINS.windowsSdk).toBe('10.0.22000.0')
    expect(readFileSync(join(ROOT, 'rust-toolchain.toml'), 'utf8')).toContain(`channel = "${RUST_PINS.rustc}"`)
    expect(readFileSync(join(ROOT, 'Cargo.toml'), 'utf8')).toContain('rust-version = "1.94"')
    expect(RUST_PINS.rustc).toBe(BUILT_WITH.rust)
    expect(BUILT_WITH.rust).toBe('1.94.1')
  })
})

describe('what the pipeline runs', () => {
  test('stage builds the program in release with --locked; source vendors the crates with --locked', () => {
    expect(parseRustArgs(['build', '--release', '--bin', RUST_PROGRAM]).cargoArgs).toEqual(['build', '--locked', '--release', '--bin', 'sketch-orca-rs'])
    expect(parseRustArgs(['vendor', '--versioned-dirs', 'v']).cargoArgs).toEqual(['vendor', '--locked', '--versioned-dirs', 'v'])
    expect(rustReleaseExe(join('r'))).toBe(join('r', 'target', 'release', 'sketch-orca-rs.exe'))
    expect(rustTimeoutMs({})).toBe(45 * 60 * 1000)
    expect(rustTimeoutMs({ SKETCH_ORCA_RUST_TIMEOUT_MS: '1000' })).toBe(1000)
  })

  test('a vendored crate ships with a licence file and no prebuilt library', () => {
    expect(vendoredProblems([{ name: 'rayon-1.12.0', files: ['Cargo.toml', 'LICENSE-APACHE', 'LICENSE-MIT', 'src/lib.rs'] }])).toEqual([])
    expect(vendoredProblems([{ name: 'windows-sys-0.61.2', files: ['license-mit', 'src/lib.rs'] }])).toEqual([])
    expect(vendoredProblems([{ name: 'x-1.0.0', files: ['Cargo.toml', 'src/LICENSE'] }])).toEqual(['x-1.0.0 has no licence file'])
    expect(vendoredProblems([{ name: 'y-1.0.0', files: ['COPYING', 'lib/y.lib'] }])).toEqual(['y-1.0.0 holds prebuilt libraries (lib/y.lib)'])
  })
})

describe('parseRustArgs', () => {
  test('build and test run with --locked', () => {
    expect(parseRustArgs(['build'])).toEqual({ command: 'build', cargoArgs: ['build', '--locked'] })
    expect(parseRustArgs(['test'])).toEqual({ command: 'test', cargoArgs: ['test', '--locked'] })
    expect(parseRustArgs(['build', '--release', '-p', 'orca-cli']).cargoArgs).toEqual(['build', '--locked', '--release', '-p', 'orca-cli'])
  })

  test('--locked is not given twice, and --frozen counts as it', () => {
    expect(parseRustArgs(['build', '--locked']).cargoArgs).toEqual(['build', '--locked'])
    expect(parseRustArgs(['test', '--frozen']).cargoArgs).toEqual(['test', '--frozen'])
  })

  test('fmt formats the whole workspace and takes no --locked (cargo fmt has none)', () => {
    expect(parseRustArgs(['fmt']).cargoArgs).toEqual(['fmt', '--all'])
    expect(parseRustArgs(['fmt', '--check']).cargoArgs).toEqual(['fmt', '--all', '--check'])
    expect(parseRustArgs(['fmt', '--all', '--check']).cargoArgs).toEqual(['fmt', '--all', '--check'])
    expect(parseRustArgs(['fmt', '-p', 'orca-json']).cargoArgs).toEqual(['fmt', '-p', 'orca-json'])
    expect(parseRustArgs(['fmt']).cargoArgs).not.toContain('--locked')
  })

  test('clippy checks every target unless one is selected', () => {
    expect(parseRustArgs(['clippy']).cargoArgs).toEqual(['clippy', '--locked', '--all-targets'])
    expect(parseRustArgs(['clippy', '--lib']).cargoArgs).toEqual(['clippy', '--locked', '--lib'])
    expect(parseRustArgs(['clippy', '--all-targets']).cargoArgs).toEqual(['clippy', '--locked', '--all-targets'])
  })

  test("puts back the -- that bun swallows before clippy's lint flags", () => {
    // bun run rs:clippy -- -D warnings reaches the script as: clippy -D warnings
    expect(parseRustArgs(['clippy', '-D', 'warnings']).cargoArgs).toEqual(['clippy', '--locked', '--all-targets', '--', '-D', 'warnings'])
    expect(parseRustArgs(['clippy', '-Dwarnings']).cargoArgs).toEqual(['clippy', '--locked', '--all-targets', '--', '-Dwarnings'])
    expect(parseRustArgs(['clippy', '-p', 'orca-cli', '-W', 'clippy::pedantic', '-A', 'clippy::module_name_repetitions']).cargoArgs).toEqual([
      'clippy', '--locked', '--all-targets', '-p', 'orca-cli', '--', '-W', 'clippy::pedantic', '-A', 'clippy::module_name_repetitions',
    ])
  })

  test("puts back the -- before libtest's options and keeps cargo's own before it", () => {
    expect(parseRustArgs(['test', '--nocapture']).cargoArgs).toEqual(['test', '--locked', '--', '--nocapture'])
    expect(parseRustArgs(['test', '-p', 'orca-cli', 'usage', '--test-threads=1', '--nocapture']).cargoArgs).toEqual([
      'test', '--locked', '-p', 'orca-cli', 'usage', '--', '--test-threads=1', '--nocapture',
    ])
    // cargo's -F is --features, not a lint flag, and --color is cargo's: they stay before the --.
    expect(parseRustArgs(['clippy', '-F', 'x', '--color', 'never']).cargoArgs).toEqual(['clippy', '--locked', '--all-targets', '-F', 'x', '--color', 'never'])
    expect(parseRustArgs(['test', '--color', 'never']).cargoArgs).toEqual(['test', '--locked', '--color', 'never'])
  })

  test('a -- the caller wrote stays where it is, and is not doubled', () => {
    expect(parseRustArgs(['clippy', '--', '-D', 'warnings']).cargoArgs).toEqual(['clippy', '--locked', '--all-targets', '--', '-D', 'warnings'])
    expect(parseRustArgs(['test', '--', '--nocapture']).cargoArgs).toEqual(['test', '--locked', '--', '--nocapture'])
    expect(parseRustArgs(['test', '-p', 'orca-cli', '--', '-D']).cargoArgs).toEqual(['test', '--locked', '-p', 'orca-cli', '--', '-D'])
    expect(parseRustArgs(['build', '--', 'x']).cargoArgs.filter((a) => a === '--').length).toBe(1)
  })

  test('build does not move anything after a --: -D is not its flag', () => {
    expect(parseRustArgs(['build', '-D', 'warnings']).cargoArgs).toEqual(['build', '--locked', '-D', 'warnings'])
  })

  test('refuses no command, an unknown command and a command that names a property of Object', () => {
    expect(() => parseRustArgs([])).toThrow(UsageError)
    expect(() => parseRustArgs(['deploy'])).toThrow('unknown rust command deploy')
    expect(() => parseRustArgs(['toString'])).toThrow(UsageError)
    expect(() => parseRustArgs(['--locked'])).toThrow(UsageError)
  })

  test('refuses another toolchain, another target and another manifest', () => {
    expect(() => parseRustArgs(['build', '+nightly'])).toThrow('rust-toolchain.toml')
    expect(() => parseRustArgs(['test', '+1.80.0'])).toThrow(UsageError)
    expect(() => parseRustArgs(['build', '--target', 'x86_64-pc-windows-gnu'])).toThrow('only x86_64-pc-windows-msvc')
    expect(() => parseRustArgs(['build', '--target=aarch64-pc-windows-msvc'])).toThrow(UsageError)
    expect(() => parseRustArgs(['build', '--target'])).toThrow(UsageError)
    expect(() => parseRustArgs(['build', '--manifest-path', 'x/Cargo.toml'])).toThrow('--manifest-path')
    expect(() => parseRustArgs(['build', '--manifest-path=x/Cargo.toml'])).toThrow(UsageError)
    expect(parseRustArgs(['build', '--target', 'x86_64-pc-windows-msvc']).cargoArgs).toEqual(['build', '--locked', '--target', 'x86_64-pc-windows-msvc'])
  })

  test('a + after the -- belongs to the tool, not to cargo', () => {
    expect(parseRustArgs(['test', '--', '+x']).cargoArgs).toEqual(['test', '--locked', '--', '+x'])
  })
})

describe('the pinned environment', () => {
  test('a Visual Studio with the pinned MSVC and SDK has no problems', () => {
    expect(pinnedEnvProblems(GOOD_VC, '17.12.35707.178')).toEqual([])
    expect(pinnedEnvProblems({ ...GOOD_VC, WindowsSDKVersion: RUST_PINS.windowsSdk }, '17.12.0')).toEqual([])
    expect(pinnedEnvProblems({ vctoolsversion: RUST_PINS.msvc, windowssdkversion: `${RUST_PINS.windowsSdk}\\`, vscmd_arg_tgt_arch: 'x64' }, '17.12')).toEqual([])
  })

  test('refuses another Visual Studio, MSVC, SDK or architecture, each by name', () => {
    expect(pinnedEnvProblems(GOOD_VC, '17.13.35818.85')).toEqual(['Visual Studio is 17.13.35818.85, expected 17.12'])
    expect(pinnedEnvProblems(GOOD_VC, '17.1')).toHaveLength(1)
    expect(pinnedEnvProblems(GOOD_VC, undefined)).toEqual(['Visual Studio is (unknown), expected 17.12'])
    expect(pinnedEnvProblems({ ...GOOD_VC, VCToolsVersion: '14.43.34808' }, '17.12.1')).toEqual(['MSVC is 14.43.34808, expected 14.42.34433'])
    expect(pinnedEnvProblems({ ...GOOD_VC, WindowsSDKVersion: '10.0.26100.0\\' }, '17.12.1')).toEqual(['the Windows SDK is 10.0.26100.0, expected 10.0.22000.0'])
    expect(pinnedEnvProblems({ ...GOOD_VC, VSCMD_ARG_TGT_ARCH: 'arm64' }, '17.12.1')).toEqual(['the target architecture is arm64, expected x64'])
    expect(pinnedEnvProblems({}, '17.12.1')).toHaveLength(3)
  })

  test('rustc must be 1.94.1 on x86_64-pc-windows-msvc', () => {
    const vv = (release: string, host: string) => `rustc ${release} (29ea6fb6a 2026-03-24)\nbinary: rustc\nhost: ${host}\nrelease: ${release}\nLLVM version: 21.1.1\n`
    expect(rustcProblems(vv('1.94.1', 'x86_64-pc-windows-msvc'))).toEqual([])
    expect(rustcProblems(vv('1.95.0', 'x86_64-pc-windows-msvc'))).toEqual(['rustc is 1.95.0, expected 1.94.1'])
    expect(rustcProblems(vv('1.94.1', 'x86_64-pc-windows-gnu'))).toEqual(["rustc's host is x86_64-pc-windows-gnu, expected x86_64-pc-windows-msvc"])
    expect(rustcProblems('')).toHaveLength(2)
  })

  test('variables that steer rustc, cargo or the linker are dropped; the rest stay', () => {
    for (const n of ['RUSTFLAGS', 'rustflags', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTUP_TOOLCHAIN', 'CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER', 'CARGO_PROFILE_RELEASE_LTO', 'RUSTC_BOOTSTRAP'])
      expect({ n, dropped: isRustSteeringEnv(n) }).toEqual({ n, dropped: true })
    for (const n of ['CARGO_HOME', 'RUSTUP_HOME', 'CARGO_TERM_COLOR', 'CARGO_HTTP_TIMEOUT', 'USERPROFILE', 'TEMP'])
      expect({ n, dropped: isRustSteeringEnv(n) }).toEqual({ n, dropped: false })
  })

  test('rustBuildEnv: the vcvars variables stay, the steering and compiler variables go, cargo is on PATH without MSYS', () => {
    const vc = {
      Path: 'C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.42.34433\\bin\\HostX64\\x64;C:\\Windows\\System32;C:\\Program Files\\Git\\usr\\bin;C:\\Windows\\System32',
      LIB: 'C:\\lib',
      INCLUDE: 'C:\\inc',
      RUSTFLAGS: '-C target-cpu=native',
      CL: '/O2',
      LINK: '/DEBUG',
      CARGO_HOME: 'C:\\cargo-home',
      CMAKE_PREFIX_PATH: 'C:\\x',
      BOOST_ROOT: 'C:\\boost',
    }
    const env = rustBuildEnv(vc, 'C:\\cargo-home\\bin')
    expect(env.LIB).toBe('C:\\lib')
    expect(env.INCLUDE).toBe('C:\\inc')
    expect(env.CARGO_HOME).toBe('C:\\cargo-home')
    for (const gone of ['RUSTFLAGS', 'CL', 'LINK', 'CMAKE_PREFIX_PATH', 'BOOST_ROOT', 'Path']) expect(Object.keys(env)).not.toContain(gone)
    expect(env.PATH.split(';')).toEqual([
      'C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.42.34433\\bin\\HostX64\\x64',
      'C:\\Windows\\System32',
      'C:\\cargo-home\\bin',
    ])
  })

  test("cargo's folder is CARGO_HOME\\bin, else .cargo\\bin in the home folder", () => {
    expect(cargoBinDir({ CARGO_HOME: 'D:\\cargo' }, 'C:\\h')).toBe(join('D:\\cargo', 'bin'))
    expect(cargoBinDir({}, 'C:\\h')).toBe(join('C:\\h', '.cargo', 'bin'))
    expect(cargoBinDir({ CARGO_HOME: '' }, 'C:\\h')).toBe(join('C:\\h', '.cargo', 'bin'))
  })
})

describe('scripts/rust.ts', () => {
  const run = (...args: string[]) => spawnSync(process.execPath, [join(ROOT, 'scripts', 'rust.ts'), ...args], { encoding: 'utf8', windowsHide: true, timeout: 60_000 })

  test('refuses a command line it does not take with exit 64, before it looks for any tool', () => {
    for (const args of [[], ['deploy'], ['build', '+nightly'], ['build', '--target', 'x86_64-pc-windows-gnu'], ['test', '--manifest-path', 'x']]) {
      const r = run(...args)
      expect({ args, code: r.status }).toEqual({ args, code: EXIT.usage })
      expect(r.stderr).toStartWith('rust.ts: ')
      expect(r.stdout).toBe('')
    }
  })

  test('the package scripts run it', () => {
    const pkg = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8'))
    expect(pkg.scripts).toMatchObject({
      'rs:build': 'bun scripts/rust.ts build',
      'rs:test': 'bun scripts/rust.ts test',
      'rs:test:release': 'bun scripts/rust.ts test --release',
      'rs:clippy': 'bun scripts/rust.ts clippy',
      'rs:fmt:check': 'bun scripts/rust.ts fmt --check',
    })
  })
})
