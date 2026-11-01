import { describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { isApiSet, isDynamicCrt, isMsvcRuntime, readPeDebugPaths, readPeImports, resolveRuntime, windowsRuntimeIo, type RuntimeIo } from '../scripts/lib/pe.ts'
import { privacyProblems } from '../scripts/lib/stage.ts'

/**
 * A minimal PE image: one section (.rdata, RVA 0x1000, file offset 0x400) holding the import
 * descriptors (RVA 0x1000), the delay-load descriptors (RVA 0x1100), the DLL names (RVA 0x1200)
 * and the debug directory (RVA 0x1380), whose CodeView records follow the section (file 0x800).
 */
function makePe(o: { is64: boolean; imports: string[]; delay?: string[]; delayAsVa?: boolean; debug?: { sig: 'RSDS' | 'NB10' | 'MISC'; path: string }[] }): Buffer {
  const debug = o.debug ?? []
  const buf = Buffer.alloc(0x800 + 0x100 * debug.length)
  buf.write('MZ', 0, 'latin1')
  buf.writeUInt32LE(0x80, 0x3c)
  buf.writeUInt32LE(0x00004550, 0x80)
  buf.writeUInt16LE(o.is64 ? 0x8664 : 0x14c, 0x84)
  buf.writeUInt16LE(1, 0x86)
  const optSize = o.is64 ? 240 : 224
  buf.writeUInt16LE(optSize, 0x94)
  const opt = 0x98
  buf.writeUInt16LE(o.is64 ? 0x20b : 0x10b, opt)
  const imageBase = o.is64 ? 0x140000000 : 0x400000
  if (o.is64) buf.writeBigUInt64LE(BigInt(imageBase), opt + 24)
  else buf.writeUInt32LE(imageBase, opt + 28)
  buf.writeUInt32LE(16, opt + (o.is64 ? 108 : 92))
  const dirs = opt + (o.is64 ? 112 : 96)
  const sec = opt + optSize
  buf.write('.rdata', sec, 'latin1')
  buf.writeUInt32LE(0x400, sec + 8)
  buf.writeUInt32LE(0x1000, sec + 12)
  buf.writeUInt32LE(0x400, sec + 16)
  buf.writeUInt32LE(0x400, sec + 20)
  const at = (rva: number) => rva - 0x1000 + 0x400
  let nameRva = 0x1200
  const putName = (s: string): number => {
    const r = nameRva
    buf.write(`${s}\0`, at(r), 'latin1')
    nameRva += s.length + 1
    return r
  }
  if (o.imports.length > 0) {
    buf.writeUInt32LE(0x1000, dirs + 8)
    buf.writeUInt32LE(20 * (o.imports.length + 1), dirs + 12)
    o.imports.forEach((n, i) => {
      const d = at(0x1000 + 20 * i)
      buf.writeUInt32LE(0x1300, d)
      buf.writeUInt32LE(putName(n), d + 12)
      buf.writeUInt32LE(0x1300, d + 16)
    })
  }
  const delay = o.delay ?? []
  if (delay.length > 0) {
    buf.writeUInt32LE(0x1100, dirs + 8 * 13)
    buf.writeUInt32LE(32 * (delay.length + 1), dirs + 8 * 13 + 4)
    delay.forEach((n, i) => {
      const d = at(0x1100 + 32 * i)
      const r = putName(n)
      buf.writeUInt32LE(o.delayAsVa ? 0 : 1, d)
      buf.writeUInt32LE(o.delayAsVa ? imageBase + r : r, d + 4)
    })
  }
  if (debug.length > 0) {
    buf.writeUInt32LE(0x1380, dirs + 8 * 6)
    buf.writeUInt32LE(28 * debug.length, dirs + 8 * 6 + 4)
    debug.forEach((r, i) => {
      const d = at(0x1380 + 28 * i)
      const raw = 0x800 + 0x100 * i
      const head = r.sig === 'RSDS' ? 24 : 16
      const size = head + Buffer.byteLength(r.path, 'utf8') + 1
      buf.write(r.sig === 'MISC' ? 'MISC' : r.sig, raw, 'latin1')
      buf.write(`${r.path}\0`, raw + head, 'utf8')
      buf.writeUInt32LE(r.sig === 'MISC' ? 4 : 2, d + 12)
      buf.writeUInt32LE(size, d + 16)
      buf.writeUInt32LE(raw, d + 24)
    })
  }
  return buf
}

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
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
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
