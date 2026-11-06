// The probe program (parity/oracle/probe/): it records what the C++ engine's foundations do (the
// static UCRT's maths, MSVC 14.42's casts, printf, to_chars, iostreams and parsers, the STL's
// algorithms, containers and random numbers, oneTBB 2021.5.0's split trees, and Orca's ConfigDef)
// into SOP1 files (parity/SOP1.md, "Probe files") and configdef.json, which the Rust crates'
// probe tests replay.
//
// It is built against the parity oracle's build tree (run parity oracle first), with the engine's
// own compile line (that of cli/config_def.cpp, read from the oracle's build.ninja) and link line
// (sketch-orca.exe's libraries and flags), the oracle's compiled gui_symbols.cpp, with CMake and
// Ninja into <work>/oracle/probe-build: no Fortran compiler is looked for and no IDE project can
// be made (noide.ts checks the folder after each step). The CRT maths functions it probes are the
// ones the oracle's linker map lists.
//
// One run writes two sets from the same cases: the full set into <work>/parity/probes and the
// committed subset into the repository's parity/probes (under PROBE_COMMITTED_MAX_BYTES together).
// Nothing in them depends on the machine, the time or the folder, so a second run (parity probe
// --check) must write the same bytes.

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { formatCommand, WIN32_DEP_CMAKE_OPTS } from '../cmake.ts'
import { compilerEnv as sharedCompilerEnv } from '../depsinputs.ts'
import { assertNoIdeProjects } from '../noide.ts'
import { readStamp, sha256File, sha256Text, writeStamp, type ParityPaths, type WorkPaths } from '../paths.ts'
import { runLogged, StepError, type Log } from '../run.ts'
import { engineLibraryDirs } from '../smoke.ts'
import type { Toolchain } from '../toolchain.ts'
import { ORACLE_EXE, ORACLE_MAP, oracleStateDir } from './oracle.ts'
import { runHidden, type RunProcess } from './same.ts'

/** The files a probe run writes into each set, in the order it writes them. */
export const PROBE_FILES = ['crt.sop1', 'fmt.sop1', 'stl.sop1', 'tbb.sop1', 'configdef.json'] as const

/** The committed set's size limit, the five files together. */
export const PROBE_COMMITTED_MAX_BYTES = 8 * 1024 * 1024

export const PROBE_EXE = 'sketch-orca-probe.exe'

/** How long one probe run may take before it is ended. */
export const PROBE_TIMEOUT_MS = 30 * 60 * 1000

/** The probe program's sources, in the repository. */
export function probeSourceDir(engineDir: string): string {
  return join(engineDir, 'parity', 'oracle', 'probe')
}

/** The committed set, in the repository. */
export function probeCommittedDir(engineDir: string): string {
  return join(engineDir, 'parity', 'probes')
}

/** Where the probe is configured and built. */
export function probeBuildDir(paths: Pick<ParityPaths, 'oracle'>): string {
  return join(paths.oracle, 'probe-build')
}

// ---------------------------------------------------------------- the CRT maths symbols

/**
 * The C library's maths functions, by the names the static UCRT defines them under: C17 7.12's
 * functions with their f and l forms, and MSVC's own (_copysign, _hypot, _finite and the like).
 * The SVML vector functions MSVC's auto-vectoriser calls (__vdecl_*) count too.
 */
const C_MATHS = [
  'acos', 'asin', 'atan', 'atan2', 'cos', 'sin', 'tan', 'acosh', 'asinh', 'atanh', 'cosh', 'sinh', 'tanh',
  'exp', 'exp2', 'expm1', 'frexp', 'ilogb', 'ldexp', 'log', 'log10', 'log1p', 'log2', 'logb', 'modf',
  'scalbn', 'scalbln', 'cbrt', 'fabs', 'hypot', 'pow', 'sqrt', 'erf', 'erfc', 'lgamma', 'tgamma', 'ceil',
  'floor', 'nearbyint', 'rint', 'lrint', 'llrint', 'round', 'lround', 'llround', 'trunc', 'fmod',
  'remainder', 'remquo', 'copysign', 'nan', 'nextafter', 'nexttoward', 'fdim', 'fmax', 'fmin', 'fma',
]
const MSVC_MATHS = [
  '_copysign', '_copysignf', '_chgsign', '_chgsignf', '_hypot', '_hypotf', '_finite', '_finitef', '_isnan',
  '_isnanf', '_fpclass', '_fpclassf', '_logb', '_logbf', '_nextafter', '_nextafterf', '_scalb', '_scalbf',
  '_j0', '_j1', '_jn', '_y0', '_y1', '_yn', '_cabs',
]
export const CRT_MATHS_NAMES: ReadonlySet<string> = new Set([...C_MATHS.flatMap((n) => [n, `${n}f`, `${n}l`]), ...MSVC_MATHS])

/**
 * The CRT maths functions a linker map lists: the code symbols ("f") of the UCRT (libucrt) or the
 * C runtime's startup library (LIBCMT, which holds the SVML dispatchers) whose names are maths
 * functions. Sorted, each once.
 */
export function crtMathsSymbols(map: string): string[] {
  const out = new Set<string>()
  for (const line of map.split(/\r?\n/)) {
    const m = /^\s*[0-9a-f]{4}:[0-9a-f]{8}\s+(\S+)\s+[0-9a-f]{16}\s+f\s+(?:i\s+)?(\S+):(\S+)\s*$/i.exec(line)
    if (!m) continue
    const [, name, lib] = m
    if (!/^(libucrt|LIBCMT)$/i.test(lib!)) continue
    if (CRT_MATHS_NAMES.has(name!) || name!.startsWith('__vdecl_')) out.add(name!)
  }
  return [...out].sort()
}

// ---------------------------------------------------------------- the engine's compile and link lines

/** Ninja's escapes undone: `$:`, `$ `, `$$` and `$\n`. */
export function ninjaUnescape(s: string): string {
  return s.replace(/\$([: $\n])/g, '$1')
}

/**
 * The variables of the build statement whose first output ends with `outputSuffix` (backslashes
 * and slashes alike), from a build.ninja: each indented `NAME = value` line under it.
 */
export function ninjaBuildVars(ninja: string, outputSuffix: string): Record<string, string> | undefined {
  const want = outputSuffix.replace(/\\/g, '/').toLowerCase()
  const lines = ninja.split(/\r?\n/)
  for (let i = 0; i < lines.length; ++i) {
    const m = /^build (.+?)(?<!\$):/.exec(lines[i]!)
    if (!m) continue
    const first = ninjaUnescape(m[1]!.split(/(?<!\$) /)[0]!).replace(/\\/g, '/').toLowerCase()
    if (!first.endsWith(want)) continue
    const vars: Record<string, string> = {}
    for (let j = i + 1; j < lines.length; ++j) {
      const v = /^ {2}([A-Za-z_][A-Za-z0-9_]*) = ?(.*)$/.exec(lines[j]!)
      if (!v) break
      vars[v[1]!] = v[2]!
    }
    return vars
  }
  return undefined
}

/** A Ninja variable's value split into the arguments cl and link see: spaces split, quotes keep. */
export function splitArgs(s: string): string[] {
  const out: string[] = []
  let cur = ''
  let quoted = false
  let any = false
  for (let i = 0; i < s.length; ++i) {
    const c = s[i]!
    if (c === '\\' && s[i + 1] === '"') {
      cur += '\\"'
      any = true
      ++i
    } else if (c === '"') {
      quoted = !quoted
      cur += c
      any = true
    } else if (c === ' ' && !quoted) {
      if (any) out.push(cur)
      cur = ''
      any = false
    } else {
      cur += c
      any = true
    }
  }
  if (any) out.push(cur)
  return out
}

/** Flags of the engine's compile line the probe leaves out: debug information, parallel builds, precompiled headers. */
export function isDroppedCompileFlag(a: string): boolean {
  return /^[/-](Zi|Z7|FS|MP\d*|Yu.*|Yc.*|Fp.*|FI.*|Fd.*)$/i.test(a)
}

/** The probe's compile arguments: the defines, include folders and flags of the engine's cli/config_def.cpp. */
export function probeCompileArgs(vars: Record<string, string>): string[] {
  for (const k of ['DEFINES', 'INCLUDES', 'FLAGS']) if (vars[k] === undefined) throw new StepError(`the oracle's compile line of config_def.cpp has no ${k}`)
  return [vars.DEFINES!, vars.INCLUDES!, vars.FLAGS!].flatMap((v) => splitArgs(ninjaUnescape(v))).filter((a) => !isDroppedCompileFlag(a))
}

/**
 * A path of the oracle's link line made absolute: the libraries in its build tree are relative to
 * it. A bare name (kernel32.lib, opengl32.lib) is the linker's to find on LIB, and stays.
 */
export function absoluteIn(build: string, p: string): string {
  if (/^[A-Za-z]:[\\/]/.test(p) || p.startsWith('\\\\') || p.startsWith('/') || p.startsWith('-') || p.startsWith('"')) return p
  if (/[\\/]/.test(p) && /\.(lib|obj|a|o)$/i.test(p)) return join(build, p)
  return p
}

/** Flags of the engine's link line the probe leaves out: the map (it has its own name) and the manifest step. */
function isDroppedLinkFlag(a: string): boolean {
  return /^\/MAP(:.*)?$/i.test(a) || /^\/MANIFEST/i.test(a)
}

/**
 * The probe's link arguments: the engine's link flags, the oracle's compiled gui_symbols.cpp,
 * and the engine's libraries, each library of the oracle's build tree made absolute.
 */
export function probeLinkArgs(vars: Record<string, string>, oracleBuild: string, guiSymbolsObj: string): string[] {
  for (const k of ['LINK_FLAGS', 'LINK_LIBRARIES']) if (vars[k] === undefined) throw new StepError(`the oracle's link line of sketch-orca.exe has no ${k}`)
  const flags = splitArgs(ninjaUnescape(vars.LINK_FLAGS!)).filter((a) => !isDroppedLinkFlag(a))
  const libs = splitArgs(ninjaUnescape(vars.LINK_LIBRARIES!)).map((a) => absoluteIn(oracleBuild, a))
  return [...flags, guiSymbolsObj, ...libs]
}

/** A response file for cl or link: one argument a line, an argument with a space quoted. */
export function rspText(args: string[]): string {
  return args.map((a) => (/\s/.test(a) && !a.startsWith('"') ? `"${a}"` : a)).join('\n') + '\n'
}

/** The probe's configure line: Ninja, cl, the static runtime, no Fortran, the two response files. */
export function probeConfigureArgs(o: { ninja: string; sourceDir: string; buildDir: string; compileRsp: string; linkRsp: string }): string[] {
  const p = (s: string) => s.replace(/\\/g, '/')
  return [
    '-S', p(o.sourceDir),
    '-B', p(o.buildDir),
    '-G', 'Ninja',
    `-DCMAKE_MAKE_PROGRAM=${p(o.ninja)}`,
    '-DCMAKE_CXX_COMPILER=cl',
    '-DCMAKE_BUILD_TYPE=Release',
    '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW',
    '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded',
    ...WIN32_DEP_CMAKE_OPTS,
    `-DPROBE_COMPILE_RSP=${p(o.compileRsp)}`,
    `-DPROBE_LINK_RSP=${p(o.linkRsp)}`,
  ]
}

// ---------------------------------------------------------------- building and running

export interface ProbeBuild {
  exe: string
  crtSymbols: string[]
  configure: string[]
}

/**
 * Builds the probe against the oracle's build tree: reads the compile and link lines from its
 * build.ninja, writes them as response files, configures when they changed and builds with Ninja.
 */
export async function buildProbe(o: { t: Toolchain; work: WorkPaths; paths: ParityPaths; engineDir: string; log: Log }): Promise<ProbeBuild> {
  const say = (m: string) => o.log.line(`parity probe: ${m}`)
  if (o.t.platform !== 'win32') throw new StepError('parity probe records the Windows engine (the static UCRT and MSVC 14.42); run it on Windows')
  const oracleBuild = o.paths.oracleBuild
  const ninjaFile = join(oracleBuild, 'build.ninja')
  if (!existsSync(ninjaFile) || !readStamp(oracleStateDir(o.paths), 'build')) throw new StepError(`no finished oracle build in ${oracleBuild}; run parity oracle first`)
  const map = join(o.paths.oracleDist, 'bin', ORACLE_MAP)
  if (!existsSync(join(o.paths.oracleDist, 'bin', ORACLE_EXE)) || !existsSync(map)) throw new StepError(`no staged oracle with its linker map in ${o.paths.oracleDist}; run parity oracle first`)
  const ninja = readFileSync(ninjaFile, 'utf8')
  const compileVars = ninjaBuildVars(ninja, 'sketch-orca.dir/config_def.cpp.obj')
  if (!compileVars) throw new StepError(`${ninjaFile} has no compile line for cli/config_def.cpp`)
  const linkVars = ninjaBuildVars(ninja, 'sketch-orca/sketch-orca.exe')
  if (!linkVars) throw new StepError(`${ninjaFile} has no link line for sketch-orca.exe`)
  const guiSymbols = join(oracleBuild, 'sketch-orca', 'CMakeFiles', 'sketch-orca.dir', 'gui_symbols.cpp.obj')
  if (!existsSync(guiSymbols)) throw new StepError(`the oracle's build has no ${guiSymbols}; run parity oracle again`)
  const crtSymbols = crtMathsSymbols(readFileSync(map, 'utf8'))
  if (crtSymbols.length === 0) throw new StepError(`${map} lists no CRT maths function; is it the oracle's linker map?`)
  say(`${crtSymbols.length} CRT maths functions in ${map}: ${crtSymbols.join(' ')}`)

  const build = probeBuildDir(o.paths)
  mkdirSync(build, { recursive: true })
  const compileRsp = join(build, 'compile.rsp')
  const linkRsp = join(build, 'link.rsp')
  writeIfChanged(compileRsp, rspText(probeCompileArgs(compileVars)))
  writeIfChanged(linkRsp, rspText(probeLinkArgs(linkVars, oracleBuild, guiSymbols)))
  writeIfChanged(join(build, 'crt-symbols.txt'), crtSymbols.join('\n') + '\n')

  const source = probeSourceDir(o.engineDir)
  const cenv = sharedCompilerEnv(o.t, o.work, o.engineDir)
  const env: Record<string, string> = { ...o.t.env, ...cenv, GIT_CEILING_DIRECTORIES: o.work.home, MSBUILDDISABLENODEREUSE: '1' }
  const configure = probeConfigureArgs({ ninja: o.t.ninja, sourceDir: source, buildDir: build, compileRsp, linkRsp })
  const state = oracleStateDir(o.paths)
  const configureKey = sha256Text(JSON.stringify({ configure, cmake: o.t.cmake, compilerEnv: cenv }))
  if (readStamp(state, 'probe-configure')?.inputs !== configureKey || !existsSync(join(build, 'CMakeCache.txt'))) {
    say(`configuring ${build}`)
    await runLogged(o.t.cmake, configure, { env, log: o.log, cwd: build })
    writeStamp(state, 'probe-configure', configureKey)
  }
  assertNoIdeProjects([build], 'parity probe')
  const buildArgs = ['--build', build.replace(/\\/g, '/'), '--', '-j', '2']
  say(`building: ${formatCommand(o.t.cmake, buildArgs)}`)
  await runLogged(o.t.cmake, buildArgs, { env, log: o.log, cwd: build })
  // Ninja and CXX only, so no step can run devenv.exe; held to it all the same.
  assertNoIdeProjects([build], 'parity probe')
  say('no CompilerIdFortran folder or .vfproj file in the build folder')
  const exe = join(build, PROBE_EXE)
  if (!existsSync(exe)) throw new StepError(`the build finished but ${exe} is missing`)
  return { exe, crtSymbols, configure }
}

function writeIfChanged(path: string, text: string): void {
  if (existsSync(path) && readFileSync(path, 'utf8') === text) return
  writeFileSync(path, text)
}

/** Runs the probe hidden into two empty folders, with the oracle's libraries on its path. */
export async function runProbe(o: {
  exe: string
  crtSymbolsFile: string
  committed: string
  full: string
  libraryDirs: string[]
  run?: RunProcess
  timeoutMs?: number
}): Promise<void> {
  for (const d of [o.committed, o.full]) {
    rmSync(d, { recursive: true, force: true })
    mkdirSync(d, { recursive: true })
  }
  const env = { ...(process.env as Record<string, string>) }
  const pathKey = Object.keys(env).find((k) => k.toUpperCase() === 'PATH') ?? 'PATH'
  env[pathKey] = [...o.libraryDirs, env[pathKey] ?? ''].join(';')
  const args = ['--committed', o.committed, '--full', o.full, '--crt-symbols', o.crtSymbolsFile]
  const r = await (o.run ?? runHidden)(o.exe, args, { cwd: o.full, env: env as never, timeoutMs: o.timeoutMs ?? PROBE_TIMEOUT_MS })
  if (r.timedOut) throw new StepError(`${o.exe} did not finish within ${(o.timeoutMs ?? PROBE_TIMEOUT_MS) / 60000} minutes`)
  if (r.code !== 0) throw new StepError(`${formatCommand(o.exe, args)} exited with ${r.code}: ${r.stderr.toString('utf8').trim()}`)
  for (const d of [o.committed, o.full]) for (const f of PROBE_FILES) if (!existsSync(join(d, f))) throw new StepError(`the probe exited 0 but wrote no ${join(d, f)}`)
}

/** The probe's library folders: the staged oracle's bin (its DLLs) and the dependency build's. */
export function probeLibraryDirs(work: WorkPaths, paths: Pick<ParityPaths, 'oracleDist'>): string[] {
  return engineLibraryDirs(work, join(paths.oracleDist, 'bin', ORACLE_EXE), 'win32')
}

/** The committed set's size, the files together, and a problem when it is over the limit. */
export function committedSizeProblem(dir: string, max = PROBE_COMMITTED_MAX_BYTES): string | undefined {
  const total = PROBE_FILES.reduce((n, f) => n + (existsSync(join(dir, f)) ? statSync(join(dir, f)).size : 0), 0)
  return total < max ? undefined : `the committed probe files in ${dir} take ${total} bytes, over ${max}`
}

/** Files that differ between two probe folders (missing, extra or other bytes), by name. */
export function probeDifferences(a: string, b: string): string[] {
  const out: string[] = []
  const names = new Set<string>([...(existsSync(a) ? readdirSync(a) : []), ...(existsSync(b) ? readdirSync(b) : [])])
  for (const n of [...names].sort()) {
    const pa = join(a, n)
    const pb = join(b, n)
    if (!existsSync(pa)) out.push(`${n}: only in ${b}`)
    else if (!existsSync(pb)) out.push(`${n}: only in ${a}`)
    else if (sha256File(pa) !== sha256File(pb)) out.push(`${n}: differs`)
  }
  return out
}
