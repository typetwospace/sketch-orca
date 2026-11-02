// privacy-fixtures: /home/builder /Users/李 /home/李 D:/build-cache/orca
import { describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { isApiSet, isDynamicCrt, isMsvcRuntime, readPeDebugPaths, readPeImports, resolveRuntime, windowsRuntimeIo, type RuntimeIo } from '../scripts/lib/pe.ts'
import { ALLOWED_BUILD_PATHS, isBinaryProgram, privacyProblems, recordedBuildPaths } from '../scripts/lib/stage.ts'
import { makePe } from './support/pe-writer.ts'

describe('readPeImports', () => {
  test('a 64-bit image: import and delay-import tables', () => {
    const pe = makePe({ is64: true, imports: ['KERNEL32.dll', 'TKernel.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'], delay: ['libgmp-10.dll'] })
    expect(readPeImports(pe)).toEqual({
      machine: 0x8664,
      is64: true,
      imports: ['KERNEL32.dll', 'TKernel.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'],
      delayImports: ['libgmp-10.dll'],
    })
  })

  test('a 32-bit image whose delay-load names are virtual addresses', () => {
    const pe = makePe({ is64: false, imports: ['USER32.dll'], delay: ['old.dll', 'older.dll'], delayAsVa: true })
    expect(readPeImports(pe)).toEqual({ machine: 0x14c, is64: false, imports: ['USER32.dll'], delayImports: ['old.dll', 'older.dll'] })
  })

  test('an image without imports', () => {
    expect(readPeImports(makePe({ is64: true, imports: [] }))).toEqual({ machine: 0x8664, is64: true, imports: [], delayImports: [] })
  })

  test('refuses what is not a PE image', () => {
    expect(() => readPeImports(Buffer.from('#!/bin/sh\n'.repeat(10)))).toThrow(/no MZ header/)
    const noPe = makePe({ is64: true, imports: [] })
    noPe.writeUInt32LE(0x12345678, 0x80)
    expect(() => readPeImports(noPe)).toThrow(/no PE signature/)
    const badMagic = makePe({ is64: true, imports: [] })
    badMagic.writeUInt16LE(0x107, 0x98)
    expect(() => readPeImports(badMagic)).toThrow(/optional header magic 0x107/)
  })

  test('refuses an import table outside every section', () => {
    const pe = makePe({ is64: true, imports: ['A.dll'] })
    pe.writeUInt32LE(0x9000, 0x98 + 112 + 8)
    expect(() => readPeImports(pe)).toThrow(/0x9000 is in no section/)
  })

  test.skipIf(process.platform !== 'win32')('reads a real Windows program', () => {
    const t = readPeImports(readFileSync(join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'cmd.exe')))
    expect(t.imports.length).toBeGreaterThan(0)
    for (const n of [...t.imports, ...t.delayImports]) expect(n.toLowerCase().endsWith('.dll')).toBe(true)
  })
})

describe('readPeDebugPaths', () => {
  test('the program database paths of the CodeView records, RSDS and NB10; other debug records are not read', () => {
    const pe = makePe({
      is64: true,
      imports: ['KERNEL32.dll'],
      debug: [
        { sig: 'RSDS', path: 'D:\\build-cache\\orca\\engine.pdb' },
        { sig: 'MISC', path: 'C:\\not\\a\\pdb' },
        { sig: 'NB10', path: 'old.pdb' },
      ],
    })
    expect(readPeDebugPaths(pe)).toEqual(['D:\\build-cache\\orca\\engine.pdb', 'old.pdb'])
    expect(readPeDebugPaths(makePe({ is64: false, imports: [], debug: [{ sig: 'RSDS', path: 'C:/Users/李/build/engine.pdb' }] }))).toEqual(['C:/Users/李/build/engine.pdb'])
    expect(readPeDebugPaths(makePe({ is64: true, imports: ['KERNEL32.dll'] }))).toEqual([])
  })

  test.skipIf(process.platform !== 'win32')('reads a real Windows program', () => {
    const exe = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'cmd.exe')
    for (const p of readPeDebugPaths(readFileSync(exe))) expect(p).toMatch(/\.pdb$/i)
  })

  test('privacyProblems: a staged program or library that records its program database with a folder is refused, any folder', () => {
    const dir = mkdtempSync(join(tmpdir(), 'orca-pe-'))
    try {
      const put = (rel: string, bytes: Buffer) => {
        mkdirSync(join(dir, 'bin'), { recursive: true })
        writeFileSync(join(dir, rel), bytes)
      }
      put('bin/a.exe', makePe({ is64: true, imports: [], debug: [{ sig: 'RSDS', path: 'D:/build-cache/orca/engine.pdb' }] }))
      put('bin/b.dll', makePe({ is64: true, imports: [], debug: [{ sig: 'RSDS', path: '\\\\server\\share\\b.pdb' }] }))
      put('bin/c.dll', makePe({ is64: true, imports: [], debug: [{ sig: 'RSDS', path: 'c.pdb' }] }))
      put('bin/d.dll', Buffer.from('not a PE file'))
      put('bin/e.exe', makePe({ is64: true, imports: [], debug: [{ sig: 'NB10', path: '/home/builder/e.pdb' }] }))
      expect(privacyProblems(dir, ['bin/a.exe', 'bin/b.dll', 'bin/c.dll', 'bin/d.dll', 'bin/e.exe'], [])).toEqual([
        'bin/a.exe (program database D:/build-cache/orca/engine.pdb)',
        'bin/b.dll (program database \\\\server\\share\\b.pdb)',
        'bin/e.exe (program database /home/builder/e.pdb)',
      ])
      // And the needles, in the same files.
      expect(privacyProblems(dir, ['bin/a.exe', 'bin/c.dll'], ['d:/build-cache/orca'])).toEqual(['bin/a.exe (d:/build-cache/orca)', 'bin/a.exe (program database D:/build-cache/orca/engine.pdb)'])
      // A prior builder's folder outside any CodeView record (__FILE__, an assertion text): in a program or library only.
      put('bin/f.dll', Buffer.concat([makePe({ is64: true, imports: [] }), Buffer.from('assert failed: E:\\agent\\_work\\7\\s\\deps\\x.cpp\0', 'latin1'), Buffer.alloc(3), Buffer.from('F:/ci/build/y.h\0', 'utf16le')]))
      put('bin/notes.txt', Buffer.from('see E:\\agent\\_work\\7\\s\\deps\\x.cpp'))
      expect(privacyProblems(dir, ['bin/f.dll', 'bin/notes.txt'], [])).toEqual(['bin/f.dll (build path E:\\agent\\_work\\7\\s\\deps\\x.cpp)', 'bin/f.dll (build path F:/ci/build/y.h)'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('recordedBuildPaths: folders of a machine, as narrow or UTF-16LE text at either alignment; not URLs, short or broken runs, or allowed folders', () => {
    const at = (s: string, enc: BufferEncoding = 'latin1', pad = 0) => recordedBuildPaths(Buffer.concat([Buffer.alloc(pad, 0xff), Buffer.from(s, enc)]))
    expect(at('x\0D:\\agent\\_work\\1\\s\\a.cpp\0y')).toEqual(['D:\\agent\\_work\\1\\s\\a.cpp'])
    expect(at('C:\\Users\\John Smith\\w\\x.h')).toEqual(['C:\\Users\\John Smith\\w\\x.h'])
    expect(at('\\\\buildsrv\\share\\x')).toEqual(['\\\\buildsrv\\share'])
    expect(at('/home/ci/build/x.c and /Users/ci/x and /root/x')).toEqual(['/Users/ci', '/home/ci', '/root/'])
    for (const pad of [0, 1]) expect(at('G:/ci/work/x.cpp', 'utf16le', pad)).toEqual(['G:/ci/work/x.cpp'])
    // URLs, a lone drive, noise that only starts like a path, the GMP compiler's own tree.
    expect(at('http://www.w3.org/2000/svg https://example.org/a/b')).toEqual([])
    expect(at('C:\\x and R:\\Sg|p5rL and J:\\0J(^&J and /W64_4.4_160271-src/build-gcc/x86_64')).toEqual([])
    expect(at('/usr/lib/x86_64-linux-gnu/libm.so.6 /homework/x')).toEqual([])
    expect(recordedBuildPaths(Buffer.from('D:\\agent\\_work\\1\\s\\a.cpp'), [{ path: 'd:/agent/_work' }])).toEqual([])
    // Names in any script: UTF-8 in narrow text, a letter's code unit in UTF-16LE; and a plus sign.
    expect(at('x\0D:/工具/private/engine.cpp\0y', 'utf8')).toEqual(['D:/工具/private/engine.cpp'])
    expect(at('C:\\Users\\Zoë Ångström\\dev\\x.cpp\0', 'utf8')).toEqual(['C:\\Users\\Zoë Ångström\\dev\\x.cpp'])
    for (const pad of [0, 1]) expect(at('\0E:\\сборка\\orca\\x.cpp\0', 'utf16le', pad)).toEqual(['E:\\сборка\\orca\\x.cpp'])
    expect(at('/home/李/w/x.c', 'utf8')).toEqual(['/home/李'])
    expect(at('C:\\src\\notepad++\\x.cpp')).toEqual(['C:\\src\\notepad++\\x.cpp'])
    // Another absolute POSIX path to a source file (__FILE__ on Linux or macOS), not the system's or a URL's.
    expect(at('assert at /opt/vendor/build/engine.cpp:42\0')).toEqual(['/opt/vendor/build/engine.cpp'])
    expect(at('\0/builds/runner/x/src/a.hpp\0', 'utf16le')).toEqual(['/builds/runner/x/src/a.hpp'])
    expect(at('/usr/include/c++/12/bits/stl_vector.h /usr/lib/gcc/x.c /opt/homebrew/include/x.h /Library/Developer/x/y.h')).toEqual([])
    expect(at('https://github.com/google/draco/blob/main/src/draco/core/x.cc file:///opt/x/y.cpp text/x-c a/b/c.cpp')).toEqual([])
    // Programs and libraries by their first bytes.
    expect(isBinaryProgram(makePe({ is64: true, imports: [] }))).toBe(true)
    expect(isBinaryProgram(Buffer.from('!<arch>\n/ 0', 'latin1'))).toBe(true)
    expect(isBinaryProgram(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2]))).toBe(true)
    expect(isBinaryProgram(Buffer.from([0xcf, 0xfa, 0xed, 0xfe]))).toBe(true)
    expect(isBinaryProgram(Buffer.from('{"json": true}'))).toBe(false)
    expect(ALLOWED_BUILD_PATHS).toEqual([])
  })
})

describe('runtime closure', () => {
  test('API sets and the Visual C++ runtime are recognised by name', () => {
    expect(isApiSet('api-ms-win-crt-runtime-l1-1-0.dll')).toBe(true)
    expect(isApiSet('ext-ms-win-ntuser-window-l1-1-0.dll')).toBe(true)
    expect(isApiSet('KERNEL32.dll')).toBe(false)
    for (const n of ['VCRUNTIME140.dll', 'vcruntime140_1.dll', 'MSVCP140.dll', 'msvcp140_atomic_wait.dll', 'concrt140.dll', 'vcomp140.dll']) expect(isMsvcRuntime(n)).toBe(true)
    for (const n of ['msvcrt.dll', 'ucrtbase.dll', 'TKernel.dll', 'kernel32.dll']) expect(isMsvcRuntime(n)).toBe(false)
  })

  test("the dynamic C runtime: the Visual C++ runtime and the UCRT, not Windows' own msvcrt.dll", () => {
    for (const n of ['VCRUNTIME140.dll', 'vcruntime140_1.dll', 'MSVCP140.dll', 'ucrtbase.dll', 'UCRTBASE.DLL', 'api-ms-win-crt-runtime-l1-1-0.dll', 'API-MS-WIN-CRT-HEAP-L1-1-0.DLL']) expect({ n, crt: isDynamicCrt(n) }).toEqual({ n, crt: true })
    for (const n of ['msvcrt.dll', 'KERNEL32.dll', 'api-ms-win-core-synch-l1-2-0.dll', 'libgmp-10.dll', 'TKernel.dll']) expect({ n, crt: isDynamicCrt(n) }).toEqual({ n, crt: false })
  })

  test('the debug builds and the older Visual C++ runtimes are the dynamic C runtime too', () => {
    const debugAndLegacy = [
      'vcruntime140d.dll',
      'VCRUNTIME140_1D.dll',
      'vcruntime140_threads.dll',
      'msvcp140d.dll',
      'msvcp140d_atomic_wait.dll',
      'msvcp140_1d.dll',
      'ucrtbased.dll',
      'msvcr120.dll',
      'MSVCR100D.dll',
      'msvcp120.dll',
      'concrt140d.dll',
      'vcomp140d.dll',
      // The Visual C++ 6 runtime's debug build: Windows carries msvcrt.dll, not this.
      'msvcrtd.dll',
      'MSVCRTD.DLL',
    ]
    for (const n of debugAndLegacy) expect({ n, crt: isDynamicCrt(n) }).toEqual({ n, crt: true })
    // Windows' own C runtime and names that only start like one are not.
    for (const n of ['msvcrt.dll', 'MSVCRT.DLL', 'msvcrtd.dll.mui', 'ucrtbase_clr0400.dll', 'vcruntime.dll']) expect({ n, crt: isDynamicCrt(n) }).toEqual({ n, crt: false })
  })

  test('a dependency on a debug or older runtime is named in crt, and never taken from Windows', () => {
    const files: Record<string, string[]> = {
      'build/engine.exe': ['KERNEL32.dll', 'TKernel.dll'],
      'build/TKernel.dll': ['KERNEL32.dll', 'msvcr120.dll', 'ucrtbased.dll', 'vcruntime140d.dll'],
    }
    const io: RuntimeIo = {
      importsOf: (p) => files[p] ?? [],
      find: (dir, name) => (`${dir}/${name}` in files ? `${dir}/${name}` : undefined),
      // An installed runtime in System32 does not make it the system's.
      isSystem: (n) => ['kernel32.dll', 'msvcr120.dll', 'vcruntime140d.dll'].includes(n.toLowerCase()),
    }
    const c = resolveRuntime('build/engine.exe', ['build'], io)
    expect(c.crt).toEqual([
      { name: 'msvcr120.dll', neededBy: 'TKernel.dll' },
      { name: 'ucrtbased.dll', neededBy: 'TKernel.dll' },
      { name: 'vcruntime140d.dll', neededBy: 'TKernel.dll' },
    ])
    expect(c.system).toEqual(['KERNEL32.dll'])
    expect(c.missing.map((m) => m.name)).toEqual(['msvcr120.dll', 'ucrtbased.dll', 'vcruntime140d.dll'])
  })

  test('walks imports through the search folders; the C++ runtime is never taken from Windows', () => {
    const files: Record<string, string[]> = {
      'build/engine.exe': ['TKernel.dll', 'KERNEL32.dll', 'api-ms-win-crt-heap-l1-1-0.dll', 'VCRUNTIME140.dll'],
      'build/TKernel.dll': ['tbb12.dll', 'vcruntime140.dll', 'ADVAPI32.dll'],
      'deps/bin/tbb12.dll': ['KERNEL32.dll', 'missing.dll'],
    }
    const system = new Set(['kernel32.dll', 'advapi32.dll', 'vcruntime140.dll'])
    const io: RuntimeIo = {
      importsOf: (p) => files[p] ?? [],
      find: (dir, name) => {
        const p = `${dir}/${name}`
        return p in files ? p : undefined
      },
      isSystem: (n) => system.has(n.toLowerCase()),
    }
    const c = resolveRuntime('build/engine.exe', ['build', 'deps/bin'], io)
    expect(c.copy).toEqual([
      { name: 'TKernel.dll', path: 'build/TKernel.dll', neededBy: 'build/engine.exe' },
      { name: 'tbb12.dll', path: 'deps/bin/tbb12.dll', neededBy: 'TKernel.dll' },
    ])
    expect(c.system).toEqual(['ADVAPI32.dll', 'api-ms-win-crt-heap-l1-1-0.dll', 'KERNEL32.dll'])
    expect(c.missing).toEqual([
      { name: 'VCRUNTIME140.dll', neededBy: 'build/engine.exe' },
      { name: 'missing.dll', neededBy: 'tbb12.dll' },
    ])
    // Every file that imports the dynamic C runtime, each time: TKernel's vcruntime140.dll too.
    expect(c.crt).toEqual([
      { name: 'api-ms-win-crt-heap-l1-1-0.dll', neededBy: 'build/engine.exe' },
      { name: 'VCRUNTIME140.dll', neededBy: 'build/engine.exe' },
      { name: 'vcruntime140.dll', neededBy: 'TKernel.dll' },
    ])
  })

  test('a library found in a search folder is copied even when Windows has one too', () => {
    const files: Record<string, string[]> = { 'e.exe': ['zlib1.dll', 'VCRUNTIME140.dll'], 'redist/VCRUNTIME140.dll': [], 'redist/zlib1.dll': [] }
    const io: RuntimeIo = {
      importsOf: (p) => files[p] ?? [],
      find: (dir, name) => (`${dir}/${name}` in files ? `${dir}/${name}` : undefined),
      isSystem: () => true,
    }
    const c = resolveRuntime('e.exe', ['redist'], io)
    expect(c.copy.map((x) => x.name)).toEqual(['zlib1.dll', 'VCRUNTIME140.dll'])
    expect(c.missing).toEqual([])
  })

  test.skipIf(process.platform !== 'win32')('windowsRuntimeIo knows what System32 holds', () => {
    const io = windowsRuntimeIo()
    expect(io.isSystem('kernel32.dll')).toBe(true)
    expect(io.isSystem('sketch-orca-does-not-exist.dll')).toBe(false)
    const sys32 = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')
    expect(io.find(sys32, 'kernel32.dll')).toBe(join(sys32, 'kernel32.dll'))
    expect(io.importsOf(join(sys32, 'cmd.exe')).length).toBeGreaterThan(0)
  })
})
