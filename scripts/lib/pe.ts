// Which libraries a program loads at start-up: the import and delay-import tables of a Windows
// PE file, read with plain buffer reads, and the closure over them (what `stage` must copy next to
// the engine, what Windows itself provides, and what cannot be found).

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

export interface PeImports {
  machine: number
  is64: boolean
  /** DLL names from the import table, as written. */
  imports: string[]
  /** DLL names from the delay-load import table. */
  delayImports: string[]
}

interface Section {
  va: number
  vsize: number
  raw: number
  rawSize: number
}

export function readPeImports(buf: Buffer): PeImports {
  if (buf.length < 0x40 || buf.toString('latin1', 0, 2) !== 'MZ') throw new Error('not a PE file (no MZ header)')
  const pe = buf.readUInt32LE(0x3c)
  if (pe + 24 > buf.length || buf.readUInt32LE(pe) !== 0x00004550) throw new Error('not a PE file (no PE signature)')
  const machine = buf.readUInt16LE(pe + 4)
  const nSections = buf.readUInt16LE(pe + 6)
  const optSize = buf.readUInt16LE(pe + 20)
  const opt = pe + 24
  const magic = buf.readUInt16LE(opt)
  if (magic !== 0x10b && magic !== 0x20b) throw new Error(`unknown PE optional header magic 0x${magic.toString(16)}`)
  const is64 = magic === 0x20b
  const nDirs = buf.readUInt32LE(opt + (is64 ? 108 : 92))
  const dirs = opt + (is64 ? 112 : 96)
  const dir = (i: number) => (i < nDirs ? { rva: buf.readUInt32LE(dirs + 8 * i), size: buf.readUInt32LE(dirs + 8 * i + 4) } : { rva: 0, size: 0 })
  const sections: Section[] = []
  const secTable = opt + optSize
  for (let i = 0; i < nSections; i++) {
    const s = secTable + 40 * i
    sections.push({ vsize: buf.readUInt32LE(s + 8), va: buf.readUInt32LE(s + 12), rawSize: buf.readUInt32LE(s + 16), raw: buf.readUInt32LE(s + 20) })
  }
  const offset = (rva: number): number => {
    for (const s of sections) {
      if (rva >= s.va && rva < s.va + Math.max(s.vsize, s.rawSize)) {
        const off = rva - s.va + s.raw
        if (off >= buf.length) break
        return off
      }
    }
    throw new Error(`PE: address 0x${rva.toString(16)} is in no section`)
  }
  const cstring = (rva: number): string => {
    const start = offset(rva)
    const end = buf.indexOf(0, start)
    return buf.toString('latin1', start, end < 0 ? buf.length : end)
  }
  const imports: string[] = []
  const imp = dir(1)
  if (imp.rva !== 0) {
    for (let p = offset(imp.rva); ; p += 20) {
      const lookup = buf.readUInt32LE(p)
      const name = buf.readUInt32LE(p + 12)
      const thunk = buf.readUInt32LE(p + 16)
      if (lookup === 0 && name === 0 && thunk === 0) break
      imports.push(cstring(name))
    }
  }
  const delayImports: string[] = []
  const delay = dir(13)
  if (delay.rva !== 0) {
    const imageBase = is64 ? Number(buf.readBigUInt64LE(opt + 24)) : buf.readUInt32LE(opt + 28)
    for (let p = offset(delay.rva); ; p += 32) {
      const attributes = buf.readUInt32LE(p)
      const name = buf.readUInt32LE(p + 4)
      if (attributes === 0 && name === 0) break
      // Bit 0 set: the fields are RVAs (every current linker); clear: virtual addresses.
      delayImports.push(cstring(attributes & 1 ? name : name - imageBase))
    }
  }
  return { machine, is64, imports, delayImports }
}

/** DLLs that are part of Windows, or API sets it maps onto them. */
export function isApiSet(name: string): boolean {
  return /^(api|ext)-ms-/i.test(name)
}

/** The Visual C++ runtime: always shipped with the engine (from Visual Studio's redist folder), never taken from the system. */
export function isMsvcRuntime(name: string): boolean {
  return /^(vcruntime\d+(_\d+)?|msvcp\d+(_\w+)?|concrt\d+|vcomp\d+|vccorlib\d+|vcamp\d+|mfc\d+\w*)\.dll$/i.test(name)
}

export interface RuntimeIo {
  /** The DLLs a file imports (import and delay-import tables). */
  importsOf(path: string): string[]
  /** The path of `name` in `dir`, or undefined. */
  find(dir: string, name: string): string | undefined
  /** Whether Windows provides `name` itself (System32). */
  isSystem(name: string): boolean
}

export interface RuntimeClosure {
  /** Libraries to ship next to the engine, with the file that needs each. */
  copy: { name: string; path: string; neededBy: string }[]
  /** Libraries Windows provides. */
  system: string[]
  /** Libraries found nowhere. */
  missing: { name: string; neededBy: string }[]
}

/**
 * Walks the imports from `exe`: each library is looked up in `searchDirs` in order (the first
 * holds the engine), and its own imports are walked in turn. A library that is not found there
 * is the system's when it is an API set or Windows has it, except the Visual C++ runtime, which
 * must be found in `searchDirs`.
 */
export function resolveRuntime(exe: string, searchDirs: string[], io: RuntimeIo): RuntimeClosure {
  const out: RuntimeClosure = { copy: [], system: [], missing: [] }
  const done = new Set<string>()
  const queue: { path: string; label: string }[] = [{ path: exe, label: exe }]
  while (queue.length > 0) {
    const { path, label } = queue.shift()!
    for (const name of io.importsOf(path)) {
      const key = name.toLowerCase()
      if (done.has(key)) continue
      done.add(key)
      if (isApiSet(name)) {
        out.system.push(name)
        continue
      }
      let found: string | undefined
      for (const d of searchDirs) {
        found = io.find(d, name)
        if (found) break
      }
      if (found) {
        out.copy.push({ name, path: found, neededBy: label })
        queue.push({ path: found, label: name })
      } else if (!isMsvcRuntime(name) && io.isSystem(name)) {
        out.system.push(name)
      } else {
        out.missing.push({ name, neededBy: label })
      }
    }
  }
  out.system.sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))
  return out
}

/** RuntimeIo on the real file system: PE tables from disk, System32 for the system. */
export function windowsRuntimeIo(systemRoot = process.env.SystemRoot ?? 'C:\\Windows'): RuntimeIo {
  const system32 = join(systemRoot, 'System32')
  return {
    importsOf: (path) => {
      const t = readPeImports(readFileSync(path))
      return [...t.imports, ...t.delayImports]
    },
    find: (dir, name) => {
      const p = join(dir, name)
      return existsSync(p) ? p : undefined
    },
    isSystem: (name) => existsSync(join(system32, name)),
  }
}
