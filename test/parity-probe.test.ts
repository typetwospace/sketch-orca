// The probe program's harness (scripts/lib/parity/probe.ts): the CRT maths functions picked out of
// a linker map, the engine's compile and link lines read from a build.ninja and turned into the
// probe's response files, its configure line (Ninja, no Fortran), running it and comparing two
// runs; and the committed probe files themselves (parity/probes/): SOP1 files that start with the
// probe's HEAD and PRUN, a configdef.json, all under the size limit. Hermetic: temporary folders,
// a fake probe process and the committed files only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parseHead, readSop1, Sop1Reader } from '../scripts/lib/parity/neutral.ts'
import {
  absoluteIn,
  committedSizeProblem,
  CRT_MATHS_NAMES,
  crtMathsSymbols,
  isDroppedCompileFlag,
  ninjaBuildVars,
  ninjaUnescape,
  PROBE_COMMITTED_MAX_BYTES,
  PROBE_FILES,
  probeCommittedDir,
  probeCompileArgs,
  probeConfigureArgs,
  probeDifferences,
  probeLinkArgs,
  probeSourceDir,
  rspText,
  runProbe,
  splitArgs,
} from '../scripts/lib/parity/probe.ts'
import type { RunProcess } from '../scripts/lib/parity/same.ts'

const ROOT = join(import.meta.dir, '..')

let work: string
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-probe-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

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

  test('the probe is configured with Ninja and cl, the static runtime, no Fortran and Eigen\'s tests off', () => {
    const args = probeConfigureArgs({ ninja: 'C:\\t\\ninja.exe', sourceDir: 'C:\\r\\parity\\oracle\\probe', buildDir: 'C:\\w\\oracle\\probe-build', compileRsp: 'C:\\w\\c.rsp', linkRsp: 'C:\\w\\l.rsp' })
    expect(args.slice(0, 6)).toEqual(['-S', 'C:/r/parity/oracle/probe', '-B', 'C:/w/oracle/probe-build', '-G', 'Ninja'])
    for (const a of ['-DCMAKE_MAKE_PROGRAM=C:/t/ninja.exe', '-DCMAKE_CXX_COMPILER=cl', '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded', '-DCMAKE_Fortran_COMPILER:STRING=NOTFOUND', '-DEIGEN_BUILD_TESTING:BOOL=OFF', '-DPROBE_COMPILE_RSP=C:/w/c.rsp', '-DPROBE_LINK_RSP=C:/w/l.rsp']) expect(args).toContain(a)
    expect(args.join(' ')).not.toMatch(/Visual Studio/)
  })
})

describe('running the probe and comparing runs', () => {
  const fake = (write: (committed: string, full: string) => void, code = 0): { run: RunProcess; calls: string[][] } => {
    const calls: string[][] = []
    const run: RunProcess = async (exe, args, o) => {
      calls.push([exe, ...args, `timeout=${o.timeoutMs}`, `cwd=${o.cwd}`])
      write(args[1]!, args[3]!)
      return { code, timedOut: false, stdout: Buffer.alloc(0), stderr: Buffer.from(code === 0 ? '' : 'sketch-orca-probe: crt: the map names a maths function the probe does not know: ldexpf') }
    }
    return { run, calls }
  }
  const writeAll = (committed: string, full: string) => {
    for (const d of [committed, full]) for (const f of PROBE_FILES) writeFileSync(join(d, f), `${f} of ${d === committed ? 'committed' : 'full'}`)
  }

  test('runs the probe hidden into two emptied folders with the map\'s symbols and its libraries on PATH', async () => {
    const committed = join(work, 'committed')
    const full = join(work, 'full')
    mkdirSync(committed)
    writeFileSync(join(committed, 'stale.sop1'), 'old')
    const { run, calls } = fake(writeAll)
    await runProbe({ exe: 'C:\\p\\sketch-orca-probe.exe', crtSymbolsFile: 'C:\\p\\crt-symbols.txt', committed, full, libraryDirs: ['C:\\o\\bin'], run, timeoutMs: 1000 })
    expect(calls).toEqual([['C:\\p\\sketch-orca-probe.exe', '--committed', committed, '--full', full, '--crt-symbols', 'C:\\p\\crt-symbols.txt', 'timeout=1000', `cwd=${full}`]])
    expect(existsSync(join(committed, 'stale.sop1'))).toBe(false)
    for (const f of PROBE_FILES) expect(readFileSync(join(full, f), 'utf8')).toBe(`${f} of full`)
  })

  test('a failing probe, or one that leaves a file out, is an error', async () => {
    const committed = join(work, 'c')
    const full = join(work, 'f')
    await expect(runProbe({ exe: 'p.exe', crtSymbolsFile: 's', committed, full, libraryDirs: [], run: fake(writeAll, 3).run })).rejects.toThrow('ldexpf')
    const missing = (c: string, f: string) => {
      writeAll(c, f)
      rmSync(join(f, 'tbb.sop1'))
    }
    await expect(runProbe({ exe: 'p.exe', crtSymbolsFile: 's', committed, full, libraryDirs: [], run: fake(missing).run })).rejects.toThrow('tbb.sop1')
  })

  test('two runs compare file by file, and the committed set has a size limit', () => {
    const a = join(work, 'a')
    const b = join(work, 'b')
    for (const d of [a, b, join(work, 'x'), join(work, 'y')]) mkdirSync(d)
    writeAll(a, join(work, 'x'))
    expect(probeDifferences(a, b).length).toBe(PROBE_FILES.length)
    writeAll(b, join(work, 'y'))
    expect(probeDifferences(a, b)).toEqual([])
    writeFileSync(join(b, 'fmt.sop1'), 'other')
    writeFileSync(join(b, 'extra.txt'), '')
    expect(probeDifferences(a, b)).toEqual([`extra.txt: only in ${b}`, 'fmt.sop1: differs'])
    expect(committedSizeProblem(a)).toBeUndefined()
    expect(committedSizeProblem(a, 10)).toContain('over 10')
  })
})

describe('the committed probe files', () => {
  const dir = probeCommittedDir(ROOT)

  test('are all there, under the size limit together', () => {
    let total = 0
    for (const f of PROBE_FILES) {
      expect(existsSync(join(dir, f))).toBe(true)
      total += statSync(join(dir, f)).size
    }
    expect(total).toBeLessThan(PROBE_COMMITTED_MAX_BYTES)
    expect(committedSizeProblem(dir)).toBeUndefined()
  })

  test('each SOP1 file reads whole, starts with the probe\'s HEAD and PRUN for the committed set, and has the records SOP1.md lists', () => {
    const kinds: Record<string, string[]> = {
      'crt.sop1': ['MATH', 'MATS', 'CAST', 'RAND'],
      'fmt.sop1': ['PRTF', 'TOCH', 'OSTR', 'TOST', 'PARS'],
      'stl.sop1': ['SORT', 'VALS', 'HASH', 'UMAP', 'USET', 'RNGS', 'SSEQ', 'SHUF'],
      'tbb.sop1': ['TBBA', 'TBBF', 'TBBR', 'TBBO'],
    }
    const sop1 = readFileSync(join(ROOT, 'parity', 'SOP1.md'), 'utf8')
    for (const [file, want] of Object.entries(kinds)) {
      const { records } = readSop1(readFileSync(join(dir, file)))
      expect(records[0]!.kind).toBe('HEAD')
      expect(parseHead(records[0]!.payload)).toEqual({ source: 'probe', name: file.replace('.sop1', ''), object: -1n, occurrence: 1 })
      expect(records[1]!.kind).toBe('PRUN')
      const prun = new Sop1Reader(records[1]!.payload)
      const msc = prun.u32()
      expect(msc).toBeGreaterThanOrEqual(194200000)
      expect(msc).toBeLessThan(194300000)
      prun.u32()
      prun.i32()
      expect(prun.str()).toBe('committed')
      expect([...new Set(records.slice(2).map((r) => r.kind))].sort()).toEqual([...want].sort())
      for (const k of want) expect(sop1).toContain(`### ${k}`)
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

  test('the probe program\'s sources are in the repository with its CMakeLists.txt', () => {
    for (const f of ['CMakeLists.txt', 'probe.hpp', 'main.cpp', 'crt.cpp', 'fmt.cpp', 'stl.cpp', 'tbb.cpp', 'configdef.cpp']) expect(existsSync(join(probeSourceDir(ROOT), f))).toBe(true)
    const cmake = readFileSync(join(probeSourceDir(ROOT), 'CMakeLists.txt'), 'utf8')
    expect(cmake).toContain('LANGUAGES CXX')
    expect(cmake).not.toMatch(/Fortran\)|enable_language/)
  })
})
