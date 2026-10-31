// The repository as a free program on its own: its licence and notices, the headers of the files
// that carry the licence, the package that runs its scripts, and a description of the engine as a
// program of its own. These run in a source bundle's engine/ folder too, which has no .git.

import { describe, expect, test } from 'bun:test'
import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { BUILT_WITH, keepEngineFile } from '../scripts/lib/bundle.ts'
import { sha256Bytes } from '../scripts/lib/files.ts'

const ROOT = join(import.meta.dir, '..')
const read = (rel: string) => readFileSync(join(ROOT, rel), 'utf8')

/** The files of the repository a source bundle carries: no .git, node_modules or local work folders. */
function repoFiles(dir = ROOT): string[] {
  const out: string[] = []
  for (const d of readdirSync(dir, { withFileTypes: true })) {
    const rel = relative(ROOT, join(dir, d.name)).split(sep).join('/')
    if (!keepEngineFile(rel)) continue
    if (d.isDirectory()) out.push(...repoFiles(join(dir, d.name)))
    else if (d.isFile()) out.push(rel)
  }
  return out.sort()
}

const HEADER = [
  '// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)',
  '// SPDX-License-Identifier: AGPL-3.0-only',
  '//',
  '// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:',
  '// you can redistribute it and/or modify it under the terms of the GNU Affero General Public',
  '// License, version 3, as published by the Free Software Foundation. It is distributed in the hope',
  '// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of',
  '// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License',
  '// (LICENSE) for more details.',
]

describe('the repository', () => {
  test("LICENSE is OrcaSlicer's LICENSE.txt, byte for byte (the GNU AGPL, version 3)", () => {
    const bytes = readFileSync(join(ROOT, 'LICENSE'))
    expect(sha256Bytes(bytes)).toBe('57c8ff33c9c0cfc3ef00e650a1cc910d7ee479a8bc509f6c9209a7c2a11399d6')
    expect(bytes.toString('utf8')).toStartWith('                    GNU AFFERO GENERAL PUBLIC LICENSE\n                       Version 3, 19 November 2007\n')
  })

  test('package.json: sketch-orca under AGPL-3.0-only, its three scripts, pinned tools', () => {
    const pkg = JSON.parse(read('package.json'))
    expect(pkg).toMatchObject({ name: 'sketch-orca', license: 'AGPL-3.0-only', private: true, type: 'module', packageManager: `bun@${BUILT_WITH.bun}` })
    expect(pkg.scripts).toEqual({ engine: 'bun scripts/orca.ts', test: 'bun test ./test', typecheck: 'tsc -p tsconfig.json' })
    for (const [name, version] of Object.entries(pkg.devDependencies as Record<string, string>)) expect({ name, version }).toEqual({ name, version: expect.stringMatching(/^\d+\.\d+\.\d+$/) })
    expect(read('cli/CMakeLists.txt')).toContain(`set(SKETCH_ORCA_VERSION "${pkg.version}")`)
    expect(read('bun.lock')).toContain('"name": "sketch-orca"')
  })

  test('.gitattributes keeps every file as committed', () => {
    expect(read('.gitattributes')).toBe('* -text\n')
  })

  test('NOTICE names the locked OrcaSlicer release and what it is based on; AUTHORS the holders', () => {
    const lock = JSON.parse(read('orca.lock.json'))
    const notice = read('NOTICE')
    for (const s of [lock.tag, lock.commit, 'OrcaSlicer contributors', 'Bambu Studio', 'Bambu Lab', 'PrusaSlicer', 'Prusa Research', 'Slic3r', 'Alessandro Ranellucci', 'ArcWelderLib', 'NanoSVG', 'libnoise', 'GMP'])
      expect({ s, found: notice.includes(s) }).toEqual({ s, found: true })
    expect(notice).toContain('GNU Affero General Public License,\nversion 3')
    expect(read('AUTHORS')).toContain('\nkevin-t2\n')
  })

  test('BUILDING.md names every tool version the engine is built with', () => {
    const building = read('BUILDING.md')
    for (const [tool, version] of Object.entries(BUILT_WITH)) expect({ tool, found: building.includes(version) }).toEqual({ tool, found: true })
  })

  test('every C++ file of cli/ starts with the copyright and licence header', () => {
    const sources = readdirSync(join(ROOT, 'cli')).filter((n) => /\.(cpp|hpp)$/.test(n))
    expect(sources.length).toBeGreaterThan(10)
    for (const n of sources) expect({ n, head: read(`cli/${n}`).split('\n').slice(0, HEADER.length) }).toEqual({ n, head: HEADER })
    const cmake = read('cli/CMakeLists.txt')
    expect(cmake).toStartWith('# Copyright (C) 2026 The sketch-orca authors (see AUTHORS)\n# SPDX-License-Identifier: AGPL-3.0-only\n')
  })

  test('the files that carry OrcaSlicer code say which, from which OrcaSlicer file', () => {
    const carries: Record<string, string[]> = {
      'cli/gui_symbols.cpp': ['src/slic3r/Utils/ColorSpaceConvert.cpp', 'deps_src/nanosvg'],
      'cli/run.cpp': ['src/OrcaSlicer.cpp', 'PartPlate.cpp', 'Plater.cpp'],
      'cli/presets.cpp': ['src/OrcaSlicer.cpp', 'Plater.cpp', 'BitmapCache'],
      'cli/CMakeLists.txt': ['src/libslic3r/PrintConfig.cpp', 'orcaslicer_copy_dlls'],
      'scripts/lib/smoke.ts': ['src/libslic3r/Slicing.cpp'],
    }
    for (const [file, from] of Object.entries(carries)) {
      const text = read(file)
      const at = text.indexOf('Carries OrcaSlicer code')
      expect({ file, at: at >= 0 }).toEqual({ file, at: true })
      const block = text.slice(at, at + 1200)
      for (const f of from) expect({ file, f, found: block.includes(f) }).toEqual({ file, f, found: true })
      expect({ file, dated: /20\d\d-\d\d-\d\d/.test(block) }).toEqual({ file, dated: true })
    }
  })

  test('the usage text gives the licence as version 3 and the engine as a separate program', () => {
    const main = read('cli/main.cpp')
    const usage = main.slice(main.indexOf('std::string usage()'), main.indexOf('int usage_error('))
    expect(usage).toContain('GNU Affero General Public License,\\n"\n            "version 3')
    expect(usage).not.toMatch(/or (at your option )?(any )?later/i)
    expect(usage).toContain('It is a separate\\n"')
  })

  test('nothing calls the engine a part of a client: it is a program of its own', () => {
    // Built from parts, so this file does not find itself.
    const needles = [
      ["Sketch's ", 'engine'],
      ['part of ', 'Sketch'],
      ['engine Sketch ', 'slices with'],
      ['Sketch engine ', 'patch'],
    ].map((p) => p.join(''))
    const found: string[] = []
    for (const f of repoFiles()) {
      if (f === 'LICENSE' || f.startsWith('licenses/') || f === 'bun.lock') continue
      const text = read(f)
      for (const n of needles) if (text.includes(n)) found.push(`${f}: ${n}`)
    }
    expect(found).toEqual([])
  })
})
