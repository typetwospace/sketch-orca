import { describe, expect, test } from 'bun:test'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parsePinnedSources } from '../scripts/lib/bundle.ts'
import { listPatches, patchedPaths, planPatches, repoUrl, type PatchInfo } from '../scripts/lib/checkout.ts'
import { LOCK_FORMAT } from '../scripts/lib/lock.ts'
import { sha256Text } from '../scripts/lib/paths.ts'

const COMMIT = '8500fcdccaa10b5099ac20d252af3a7c560046f1'
const p = (name: string, sha256 = sha256Text(name)): PatchInfo => ({ name, path: `/patches/${name}`, sha256 })

describe('checkout', () => {
  test('listPatches: *.patch files in name order with their hashes', () => {
    const dir = mkdtempSync(join(tmpdir(), 'orca-patches-'))
    try {
      writeFileSync(join(dir, '0002-b.patch'), 'two\n')
      writeFileSync(join(dir, '0001-a.patch'), 'one\n')
      writeFileSync(join(dir, 'README.txt'), 'not a patch\n')
      expect(listPatches(dir)).toEqual([
        { name: '0001-a.patch', path: join(dir, '0001-a.patch'), sha256: sha256Text('one\n') },
        { name: '0002-b.patch', path: join(dir, '0002-b.patch'), sha256: sha256Text('two\n') },
      ])
      expect(listPatches(join(dir, 'missing'))).toEqual([])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('the checked-in patches change deps/ (0001, 0005, 0006), the CMake files (0002, 0003) and Print.hpp (0004)', () => {
    const patches = listPatches(join(import.meta.dir, '..', 'patches'))
    expect(patches.map((x) => x.name)).toEqual([
      '0001-deps-without-gui.patch',
      '0002-root-cmake-without-gui-packages.patch',
      '0003-src-cmake-sketch-orca-front-end.patch',
      '0004-libslic3r-unmade-wipe-tower.patch',
      '0005-deps-msvc-static-runtime.patch',
      '0006-deps-openssl-3.patch',
    ])
    expect(patchedPaths(readFileSync(patches[0]!.path, 'utf8'))).toEqual(['deps/CMakeLists.txt', 'deps/OpenCV/OpenCV.cmake'])
    expect(patchedPaths(readFileSync(patches[1]!.path, 'utf8'))).toEqual(['CMakeLists.txt'])
    expect(patchedPaths(readFileSync(patches[2]!.path, 'utf8'))).toEqual(['src/CMakeLists.txt'])
    expect(patchedPaths(readFileSync(patches[3]!.path, 'utf8'))).toEqual(['src/libslic3r/Print.hpp'])
    expect(patchedPaths(readFileSync(patches[4]!.path, 'utf8'))).toEqual(['deps/CMakeLists.txt', 'deps/OpenCV/OpenCV.cmake'])
    expect(patchedPaths(readFileSync(patches[5]!.path, 'utf8'))).toEqual(['deps/OpenSSL/OpenSSL.cmake'])
  })

  test('patch 0006 pins OpenSSL 3.5.9 (Apache-2.0) by its published SHA-256, in the form source reads', () => {
    const text = readFileSync(join(import.meta.dir, '..', 'patches', '0006-deps-openssl-3.patch'), 'utf8')
    const added = text.split('\n').filter((l) => l.startsWith('+') && !l.startsWith('+++')).map((l) => l.slice(1)).join('\n')
    expect(parsePinnedSources(added)).toEqual([
      { urls: ['https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz'], sha256: '603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a' },
    ])
    expect(text).toContain('-    URL "https://github.com/openssl/openssl/archive/OpenSSL_1_1_1w.tar.gz"')
  })

  test('each patch says it was modified for Sketch, with a date, in its header and in every comment it adds (AGPL-3.0 section 5a)', () => {
    for (const patch of listPatches(join(import.meta.dir, '..', 'patches'))) {
      const text = readFileSync(patch.path, 'utf8')
      const [header] = text.split('\n---\n')
      expect(header).toMatch(/^Modified for Sketch, \d{4}-\d{2}-\d{2}\.$/m)
      // The comments a patch adds: CMake's # and C++'s //.
      const added = text.split('\n').filter((l) => l.startsWith('+') && !l.startsWith('+++') && /(#|\/\/)\s*Sketch/.test(l))
      expect(added.length).toBeGreaterThan(0)
      for (const line of added) expect(line).toMatch(/Sketch \(sketch-orca\), \d{4}-\d{2}-\d{2}/)
    }
  })

  test('patchedPaths reads the diff --git lines', () => {
    const text = 'Header text\n---\ndiff --git a/deps/CMakeLists.txt b/deps/CMakeLists.txt\n@@ -1 +1 @@\ndiff --git a/src/old.txt b/src/new.txt\n'
    expect(patchedPaths(text)).toEqual(['deps/CMakeLists.txt', 'src/new.txt'])
    expect(patchedPaths('no diff here')).toEqual([])
  })

  test('planPatches: nothing applied, a prefix applied, the same set, and every reason to start over', () => {
    const wanted = [p('0001-a.patch'), p('0002-b.patch'), p('0003-c.patch')]
    const applied = (names: string[], commit = COMMIT) => ({ commit, applied: names.map((n) => ({ name: n, sha256: sha256Text(n) })) })
    expect(planPatches(undefined, COMMIT, wanted)).toEqual({ reset: true, apply: wanted })
    expect(planPatches(applied(['0001-a.patch']), COMMIT, wanted)).toEqual({ reset: false, apply: wanted.slice(1) })
    expect(planPatches(applied(['0001-a.patch', '0002-b.patch', '0003-c.patch']), COMMIT, wanted)).toEqual({ reset: false, apply: [] })
    // Another commit, a changed patch, a removed patch, a different order.
    expect(planPatches(applied(['0001-a.patch'], 'f'.repeat(40)), COMMIT, wanted).reset).toBe(true)
    expect(planPatches({ commit: COMMIT, applied: [{ name: '0001-a.patch', sha256: 'changed' }] }, COMMIT, wanted)).toEqual({ reset: true, apply: wanted })
    expect(planPatches(applied(['0001-a.patch', '0002-b.patch', '0003-c.patch', '0004-d.patch']), COMMIT, wanted).reset).toBe(true)
    expect(planPatches(applied(['0002-b.patch']), COMMIT, wanted).reset).toBe(true)
  })

  test('repoUrl is the GitHub clone URL of the lock repo', () => {
    expect(repoUrl({ format: LOCK_FORMAT, repo: 'OrcaSlicer/OrcaSlicer', tag: 'v2.4.2', commit: COMMIT, version: '2.4.2', publishedAt: '', license: 'AGPL-3.0' })).toBe('https://github.com/OrcaSlicer/OrcaSlicer.git')
  })
})
