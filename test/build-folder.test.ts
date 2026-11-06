// The engine's build folder is started over when the dependency build it was configured against
// changed: CMake's cache would keep the libraries an earlier prefix had. Hermetic: a temporary
// folder only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { matchDepsBuild } from '../scripts/orca.ts'
import { readStamp } from '../scripts/lib/paths.ts'

let root: string
let w: { build: string; state: string }
const lines: string[] = []
const log = { line: (m: string) => void lines.push(m) }

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'orca-build-folder-'))
  w = { build: join(root, 'build'), state: join(root, 'state') }
  mkdirSync(w.state, { recursive: true })
  lines.length = 0
})
afterEach(() => rmSync(root, { recursive: true, force: true }))

function configured(): void {
  mkdirSync(w.build, { recursive: true })
  writeFileSync(join(w.build, 'CMakeCache.txt'), 'MPFR_LIBRARIES:FILEPATH=.../libmpfr-4.lib\n')
}

describe('the engine build folder and the dependency build', () => {
  test('a folder with no record of its dependency build is started over, and the record written', () => {
    configured()
    matchDepsBuild(w, 'deps-a', log)
    expect(existsSync(w.build)).toBe(false)
    expect(readStamp(w.state, 'orca-deps')?.inputs).toBe('deps-a')
    expect(lines[0]).toContain('configured against another dependency build')
  })

  test('the same dependency build keeps the folder; another one starts it over', () => {
    matchDepsBuild(w, 'deps-a', log)
    configured()
    matchDepsBuild(w, 'deps-a', log)
    expect(existsSync(join(w.build, 'CMakeCache.txt'))).toBe(true)
    matchDepsBuild(w, 'deps-b', log)
    expect(existsSync(w.build)).toBe(false)
    expect(readStamp(w.state, 'orca-deps')?.inputs).toBe('deps-b')
  })

  test('no folder yet: nothing to remove, the record is written', () => {
    matchDepsBuild(w, 'deps-a', log)
    expect(lines).toEqual([])
    expect(readStamp(w.state, 'orca-deps')?.inputs).toBe('deps-a')
  })
})
