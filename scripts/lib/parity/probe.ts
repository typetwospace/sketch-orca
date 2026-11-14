// The probe programs (parity/oracle/probe/): they record what the C++ engine's foundations do (the
// static UCRT's maths, MSVC 14.42's casts, printf, to_chars, iostreams and parsers, the STL's
// algorithms, containers and random numbers, oneTBB 2021.5.0's split trees, and Orca's ConfigDef)
// and, from phase O2, the libraries the model's reading reaches (geometry, meshes, archives,
// Boost's property_tree, arrangement, the placeholder parser) into SOP1 files (parity/SOP1.md,
// "Probe files") and configdef.json, which the Rust crates' probe tests replay.
//
// The registry (PROBE_REGISTRY, 2026-11-07) lists the probes: one program and one file each,
// sketch-orca-probe-<name> from parity/oracle/probe/main.cpp and <name>.cpp. parity probe builds
// and runs them all; parity probe --only <name> builds and runs one and writes only its file. Each
// is built against the parity oracle's build tree (run parity oracle first), with the engine's own
// compile line (that of cli/config_def.cpp, read from the oracle's build.ninja) and link line
// (sketch-orca.exe's libraries and flags), the oracle's compiled gui_symbols.cpp, with CMake and
// Ninja into <work>/oracle/probe-build: no Fortran compiler is looked for and no IDE project can
// be made (noide.ts checks the folder before and after each build). The CRT maths functions the
// crt probe calls are the ones the oracle's linker map lists.
//
// A run writes two sets from the same cases: the full set into <work>/parity/probes and the
// committed subset into the repository's parity/probes, each file within its budget (crt, fmt,
// stl, tbb and configdef.json under PROBE_COMMITTED_MAX_BYTES together, every later file
// PROBE_FILE_MAX_BYTES at most). Nothing in them depends on the machine, the time or the folder, so
// a second run (parity probe --check) must write the same bytes.
//
// From phase O3 (orca-o3-skeleton, 2026-11-13) each new probe's entry is a file of its own,
// probes/<name>.ts, imported here after the O2 entries; ProbeEntry and probe() are in
// probes/entry.ts. Each O3 probe has a kind prefix and a page of its own, parity/sop1/<name>.md,
// where its record kinds' ### sections go; SOP1.md and those pages are read together
// (sop1Pages), and probeKindProblems holds the kind rule that lets the probe units record side by
// side: distinct prefixes that no kind of SOP1.md starts with, no kind defined on two pages, and
// every kind an O3 probe's committed file holds beyond HEAD, PRUN and END starting with its prefix
// and defined on its own page.

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { formatCommand, WIN32_DEP_CMAKE_OPTS } from '../cmake.ts'
import { compilerEnv as sharedCompilerEnv } from '../depsinputs.ts'
import { assertNoIdeProjects } from '../noide.ts'
import { readStamp, sha256File, sha256Text, writeStamp, type ParityPaths, type WorkPaths } from '../paths.ts'
import { runLogged, StepError, UsageError, type Log } from '../run.ts'
import { engineLibraryDirs } from '../smoke.ts'
import type { Toolchain } from '../toolchain.ts'
import { readSop1 } from './neutral.ts'
import { ORACLE_EXE, ORACLE_MAP, oracleStateDir } from './oracle.ts'
import { entry as arachneProbe } from './probes/arachne.ts'
import { PROBE_FILE_MAX_BYTES, probe, type ProbeEntry } from './probes/entry.ts'
import { entry as fillProbe } from './probes/fill.ts'
import { entry as indexProbe } from './probes/index.ts'
import { entry as layerProbe } from './probes/layer.ts'
import { entry as pathsProbe } from './probes/paths.ts'
import { entry as sliceProbe } from './probes/slice.ts'
import { entry as surfacesProbe } from './probes/surfaces.ts'
import { entry as voronoiProbe } from './probes/voronoi.ts'
import { entry as wallsProbe } from './probes/walls.ts'
import { runHidden, type RunProcess } from './same.ts'

export { PROBE_FILE_MAX_BYTES, probe, type ProbeEntry }

/** The committed size limit of the five foundation files (crt, fmt, stl, tbb, configdef.json) together. */
export const PROBE_COMMITTED_MAX_BYTES = 8 * 1024 * 1024

/**
 * The probes, in the order a full run builds and runs them. parity/oracle/probe/CMakeLists.txt's
 * PROBE_NAMES lists the same names in the same order (test/parity-probe.test.ts holds it). Phase
 * O3's nine come last, each from its own file under probes/ (2026-11-13).
 */
export const PROBE_REGISTRY: readonly ProbeEntry[] = [
  probe('crt', 'O1', { budget: 'foundations', crtSymbols: true }),
  probe('fmt', 'O1', { budget: 'foundations' }),
  probe('stl', 'O1', { budget: 'foundations' }),
  probe('tbb', 'O1', { budget: 'foundations' }),
  probe('configdef', 'O1', { file: 'configdef.json', budget: 'foundations' }),
  probe('geom', 'o2-geom'),
  probe('qhull', 'o2-mesh'),
  probe('mesh', 'o2-mesh'),
  probe('cstdio', 'o2-mesh'),
  probe('zip', 'o2-model'),
  probe('xml', 'o2-model'),
  probe('ptree', 'orca-ptree-probe'),
  probe('model', 'o2-model'),
  probe('arrange', 'o2-arrange'),
  probe('placeholder', 'o2-front'),
  layerProbe,
  indexProbe,
  pathsProbe,
  sliceProbe,
  voronoiProbe,
  arachneProbe,
  wallsProbe,
  surfacesProbe,
  fillProbe,
]

/** Every file a full probe run writes into each set, in the order it writes them. */
export const PROBE_FILES: readonly string[] = PROBE_REGISTRY.map((e) => e.file)

/** The probes a run takes: all of them, or the one --only names (a usage error for a name the registry lacks). */
export function probeSelection(only?: string): ProbeEntry[] {
  if (only === undefined) return [...PROBE_REGISTRY]
  const e = PROBE_REGISTRY.find((x) => x.name === only)
  if (!e) throw new UsageError(`parity probe --only ${only}: no such probe (${PROBE_REGISTRY.map((x) => x.name).join(', ')})`)
  return [e]
}

// ---------------------------------------------------------------- the record kinds' pages

/** One page that defines record kinds: parity/SOP1.md, or a probe's own page parity/sop1/<name>.md. */
export interface Sop1Page {
  /** `SOP1.md`, or `sop1/<name>.md`: the path under parity/. */
  readonly name: string
  readonly text: string
}

/** The page every O1 and O2 kind is defined on. */
export const SOP1_MAIN_PAGE = 'SOP1.md'

/** An O3 probe's own page, under parity/. */
export function probePage(name: string): string {
  return `sop1/${name}.md`
}

/**
 * The pages that define record kinds, read together wherever the ### KIND sections or the
 * backticked files are looked up: parity/SOP1.md first, then every parity/sop1/*.md by name.
 */
export function sop1Pages(engineDir: string): Sop1Page[] {
  const parity = join(engineDir, 'parity')
  const pages: Sop1Page[] = [{ name: SOP1_MAIN_PAGE, text: readFileSync(join(parity, SOP1_MAIN_PAGE), 'utf8') }]
  const dir = join(parity, 'sop1')
  if (existsSync(dir)) for (const f of readdirSync(dir).filter((n) => n.endsWith('.md')).sort()) pages.push({ name: `sop1/${f}`, text: readFileSync(join(dir, f), 'utf8') })
  return pages
}

/** The kinds a page defines: its `### KIND` headings (three or four capitals and digits alone on the line), in order. */
export function pageKinds(text: string): string[] {
  return [...text.matchAll(/^### ([A-Z][A-Z0-9]{2,3})[ \t]*\r?$/gm)].map((m) => m[1]!)
}

/** The record kinds a probe file holds beyond HEAD, PRUN and END, each once, sorted (a kind's padding trimmed). */
export function fileKinds(bytes: Buffer): string[] {
  const { records } = readSop1(bytes)
  return [...new Set(records.map((r) => r.kind.trimEnd()))].filter((k) => !['HEAD', 'PRUN', 'END'].includes(k)).sort()
}

/**
 * The kind rule, which lets phase O3's probe units record side by side without breaking each
 * other's runs. Given the pages (SOP1.md and parity/sop1/*.md), the registry's entries and the
 * committed files (by file name; a file not given is not judged), one problem per breach:
 *
 *   - the kind prefixes are distinct, and no kind of SOP1.md starts with one;
 *   - no `### KIND` heading is on two pages;
 *   - every kind an O3 probe's committed file holds beyond HEAD, PRUN and END starts with its
 *     prefix and has its `### KIND` section on its own page, sop1/<name>.md.
 */
export function probeKindProblems(o: { pages: readonly Sop1Page[]; entries: readonly ProbeEntry[]; files: Readonly<Record<string, Buffer>> }): string[] {
  const problems: string[] = []
  const withPrefix = o.entries.filter((e) => e.kindPrefix !== undefined)
  const byPrefix = new Map<string, string[]>()
  for (const e of withPrefix) byPrefix.set(e.kindPrefix!, [...(byPrefix.get(e.kindPrefix!) ?? []), e.name])
  for (const [prefix, names] of byPrefix) if (names.length > 1) problems.push(`the kind prefix ${prefix} is shared by the probes ${names.join(', ')}`)
  const main = o.pages.find((p) => p.name === SOP1_MAIN_PAGE)
  if (main) for (const k of pageKinds(main.text)) for (const e of withPrefix) if (k.startsWith(e.kindPrefix!)) problems.push(`the kind ${k} of ${SOP1_MAIN_PAGE} starts with the prefix ${e.kindPrefix} of the probe ${e.name}`)
  const pagesOf = new Map<string, string[]>()
  for (const p of o.pages) for (const k of new Set(pageKinds(p.text))) pagesOf.set(k, [...(pagesOf.get(k) ?? []), p.name])
  for (const [k, pages] of pagesOf) if (pages.length > 1) problems.push(`the kind ${k} is defined on ${pages.length} pages: ${pages.join(', ')}`)
  for (const e of withPrefix) {
    const bytes = o.files[e.file]
    if (bytes === undefined) continue
    const own = probePage(e.name)
    for (const k of fileKinds(bytes)) {
      if (!k.startsWith(e.kindPrefix!)) problems.push(`${e.file} holds the kind ${k}, which does not start with its prefix ${e.kindPrefix}`)
      if (!(pagesOf.get(k) ?? []).includes(own)) problems.push(`${e.file} holds the kind ${k}, which has no ### ${k} section on its page parity/${own}`)
    }
  }
  return problems
}

/** A probe's CMake target and program name. */
export function probeTarget(name: string): string {
  return `sketch-orca-probe-${name}`
}

/** A probe's program, as the build writes it. */
export function probeExe(name: string): string {
  return `${probeTarget(name)}.exe`
}

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
  /** Each built probe's program, by name. */
  exes: Record<string, string>
  crtSymbols: string[]
  configure: string[]
}

/** The build line for some probes: every target for a full run, else only theirs. */
export function probeBuildArgs(build: string, entries: readonly ProbeEntry[]): string[] {
  const all = entries.length === PROBE_REGISTRY.length && PROBE_REGISTRY.every((e) => entries.includes(e))
  const targets = all ? [] : ['--target', ...entries.map((e) => probeTarget(e.name))]
  return ['--build', build.replace(/\\/g, '/'), ...targets, '--', '-j', '2']
}

/**
 * Builds probes against the oracle's build tree: reads the compile and link lines from its
 * build.ninja, writes them as response files, configures when they changed and builds the
 * entries' targets (all of them by default) with Ninja.
 */
export async function buildProbe(o: { t: Toolchain; work: WorkPaths; paths: ParityPaths; engineDir: string; log: Log; entries?: readonly ProbeEntry[] }): Promise<ProbeBuild> {
  const entries = o.entries ?? PROBE_REGISTRY
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
  const buildArgs = probeBuildArgs(build, entries)
  say(`building: ${formatCommand(o.t.cmake, buildArgs)}`)
  await runLogged(o.t.cmake, buildArgs, { env, log: o.log, cwd: build })
  // Ninja and CXX only, so no step can run devenv.exe; held to it all the same.
  assertNoIdeProjects([build], 'parity probe')
  say('no CompilerIdFortran folder or .vfproj file in the build folder')
  const exes: Record<string, string> = {}
  for (const e of entries) {
    const exe = join(build, probeExe(e.name))
    if (!existsSync(exe)) throw new StepError(`the build finished but ${exe} is missing`)
    exes[e.name] = exe
  }
  return { exes, crtSymbols, configure }
}

function writeIfChanged(path: string, text: string): void {
  if (existsSync(path) && readFileSync(path, 'utf8') === text) return
  writeFileSync(path, text)
}

/**
 * Runs probes hidden, one after the other in the registry's order, with the oracle's libraries on
 * their path. A full run (every entry) starts from two emptied folders, so they hold exactly the
 * registry's files; a run of some entries removes only their files first and leaves the rest.
 */
export async function runProbe(o: {
  entries: readonly ProbeEntry[]
  exes: Record<string, string>
  crtSymbolsFile: string
  committed: string
  full: string
  libraryDirs: string[]
  run?: RunProcess
  timeoutMs?: number
}): Promise<void> {
  const all = o.entries.length === PROBE_REGISTRY.length && PROBE_REGISTRY.every((e) => o.entries.includes(e))
  for (const d of [o.committed, o.full]) {
    if (all) rmSync(d, { recursive: true, force: true })
    mkdirSync(d, { recursive: true })
    if (!all) for (const e of o.entries) rmSync(join(d, e.file), { force: true })
  }
  const env = { ...(process.env as Record<string, string>) }
  const pathKey = Object.keys(env).find((k) => k.toUpperCase() === 'PATH') ?? 'PATH'
  env[pathKey] = [...o.libraryDirs, env[pathKey] ?? ''].join(';')
  const timeoutMs = o.timeoutMs ?? PROBE_TIMEOUT_MS
  for (const e of o.entries) {
    const exe = o.exes[e.name]
    if (exe === undefined) throw new StepError(`no program for the probe ${e.name}`)
    const args = ['--committed', o.committed, '--full', o.full, ...(e.crtSymbols ? ['--crt-symbols', o.crtSymbolsFile] : [])]
    const r = await (o.run ?? runHidden)(exe, args, { cwd: o.full, env: env as never, timeoutMs })
    if (r.timedOut) throw new StepError(`${exe} did not finish within ${timeoutMs / 60000} minutes`)
    if (r.code !== 0) throw new StepError(`${formatCommand(exe, args)} exited with ${r.code}: ${r.stderr.toString('utf8').trim()}`)
    for (const d of [o.committed, o.full]) if (!existsSync(join(d, e.file))) throw new StepError(`the probe ${e.name} exited 0 but wrote no ${join(d, e.file)}`)
  }
}

/** The probe's library folders: the staged oracle's bin (its DLLs) and the dependency build's. */
export function probeLibraryDirs(work: WorkPaths, paths: Pick<ParityPaths, 'oracleDist'>): string[] {
  return engineLibraryDirs(work, join(paths.oracleDist, 'bin', ORACLE_EXE), 'win32')
}

/**
 * The committed files' budgets, for the entries a run wrote: the foundation files (when any of them
 * ran) under `limits.foundations` together, every other file at most its own budget. One problem
 * per file or group over its limit; none when all fit.
 */
export function committedSizeProblems(
  dir: string,
  entries: readonly ProbeEntry[] = PROBE_REGISTRY,
  limits: { foundations: number } = { foundations: PROBE_COMMITTED_MAX_BYTES },
): string[] {
  const size = (f: string) => (existsSync(join(dir, f)) ? statSync(join(dir, f)).size : 0)
  const problems: string[] = []
  if (entries.some((e) => e.budget === 'foundations')) {
    const group = PROBE_REGISTRY.filter((e) => e.budget === 'foundations')
    const total = group.reduce((n, e) => n + size(e.file), 0)
    if (total >= limits.foundations) problems.push(`the committed foundation probe files (${group.map((e) => e.file).join(', ')}) in ${dir} take ${total} bytes together, over ${limits.foundations}`)
  }
  for (const e of entries) {
    if (e.budget === 'foundations') continue
    const n = size(e.file)
    if (n > e.budget) problems.push(`the committed ${e.file} in ${dir} takes ${n} bytes, over its ${e.budget}`)
  }
  return problems
}

/** Files that differ between two probe folders (missing, extra or other bytes), by name; only `files` when given. */
export function probeDifferences(a: string, b: string, files?: readonly string[]): string[] {
  const out: string[] = []
  const names = new Set<string>(files ?? [...(existsSync(a) ? readdirSync(a) : []), ...(existsSync(b) ? readdirSync(b) : [])])
  for (const n of [...names].sort()) {
    const pa = join(a, n)
    const pb = join(b, n)
    if (!existsSync(pa) && !existsSync(pb)) out.push(`${n}: in neither ${a} nor ${b}`)
    else if (!existsSync(pa)) out.push(`${n}: only in ${b}`)
    else if (!existsSync(pb)) out.push(`${n}: only in ${a}`)
    else if (sha256File(pa) !== sha256File(pb)) out.push(`${n}: differs`)
  }
  return out
}
