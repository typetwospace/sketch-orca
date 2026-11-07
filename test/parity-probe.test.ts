// The probe programs' harness (scripts/lib/parity/probe.ts and cmd-probe.ts): the registry of
// probes (one program and one file each), --only and --check on the command line, the CRT maths
// functions picked out of a linker map, the engine's compile and link lines read from a
// build.ninja and turned into the probes' response files, their configure line (Ninja, no
// Fortran) and build line, running them and comparing two runs, the per-file budgets; and the
// committed probe files themselves (parity/probes/): SOP1 files that start with the probe's HEAD
// and PRUN, a configdef.json, each within its budget. Hermetic: temporary folders, a fake probe
// process and the committed files only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parseProbeArgs, usage } from '../scripts/lib/parity/cmd-probe.ts'
import { parseHead, readSop1, Sop1Reader } from '../scripts/lib/parity/neutral.ts'
import {
  absoluteIn,
  committedSizeProblems,
  CRT_MATHS_NAMES,
  crtMathsSymbols,
  isDroppedCompileFlag,
  ninjaBuildVars,
  ninjaUnescape,
  PROBE_COMMITTED_MAX_BYTES,
  PROBE_FILE_MAX_BYTES,
  PROBE_FILES,
  PROBE_REGISTRY,
  probeBuildArgs,
  probeCommittedDir,
  probeCompileArgs,
  probeConfigureArgs,
  probeDifferences,
  probeExe,
  probeLinkArgs,
  probeSelection,
  probeSourceDir,
  probeTarget,
  rspText,
  runProbe,
  splitArgs,
  type ProbeEntry,
} from '../scripts/lib/parity/probe.ts'
import { UsageError } from '../scripts/lib/run.ts'
import type { RunProcess } from '../scripts/lib/parity/same.ts'

const ROOT = join(import.meta.dir, '..')

let work: string
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-probe-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

const entry = (name: string): ProbeEntry => PROBE_REGISTRY.find((e) => e.name === name)!

describe('the registry', () => {
  test('one probe per file: the five foundation files of O1 and the nine files of phase O2, each with its task', () => {
    expect(PROBE_REGISTRY.map((e) => e.name)).toEqual(['crt', 'fmt', 'stl', 'tbb', 'configdef', 'geom', 'qhull', 'mesh', 'cstdio', 'zip', 'xml', 'model', 'arrange', 'placeholder'])
    expect(PROBE_FILES).toEqual(PROBE_REGISTRY.map((e) => e.file))
    expect(new Set(PROBE_FILES).size).toBe(PROBE_FILES.length)
    for (const e of PROBE_REGISTRY) expect(e.file).toBe(e.name === 'configdef' ? 'configdef.json' : `${e.name}.sop1`)
    const owners = Object.fromEntries(PROBE_REGISTRY.map((e) => [e.name, e.owner]))
    expect(owners).toEqual({
      crt: 'O1', fmt: 'O1', stl: 'O1', tbb: 'O1', configdef: 'O1',
      geom: 'o2-geom', qhull: 'o2-mesh', mesh: 'o2-mesh', cstdio: 'o2-mesh', zip: 'o2-model', xml: 'o2-model', model: 'o2-model', arrange: 'o2-arrange', placeholder: 'o2-front',
    })
  })

  test('the foundation files share the 8 MB budget; every other file has 512 KB of its own; only crt takes the map\'s symbols', () => {
    expect(PROBE_COMMITTED_MAX_BYTES).toBe(8 * 1024 * 1024)
    expect(PROBE_FILE_MAX_BYTES).toBe(512 * 1024)
    for (const e of PROBE_REGISTRY) expect(e.budget).toBe(e.owner === 'O1' ? 'foundations' : PROBE_FILE_MAX_BYTES)
    expect(PROBE_REGISTRY.filter((e) => e.crtSymbols).map((e) => e.name)).toEqual(['crt'])
  })

  test('a probe is the program sketch-orca-probe-<name>; --only picks one, and a name the registry lacks is a usage error', () => {
    expect(probeTarget('geom')).toBe('sketch-orca-probe-geom')
    expect(probeExe('geom')).toBe('sketch-orca-probe-geom.exe')
    expect(probeSelection()).toEqual([...PROBE_REGISTRY])
    expect(probeSelection('geom')).toEqual([entry('geom')])
    expect(() => probeSelection('nope')).toThrow(UsageError)
    expect(() => probeSelection('nope')).toThrow(/no such probe \(crt, fmt, .*placeholder\)/)
  })

  test('the command line: --check and --only <name>, with and without each other; nothing else', () => {
    expect(parseProbeArgs([])).toEqual({ check: false })
    expect(parseProbeArgs(['--check'])).toEqual({ check: true })
    expect(parseProbeArgs(['--only', 'geom'])).toEqual({ check: false, only: 'geom' })
    expect(parseProbeArgs(['--only=geom', '--check'])).toEqual({ check: true, only: 'geom' })
    expect(parseProbeArgs(['--check', '--only', 'crt'])).toEqual({ check: true, only: 'crt' })
    for (const bad of [['--only'], ['--only', '--check'], ['--only='], ['--only', 'a', '--only', 'b'], ['--all'], ['geom']]) expect(() => parseProbeArgs(bad)).toThrow(UsageError)
    expect(usage).toContain('--only <name>')
    expect(usage).toContain('512 KB')
  })

  test('a full run builds every target; --only builds its own', () => {
    expect(probeBuildArgs('C:\\w\\oracle\\probe-build', PROBE_REGISTRY)).toEqual(['--build', 'C:/w/oracle/probe-build', '--', '-j', '2'])
    expect(probeBuildArgs('C:\\b', [entry('geom')])).toEqual(['--build', 'C:/b', '--target', 'sketch-orca-probe-geom', '--', '-j', '2'])
  })

  test('the probe sources: main.cpp, probe.hpp and one <name>.cpp per probe; CMakeLists.txt lists the registry\'s names in its order', () => {
    const dir = probeSourceDir(ROOT)
    for (const f of ['CMakeLists.txt', 'probe.hpp', 'main.cpp', ...PROBE_REGISTRY.map((e) => `${e.name}.cpp`)]) expect({ f, there: existsSync(join(dir, f)) }).toEqual({ f, there: true })
    const cmake = readFileSync(join(dir, 'CMakeLists.txt'), 'utf8')
    expect(cmake).toContain('LANGUAGES CXX')
    expect(cmake).not.toMatch(/Fortran\)|enable_language/)
    const names = /^set\(PROBE_NAMES ([^)]*)\)$/m.exec(cmake)?.[1]?.split(/\s+/)
    expect(names).toEqual(PROBE_REGISTRY.map((e) => e.name))
    for (const e of PROBE_REGISTRY) {
      const source = readFileSync(join(dir, `${e.name}.cpp`), 'utf8')
      expect({ name: e.name, target: source.includes(`static const Target t{"${e.name}", ${e.crtSymbols}, `) || source.includes(`static const probe::Target t{"${e.name}", ${e.crtSymbols}, `) }).toEqual({ name: e.name, target: true })
    }
  })
})

describe('the CRT maths functions of a linker map', () => {
  const map = [
    ' Address         Publics by Value              Rva+Base               Lib:Object',
    ' 0001:009110a0       floor                      00000001409120a0 f   libucrt:floor.obj',
    ' 0001:0093b0f0       _copysign                  000000014093c0f0 f   libucrt:_copysign.obj',
    ' 0001:008fbfa0       __vdecl_floor2             00000001408fcfa0 f   LIBCMT:svml_dfloor2_dispatch.obj',
    ' 0001:0090d528       _dclass                    000000014090e528 f   libucrt:nan.obj',
    ' 0001:00910000       printf                     0000000140911000 f   libucrt:printf.obj',
    ' 0001:00920000       floor                      0000000140921000 f   libslic3r:Geometry.obj',
    ' 0002:00000010       sqrt                       0000000140a00010     libucrt:sqrt.obj',
    ' 0001:009279d4       fma                        00000001409289d4 f i libucrt:fma.obj',
    ' 0001:009110a0       floor                      00000001409120a0 f   libucrt:floor.obj',
  ].join('\r\n')

  test('takes the code symbols of libucrt and LIBCMT that are maths functions, sorted, each once', () => {
    expect(crtMathsSymbols(map)).toEqual(['__vdecl_floor2', '_copysign', 'floor', 'fma'])
  })

  test('the names cover C17 7.12 with the f and l forms and MSVC\'s own', () => {
    for (const n of ['acos', 'acosf', 'acosl', 'nexttoward', 'llround', 'fmaxf', '_hypot', '_finite', '_isnan', '_fpclass']) expect(CRT_MATHS_NAMES.has(n)).toBe(true)
    for (const n of ['printf', '_dclass', '_dxp_addh', 'memcpy']) expect(CRT_MATHS_NAMES.has(n)).toBe(false)
  })
})

describe('the engine\'s compile and link lines', () => {
  const ninja = [
    'rule CXX_COMPILER__sketch-orca_unscanned_Release',
    '  command = cl.exe $DEFINES $INCLUDES $FLAGS',
    '',
    'build sketch-orca\\CMakeFiles\\sketch-orca.dir\\config_def.cpp.obj: CXX_COMPILER__sketch-orca_unscanned_Release C$:\\w\\oracle\\src\\sketch-orca\\cli\\config_def.cpp || order',
    '  DEFINES = -DBOOST_ALL_NO_LIB -DGIT_COMMIT_HASH=\\"8500fcdc\\" -DNOMINMAX',
    '  FLAGS = /DWIN32 /W3 /GR /EHsc /O2 /Ob2 /DNDEBUG -std:c++17 -MT /MP -bigobj /Zi /FS /utf-8 /YuC:/w/pch.hxx /FpC:/w/pch.pch /FIC:/w/pch.hxx',
    '  INCLUDES = -IC:\\w\\oracle\\build\\sketch-orca\\generated -external:IC:\\w\\deps\\include -external:W0',
    '  OBJECT_DIR = sketch-orca\\CMakeFiles\\sketch-orca.dir',
    '',
    'build sketch-orca\\sketch-orca.exe: CXX_EXECUTABLE_LINKER__sketch-orca_Release sketch-orca\\CMakeFiles\\sketch-orca.dir\\main.cpp.obj | src\\libslic3r\\libslic3r.lib',
    '  FLAGS = /DWIN32 /O2 -MT',
    '  LINK_FLAGS = /MAP /machine:x64 /LTCG /INCREMENTAL:NO /subsystem:console',
    '  LINK_LIBRARIES = src\\libslic3r\\libslic3r.lib  C:\\w\\deps\\lib\\tbb12.lib  opengl32.lib  kernel32.lib',
    '  TARGET_FILE = sketch-orca\\sketch-orca.exe',
    '',
  ].join('\n')

  test('a build statement\'s variables, found by the end of its first output', () => {
    const v = ninjaBuildVars(ninja, 'sketch-orca.dir/config_def.cpp.obj')!
    expect(Object.keys(v)).toEqual(['DEFINES', 'FLAGS', 'INCLUDES', 'OBJECT_DIR'])
    expect(ninjaBuildVars(ninja, 'sketch-orca/sketch-orca.exe')!.LINK_FLAGS).toBe('/MAP /machine:x64 /LTCG /INCREMENTAL:NO /subsystem:console')
    expect(ninjaBuildVars(ninja, 'nothing.obj')).toBeUndefined()
  })

  test('Ninja\'s escapes are undone and a value splits as the command line does', () => {
    expect(ninjaUnescape('C$:\\w a$ b $$x')).toBe('C:\\w a b $x')
    expect(splitArgs('-DA  -DB=\\"x y\\" "C:\\a b\\c.lib" /O2')).toEqual(['-DA', '-DB=\\"x', 'y\\"', '"C:\\a b\\c.lib"', '/O2'])
  })

  test('the compile line keeps the defines, includes and code flags and drops PCH, debug information and /MP', () => {
    const args = probeCompileArgs(ninjaBuildVars(ninja, 'sketch-orca.dir/config_def.cpp.obj')!)
    expect(args).toContain('-DGIT_COMMIT_HASH=\\"8500fcdc\\"')
    expect(args).toContain('-IC:\\w\\oracle\\build\\sketch-orca\\generated')
    expect(args).toContain('-external:W0')
    for (const keep of ['/O2', '/Ob2', '-MT', '/EHsc', '-std:c++17', '/utf-8', '-bigobj']) expect(args).toContain(keep)
    for (const drop of ['/MP', '/Zi', '/FS']) expect(args).not.toContain(drop)
    expect(args.some((a) => /^\/(Yu|Fp|FI)/.test(a))).toBe(false)
    expect(isDroppedCompileFlag('/MP8')).toBe(true)
    expect(isDroppedCompileFlag('/O2')).toBe(false)
    expect(() => probeCompileArgs({ FLAGS: '' })).toThrow('no DEFINES')
  })

  test('the link line: the engine\'s flags without /MAP, gui_symbols, the libraries of the build tree made absolute', () => {
    const args = probeLinkArgs(ninjaBuildVars(ninja, 'sketch-orca/sketch-orca.exe')!, 'C:\\w\\oracle\\build', 'C:\\w\\gui_symbols.cpp.obj')
    expect(args).toEqual([
      '/machine:x64',
      '/LTCG',
      '/INCREMENTAL:NO',
      '/subsystem:console',
      'C:\\w\\gui_symbols.cpp.obj',
      join('C:\\w\\oracle\\build', 'src\\libslic3r\\libslic3r.lib'),
      'C:\\w\\deps\\lib\\tbb12.lib',
      'opengl32.lib',
      'kernel32.lib',
    ])
    expect(absoluteIn('C:\\b', 'kernel32.lib')).toBe('kernel32.lib')
    expect(absoluteIn('C:\\b', '/NODEFAULTLIB:x')).toBe('/NODEFAULTLIB:x')
  })

  test('a response file has one argument a line and quotes one with a space', () => {
    expect(rspText(['/O2', 'C:\\a b\\c.lib', '"q r"'])).toBe('/O2\n"C:\\a b\\c.lib"\n"q r"\n')
  })

  test('the probes are configured with Ninja and cl, the static runtime, no Fortran and Eigen\'s tests off', () => {
    const args = probeConfigureArgs({ ninja: 'C:\\t\\ninja.exe', sourceDir: 'C:\\r\\parity\\oracle\\probe', buildDir: 'C:\\w\\oracle\\probe-build', compileRsp: 'C:\\w\\c.rsp', linkRsp: 'C:\\w\\l.rsp' })
    expect(args.slice(0, 6)).toEqual(['-S', 'C:/r/parity/oracle/probe', '-B', 'C:/w/oracle/probe-build', '-G', 'Ninja'])
    for (const a of ['-DCMAKE_MAKE_PROGRAM=C:/t/ninja.exe', '-DCMAKE_CXX_COMPILER=cl', '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded', '-DCMAKE_Fortran_COMPILER:STRING=NOTFOUND', '-DEIGEN_BUILD_TESTING:BOOL=OFF', '-DPROBE_COMPILE_RSP=C:/w/c.rsp', '-DPROBE_LINK_RSP=C:/w/l.rsp']) expect(args).toContain(a)
    expect(args.join(' ')).not.toMatch(/Visual Studio/)
  })
})

describe('running the probes and comparing runs', () => {
  const exes = Object.fromEntries(PROBE_REGISTRY.map((e) => [e.name, `C:\\p\\${probeExe(e.name)}`]))
  /** A fake probe process: each program writes its own file into both folders, marked with the set. */
  const fake = (code = 0, skip?: string): { run: RunProcess; calls: string[][] } => {
    const calls: string[][] = []
    const run: RunProcess = async (exe, args, o) => {
      calls.push([exe, ...args, `timeout=${o.timeoutMs}`, `cwd=${o.cwd}`])
      const e = PROBE_REGISTRY.find((x) => exe.endsWith(probeExe(x.name)))!
      if (code === 0 && e.name !== skip) for (const [d, set] of [[args[1]!, 'committed'], [args[3]!, 'full']] as const) writeFileSync(join(d, e.file), `${e.file} of ${set}`)
      return { code, timedOut: false, stdout: Buffer.alloc(0), stderr: Buffer.from(code === 0 ? '' : 'sketch-orca-probe-crt: the oracle\'s map lists the CRT maths function ldexpf, which the probe does not know') }
    }
    return { run, calls }
  }

  test('a full run empties both folders and runs every program hidden, in the registry\'s order; only crt gets the map\'s symbols', async () => {
    const committed = join(work, 'committed')
    const full = join(work, 'full')
    mkdirSync(committed)
    writeFileSync(join(committed, 'stale.sop1'), 'old')
    const { run, calls } = fake()
    await runProbe({ entries: PROBE_REGISTRY, exes, crtSymbolsFile: 'C:\\p\\crt-symbols.txt', committed, full, libraryDirs: ['C:\\o\\bin'], run, timeoutMs: 1000 })
    expect(calls.map((c) => c[0])).toEqual(PROBE_REGISTRY.map((e) => exes[e.name]))
    expect(calls[0]).toEqual([exes.crt!, '--committed', committed, '--full', full, '--crt-symbols', 'C:\\p\\crt-symbols.txt', 'timeout=1000', `cwd=${full}`])
    expect(calls[5]).toEqual([exes.geom!, '--committed', committed, '--full', full, 'timeout=1000', `cwd=${full}`])
    expect(existsSync(join(committed, 'stale.sop1'))).toBe(false)
    for (const f of PROBE_FILES) expect(readFileSync(join(full, f), 'utf8')).toBe(`${f} of full`)
  })

  test('--only runs its one program and writes only its file: the other files stay as they were', async () => {
    const committed = join(work, 'committed')
    const full = join(work, 'full')
    for (const d of [committed, full]) {
      mkdirSync(d)
      for (const f of PROBE_FILES) writeFileSync(join(d, f), 'before')
    }
    const { run, calls } = fake()
    await runProbe({ entries: probeSelection('geom'), exes, crtSymbolsFile: 's', committed, full, libraryDirs: [], run })
    expect(calls.map((c) => c[0])).toEqual([exes.geom!])
    for (const d of [committed, full]) for (const f of PROBE_FILES) expect(readFileSync(join(d, f), 'utf8')).toBe(f === 'geom.sop1' ? `geom.sop1 of ${d === committed ? 'committed' : 'full'}` : 'before')
  })

  test('a failing probe, or one that leaves its file out, is an error; so is a probe without a program', async () => {
    const committed = join(work, 'c')
    const full = join(work, 'f')
    await expect(runProbe({ entries: PROBE_REGISTRY, exes, crtSymbolsFile: 's', committed, full, libraryDirs: [], run: fake(3).run })).rejects.toThrow('ldexpf')
    await expect(runProbe({ entries: PROBE_REGISTRY, exes, crtSymbolsFile: 's', committed, full, libraryDirs: [], run: fake(0, 'tbb').run })).rejects.toThrow('tbb.sop1')
    await expect(runProbe({ entries: probeSelection('xml'), exes: {}, crtSymbolsFile: 's', committed, full, libraryDirs: [], run: fake().run })).rejects.toThrow('no program for the probe xml')
  })

  test('two runs compare file by file, all of them or the files named', () => {
    const a = join(work, 'a')
    const b = join(work, 'b')
    for (const d of [a, b]) mkdirSync(d)
    for (const f of PROBE_FILES) writeFileSync(join(a, f), f)
    expect(probeDifferences(a, b).length).toBe(PROBE_FILES.length)
    for (const f of PROBE_FILES) writeFileSync(join(b, f), f)
    expect(probeDifferences(a, b)).toEqual([])
    writeFileSync(join(b, 'fmt.sop1'), 'other')
    writeFileSync(join(b, 'extra.txt'), '')
    expect(probeDifferences(a, b)).toEqual([`extra.txt: only in ${b}`, 'fmt.sop1: differs'])
    expect(probeDifferences(a, b, ['geom.sop1'])).toEqual([])
    expect(probeDifferences(a, b, ['fmt.sop1', 'none.sop1'])).toEqual(['fmt.sop1: differs', `none.sop1: in neither ${a} nor ${b}`])
  })

  test('the budgets: the foundation files together under their limit, every other file under its own', () => {
    const d = join(work, 'd')
    mkdirSync(d)
    for (const f of PROBE_FILES) writeFileSync(join(d, f), 'x'.repeat(100))
    expect(committedSizeProblems(d)).toEqual([])
    expect(committedSizeProblems(d, PROBE_REGISTRY, { foundations: 500 })).toEqual([`the committed foundation probe files (crt.sop1, fmt.sop1, stl.sop1, tbb.sop1, configdef.json) in ${d} take 500 bytes together, over 500`])
    writeFileSync(join(d, 'geom.sop1'), Buffer.alloc(PROBE_FILE_MAX_BYTES))
    expect(committedSizeProblems(d)).toEqual([])
    writeFileSync(join(d, 'geom.sop1'), Buffer.alloc(PROBE_FILE_MAX_BYTES + 1))
    expect(committedSizeProblems(d)).toEqual([`the committed geom.sop1 in ${d} takes ${PROBE_FILE_MAX_BYTES + 1} bytes, over its ${PROBE_FILE_MAX_BYTES}`])
    // --only checks its own budget: a foundation probe its group's, another probe its file's.
    expect(committedSizeProblems(d, probeSelection('xml'))).toEqual([])
    expect(committedSizeProblems(d, probeSelection('geom'))).toHaveLength(1)
    expect(committedSizeProblems(d, probeSelection('crt'), { foundations: 500 })).toHaveLength(1)
  })
})

describe('the committed probe files', () => {
  const dir = probeCommittedDir(ROOT)

  test('are all there, each within its budget', () => {
    let foundations = 0
    for (const e of PROBE_REGISTRY) {
      expect({ file: e.file, there: existsSync(join(dir, e.file)) }).toEqual({ file: e.file, there: true })
      const size = statSync(join(dir, e.file)).size
      if (e.budget === 'foundations') foundations += size
      else expect({ file: e.file, within: size <= e.budget }).toEqual({ file: e.file, within: true })
    }
    expect(foundations).toBeLessThan(PROBE_COMMITTED_MAX_BYTES)
    expect(committedSizeProblems(dir)).toEqual([])
  })

  test('each SOP1 file reads whole and starts with the probe\'s HEAD and PRUN for the committed set; every record kind is in SOP1.md', () => {
    const foundations: Record<string, string[]> = {
      'crt.sop1': ['MATH', 'MATS', 'CAST', 'RAND'],
      'fmt.sop1': ['PRTF', 'TOCH', 'OSTR', 'TOST', 'PARS'],
      'stl.sop1': ['SORT', 'VALS', 'HASH', 'UMAP', 'USET', 'RNGS', 'SSEQ', 'SHUF'],
      'tbb.sop1': ['TBBA', 'TBBF', 'TBBR', 'TBBO'],
    }
    const sop1 = readFileSync(join(ROOT, 'parity', 'SOP1.md'), 'utf8')
    for (const e of PROBE_REGISTRY.filter((x) => x.file.endsWith('.sop1'))) {
      const { records } = readSop1(readFileSync(join(dir, e.file)))
      expect(records[0]!.kind).toBe('HEAD')
      expect(parseHead(records[0]!.payload)).toEqual({ source: 'probe', name: e.name, object: -1n, occurrence: 1 })
      expect(records[1]!.kind).toBe('PRUN')
      const prun = new Sop1Reader(records[1]!.payload)
      const msc = prun.u32()
      expect(msc).toBeGreaterThanOrEqual(194200000)
      expect(msc).toBeLessThan(194300000)
      prun.u32()
      prun.i32()
      expect(prun.str()).toBe('committed')
      const kinds = [...new Set(records.slice(2).map((r) => r.kind))].sort()
      if (foundations[e.file]) expect(kinds).toEqual([...foundations[e.file]!].sort())
      for (const k of kinds) expect({ file: e.file, kind: k, documented: sop1.includes(`### ${k}`) }).toEqual({ file: e.file, kind: k, documented: true })
      expect({ file: e.file, inTable: sop1.includes(`\`${e.file}\``) }).toEqual({ file: e.file, inTable: true })
    }
  })

  test('crt.sop1 probes every maths function the oracle\'s map listed, with FMA3 on and off', () => {
    const { records } = readSop1(readFileSync(join(dir, 'crt.sop1')))
    const seen = new Map<string, Set<number>>()
    for (const r of records.filter((x) => x.kind === 'MATH')) {
      const p = new Sop1Reader(r.payload)
      const name = p.str()
      const state = p.i32()
      if (!seen.has(name)) seen.set(name, new Set())
      seen.get(name)!.add(state)
    }
    for (const n of ['floor', 'pow', 'atan2f', '__vdecl_floor2', '_hypot', 'nexttoward']) expect([...(seen.get(n) ?? [])].sort()).toEqual([0, 1])
    const hazards = readFileSync(join(ROOT, 'parity', 'HAZARDS.md'), 'utf8')
    for (const n of seen.keys()) expect(hazards).toContain(n)
  })

  test('configdef.json is the engine\'s ConfigDef', () => {
    const c = JSON.parse(readFileSync(join(dir, 'configdef.json'), 'utf8'))
    expect(c.format).toBe('sketch-orca-probe-configdef/1')
    expect(c.orcaVersion).toBe('2.4.2')
    expect(c.count).toBe(c.options.length)
    expect(c.count).toBeGreaterThan(500)
    const layer = c.options.find((o: { key: string }) => o.key === 'layer_height')
    expect(layer.default).toBe('0.2')
    expect(c.legacy.length).toBeGreaterThan(0)
    expect(c.printOptions).toContain('layer_height')
  })
})
