// privacy-fixtures: test@example.invalid
import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { parsePinnedSources } from '../scripts/lib/bundle.ts'
import { applyPatches, listPatches, patchedDigest, patchedPaths, planPatches, repoUrl, sourceDigest, type PatchInfo } from '../scripts/lib/checkout.ts'
import { firstExisting, gitCandidates } from '../scripts/lib/env.ts'
import { LOCK_FORMAT } from '../scripts/lib/lock.ts'
import { sha256Text } from '../scripts/lib/paths.ts'
import { Log } from '../scripts/lib/run.ts'
import { currentPlatform } from '../scripts/lib/toolchain.ts'

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

describe('the checkout as the patches made it', () => {
  // Each case runs git a few dozen times, slow on Windows while the other test files run.
  const git = firstExisting(gitCandidates(currentPlatform(), process.env))
  let dir = ''
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'orca-source-'))
  })
  afterEach(() => {
    rmSync(dir, { recursive: true, force: true })
  })

  /** git in the test's own repository, with no user or system configuration. */
  function inRepo(repo: string, ...args: string[]): string {
    const empty = join(dir, 'gitconfig')
    if (!existsSync(empty)) writeFileSync(empty, '')
    const r = spawnSync(git!, ['-C', repo, ...args], {
      encoding: 'utf8',
      windowsHide: true,
      env: { ...process.env, GIT_CONFIG_GLOBAL: empty, GIT_CONFIG_NOSYSTEM: '1', GIT_AUTHOR_NAME: 'test', GIT_AUTHOR_EMAIL: 'test@example.invalid', GIT_COMMITTER_NAME: 'test', GIT_COMMITTER_EMAIL: 'test@example.invalid' },
    })
    if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`)
    return r.stdout
  }

  function put(path: string, text: string | Buffer): void {
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, text)
  }

  /** A repository with a source file, a resource and an ignored folder, committed. */
  function makeRepo(): { repo: string; head: string } {
    const repo = join(dir, 'src')
    put(join(repo, 'src', 'a.cpp'), 'int a() { return 1; }\n')
    put(join(repo, 'resources', 'profiles', 'p.json'), '{"layer_height": "0.2"}\n')
    put(join(repo, '.gitignore'), 'ignored/\n')
    inRepo(repo, 'init', '-q')
    // As fetch clones Orca: the pipeline's own git calls see the user's configuration, but not this.
    inRepo(repo, 'config', 'core.autocrlf', 'false')
    inRepo(repo, 'add', '-A')
    inRepo(repo, 'commit', '-q', '-m', 'first')
    return { repo, head: inRepo(repo, 'rev-parse', 'HEAD').trim() }
  }

  /** A patch of the repository (an edit and a new file), written beside it; the checkout is left clean. */
  function makePatch(repo: string): string {
    put(join(repo, 'src', 'a.cpp'), 'int a() { return 1; }\nint a2() { return 2; }\n')
    put(join(repo, 'src', 'new.cpp'), 'int n() { return 3; }\n')
    inRepo(repo, 'add', '-A')
    const path = join(dir, 'a.patch')
    writeFileSync(path, inRepo(repo, 'diff', '--cached'))
    inRepo(repo, 'reset', '-q', '--hard')
    return path
  }

  test.skipIf(git === undefined)('sourceDigest: any edit, added or removed file changes it; the same tree gives the same digest', () => {
    const { repo, head } = makeRepo()
    const clean = sourceDigest(git!, repo, head)
    expect(sourceDigest(git!, repo, head)).toBe(clean)
    const seen = new Set([clean])
    const changed = (): string => {
      const d = sourceDigest(git!, repo, head)
      expect(seen.has(d)).toBe(false)
      seen.add(d)
      return d
    }
    put(join(repo, 'src', 'a.cpp'), 'int a() { return 2; }\n')
    changed()
    // Bytes that are not UTF-8 count as themselves.
    put(join(repo, 'src', 'a.cpp'), Buffer.from([0x69, 0xe9, 0x0a]))
    changed()
    put(join(repo, 'src', 'a.cpp'), Buffer.from([0x69, 0xe8, 0x0a]))
    changed()
    put(join(repo, 'src', 'a.cpp'), 'int a() { return 1; }\n')
    expect(sourceDigest(git!, repo, head)).toBe(clean)
    // A staged edit counts as well.
    put(join(repo, 'resources', 'profiles', 'p.json'), '{"layer_height": "0.3"}\n')
    inRepo(repo, 'add', '-A')
    changed()
    inRepo(repo, 'reset', '-q', '--hard')
    expect(sourceDigest(git!, repo, head)).toBe(clean)
    // A file git does not track, by its content; a removed one.
    put(join(repo, 'src', 'b.cpp'), 'int b();\n')
    changed()
    put(join(repo, 'src', 'b.cpp'), 'int b(int);\n')
    changed()
    rmSync(join(repo, 'src', 'b.cpp'))
    expect(sourceDigest(git!, repo, head)).toBe(clean)
    rmSync(join(repo, 'resources', 'profiles', 'p.json'))
    changed()
    inRepo(repo, 'checkout', '-q', '--', '.')
    // What .gitignore leaves out is not part of it.
    put(join(repo, 'ignored', 'cache.bin'), 'x')
    expect(sourceDigest(git!, repo, head)).toBe(clean)
  }, 60_000)

  test.skipIf(git === undefined)('patchedDigest: the digest of the commit with the patches applied, worked out without touching the checkout', () => {
    const { repo, head } = makeRepo()
    const patch = makePatch(repo)
    const clean = sourceDigest(git!, repo, head)
    expect(patchedDigest(git!, repo, head, [])).toBe(clean)
    expect(patchedDigest(git!, repo, head, [{ path: patch }])).not.toBe(clean)
    // The checkout is as it was.
    expect(sourceDigest(git!, repo, head)).toBe(clean)
    expect(existsSync(join(repo, 'src', 'new.cpp'))).toBe(false)
    inRepo(repo, 'apply', patch)
    expect(sourceDigest(git!, repo, head)).toBe(patchedDigest(git!, repo, head, [{ path: patch }]))
  }, 60_000)

  test.skipIf(git === undefined)('applyPatches keeps a checkout that is the commit with the patches applied, and resets one that is not', async () => {
    const { repo, head } = makeRepo()
    const patch = makePatch(repo)
    const patches = join(dir, 'patches')
    put(join(patches, '0001-a.patch'), readFileSync(patch))
    const state = join(dir, 'state')
    const lock = { format: LOCK_FORMAT, repo: 'OrcaSlicer/OrcaSlicer', tag: 'v0', commit: head, version: '0', publishedAt: '', license: 'AGPL-3.0' } as const
    const log = new Log(join(dir, 'build.log'), true)
    const logText = () => readFileSync(log.path, 'utf8')
    const patched = patchedDigest(git!, repo, head, [{ path: patch }])

    await applyPatches(git!, lock, repo, patches, state, log)
    expect(JSON.parse(readFileSync(join(state, 'patches.json'), 'utf8')).applied).toEqual([{ name: '0001-a.patch', sha256: sha256Text(readFileSync(patch, 'utf8')) }])
    expect(sourceDigest(git!, repo, head)).toBe(patched)
    expect(readFileSync(join(repo, 'src', 'new.cpp'), 'utf8')).toBe('int n() { return 3; }\n')

    // Run again on the same checkout: nothing to do.
    await applyPatches(git!, lock, repo, patches, state, log)
    expect(logText()).toContain('patches: 1 already applied')
    expect(logText()).not.toContain('resetting it')

    // An edit no patch made (it would be compiled, and staged, without being in the source bundle).
    put(join(repo, 'resources', 'profiles', 'p.json'), '{"layer_height": "0.1"}\n')
    put(join(repo, 'src', 'extra.cpp'), 'int x();\n')
    await applyPatches(git!, lock, repo, patches, state, log)
    expect(logText()).toContain('is not the locked commit with the recorded patches applied')
    expect(readFileSync(join(repo, 'resources', 'profiles', 'p.json'), 'utf8')).toBe('{"layer_height": "0.2"}\n')
    expect(existsSync(join(repo, 'src', 'extra.cpp'))).toBe(false)
    expect(readFileSync(join(repo, 'src', 'a.cpp'), 'utf8')).toContain('a2()')
    expect(sourceDigest(git!, repo, head)).toBe(patched)

    // A record that says the patch is applied, on a checkout where it is not (a reset by hand).
    inRepo(repo, 'reset', '-q', '--hard')
    inRepo(repo, 'clean', '-fdq')
    writeFileSync(log.path, '')
    await applyPatches(git!, lock, repo, patches, state, log)
    expect(logText()).toContain('resetting it')
    expect(sourceDigest(git!, repo, head)).toBe(patched)
  }, 60_000)
})
