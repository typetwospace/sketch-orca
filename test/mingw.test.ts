import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { hashTree, sha256Bytes } from '../scripts/lib/files.ts'
import {
  ensureMingw,
  ensurePinnedFile,
  type Fetcher,
  MINGW_PACKAGES,
  MINGW_RUNTIME_LICENSES,
  MINGW_TOOLS,
  mingwPinProblems,
  mingwPinsDigest,
  MINGW_ETC_WRITTEN,
  mingwRoot,
  mingwTreeFile,
  MSYS2_BASE,
  PACKAGE_RECORDS,
  unpackArgs,
  windowsTar,
  type MingwPackage,
} from '../scripts/lib/mingw.ts'
import { readStamp, workPaths, writeStamp } from '../scripts/lib/paths.ts'
import { Log } from '../scripts/lib/run.ts'

const H = (c: string) => c.repeat(64)

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'orca-mingw-'))
})
afterEach(() => {
  rmSync(dir, { recursive: true, force: true })
})

const put = (path: string, text: string) => {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, text)
}

describe('the pinned MinGW-w64 toolchain', () => {
  test('the pins: one file each, a size, a lower-case SHA-256 and https URLs that name the file', () => {
    expect(mingwPinProblems()).toEqual([])
    expect(MSYS2_BASE.file).toBe('msys2-base-x86_64-20260927.tar.xz')
    expect(MSYS2_BASE.urls).toHaveLength(2)
    // make, m4 and diffutils from the msys repository; GCC 16.2.0 and its closure from mingw64.
    const names = MINGW_PACKAGES.map((p) => p.file)
    for (const n of ['make-', 'm4-', 'diffutils-', 'mingw-w64-x86_64-gcc-16.2.0-', 'mingw-w64-x86_64-binutils-', 'mingw-w64-x86_64-crt-', 'mingw-w64-x86_64-headers-', 'mingw-w64-x86_64-libgcc-16.2.0-'])
      expect(names.some((f) => f.startsWith(n))).toBe(true)
    for (const p of MINGW_PACKAGES) expect(p.file).toMatch(/\.pkg\.tar\.zst$/)
  })

  test('mingwPinProblems names each kind of bad pin', () => {
    const good: MingwPackage = { file: 'a.pkg.tar.zst', urls: ['https://x.example/a.pkg.tar.zst'], size: 1, sha256: H('a') }
    expect(mingwPinProblems(good, [])).toEqual([])
    expect(mingwPinProblems(good, [good])).toEqual(['a.pkg.tar.zst is pinned twice'])
    expect(mingwPinProblems({ ...good, size: 0 }, [])).toEqual(['a.pkg.tar.zst: size 0'])
    expect(mingwPinProblems({ ...good, sha256: H('A') }, [])).toEqual([`a.pkg.tar.zst: SHA-256 ${H('A')}`])
    expect(mingwPinProblems({ ...good, urls: [] }, [])).toEqual(['a.pkg.tar.zst: no URL'])
    expect(mingwPinProblems({ ...good, urls: ['http://x.example/a.pkg.tar.zst', 'https://x.example/b.pkg.tar.zst'] }, [])).toEqual([
      'a.pkg.tar.zst: URL http://x.example/a.pkg.tar.zst',
      'a.pkg.tar.zst: URL https://x.example/b.pkg.tar.zst',
    ])
  })

  test('mingwPinsDigest: stable, and changed by any pin', () => {
    expect(mingwPinsDigest()).toBe(mingwPinsDigest(MSYS2_BASE, MINGW_PACKAGES))
    expect(mingwPinsDigest()).toMatch(/^[0-9a-f]{64}$/)
    expect(mingwPinsDigest({ ...MSYS2_BASE, sha256: H('b') })).not.toBe(mingwPinsDigest())
    expect(mingwPinsDigest(MSYS2_BASE, MINGW_PACKAGES.slice(1))).not.toBe(mingwPinsDigest())
    expect(mingwPinsDigest(MSYS2_BASE, [...MINGW_PACKAGES].reverse())).not.toBe(mingwPinsDigest())
  })

  test('mingwRoot is <work>/mingw/msys64', () => {
    expect(mingwRoot(workPaths('W'))).toBe(join('W', 'mingw', 'msys64'))
  })

  test("unpackArgs: Windows' tar into the folder, without pacman's records", () => {
    expect(unpackArgs('p.tar.zst', 'D')).toEqual(['-x', '-f', 'p.tar.zst', '-C', 'D', '--exclude', '.PKGINFO', '--exclude', '.BUILDINFO', '--exclude', '.MTREE', '--exclude', '.INSTALL'])
    expect(PACKAGE_RECORDS).toContain('.MTREE')
    expect(windowsTar({ SystemRoot: 'S' })).toBe(join('S', 'System32', 'tar.exe'))
  })

  test('mingwTreeFile: usr/, mingw64/ and the configuration MSYS2 reads as it starts, not what it writes', () => {
    for (const rel of ['usr', 'usr/bin/bash.exe', 'mingw64', 'mingw64/bin/gcc.exe']) expect(mingwTreeFile(rel)).toBe(true)
    // The mount table (one in fstab.d could put another folder at /mingw64) and the user database.
    for (const rel of ['etc', 'etc/fstab', 'etc/fstab.d', 'etc/fstab.d/builder', 'etc/nsswitch.conf', 'etc/passwd', 'etc/group', 'etc/profile', 'etc/pacman.d/mirrorlist.msys']) expect(mingwTreeFile(rel)).toBe(true)
    for (const rel of ['tmp', 'tmp/x', 'var/lib/pacman', 'home/u/.bash_history', 'dev', 'usrx', 'mingw64x/a', 'etcx/fstab']) expect(mingwTreeFile(rel)).toBe(false)
    // What MSYS2's first login shell writes into etc/, which the build's bash (--noprofile) neither writes nor reads.
    for (const rel of [...MINGW_ETC_WRITTEN, 'etc/pacman.d/gnupg/pubring.gpg']) expect(mingwTreeFile(rel)).toBe(false)
    expect(MINGW_ETC_WRITTEN).not.toContain('etc/fstab')
  })

  test('the tools and licences it names are in the packages it pins', () => {
    expect(MINGW_TOOLS).toEqual(['usr/bin/bash.exe', 'usr/bin/make.exe', 'usr/bin/m4.exe', 'usr/bin/cygpath.exe', 'mingw64/bin/gcc.exe', 'mingw64/bin/strip.exe'])
    expect(MINGW_RUNTIME_LICENSES.map((l) => l.from)).toEqual([
      'mingw64/share/licenses/crt/COPYING.MinGW-w64-runtime.txt',
      'mingw64/share/licenses/libgcc/COPYING.RUNTIME',
      'mingw64/share/licenses/libgcc/COPYING3',
    ])
  })
})

describe('ensurePinnedFile', () => {
  const bytes = new TextEncoder().encode('a package')
  const urls = ['https://a.example/p.pkg.tar.zst', 'https://b.example/p.pkg.tar.zst', 'https://c.example/p.pkg.tar.zst']
  const pin: MingwPackage = { file: 'p.pkg.tar.zst', urls, size: bytes.length, sha256: sha256Bytes(bytes) }

  test('kept when it matches, else downloaded from the URLs in turn and checked', async () => {
    const asked: string[] = []
    const fetcher: Fetcher = async (url) => {
      asked.push(url)
      if (url.startsWith('https://a.')) throw new Error('HTTP 404')
      if (url.startsWith('https://b.')) return new TextEncoder().encode('something else')
      return bytes
    }
    const lines: string[] = []
    const path = join(dir, 'downloads', pin.file)
    expect(await ensurePinnedFile(pin, path, fetcher, (l) => lines.push(l))).toBe(path)
    expect(readFileSync(path, 'utf8')).toBe('a package')
    expect(existsSync(`${path}.partial`)).toBe(false)
    expect(asked).toEqual(urls)
    // Already there: nothing is downloaded.
    asked.length = 0
    expect(await ensurePinnedFile(pin, path, fetcher, () => {})).toBe(path)
    expect(asked).toEqual([])
    // A copy that does not match is replaced.
    writeFileSync(path, 'tampered')
    expect(await ensurePinnedFile(pin, path, fetcher, (l) => lines.push(l))).toBe(path)
    expect(readFileSync(path, 'utf8')).toBe('a package')
    expect(lines.some((l) => l.includes('is not the pinned p.pkg.tar.zst'))).toBe(true)
  })

  test('refused, naming what each URL gave, when none gives the pinned bytes', async () => {
    const p: MingwPackage = { file: 'q.pkg.tar.zst', urls: ['https://a.example/q.pkg.tar.zst', 'https://b.example/q.pkg.tar.zst'], size: 3, sha256: H('e') }
    const fetcher: Fetcher = async (url) => {
      if (url.startsWith('https://a.')) throw new Error('HTTP 503')
      return new TextEncoder().encode('abc')
    }
    const path = join(dir, 'downloads', p.file)
    await expect(ensurePinnedFile(p, path, fetcher, () => {})).rejects.toThrow(
      /cannot get q\.pkg\.tar\.zst: https:\/\/a\.example\/q\.pkg\.tar\.zst: HTTP 503; https:\/\/b\.example\/q\.pkg\.tar\.zst: 3 bytes with SHA-256 [0-9a-f]{64}, not 3 bytes with e{64}/,
    )
    expect(existsSync(path)).toBe(false)
  })
})

describe('ensureMingw', () => {
  const noFetch: Fetcher = async (url) => {
    throw new Error(`fetched ${url}`)
  }
  const tree = (root: string) => {
    for (const t of MINGW_TOOLS) put(join(root, ...t.split('/')), t)
    put(join(root, 'mingw64', 'include', 'stdio.h'), 'header')
  }

  test('a tree the stamp records, unchanged in usr/ and mingw64/, is kept without downloading anything', async () => {
    const w = workPaths(join(dir, 'work'))
    const root = mingwRoot(w)
    tree(root)
    writeStamp(w.state, 'mingw', mingwPinsDigest(), { root, treeDigest: hashTree(root, (rel) => mingwTreeFile(rel)) })
    const log = new Log(join(dir, 'build.log'), true)
    expect(await ensureMingw(w, log, noFetch, join(dir, 'no-tar.exe'))).toBe(root)
    // What MSYS2 writes as it runs does not count.
    put(join(root, 'tmp', 'x'), 'x')
    put(join(root, 'home', 'u', '.bash_history'), 'ls')
    expect(await ensureMingw(w, log, noFetch, join(dir, 'no-tar.exe'))).toBe(root)
    expect(readFileSync(log.path, 'utf8')).toContain('is the pinned toolchain; keeping it')
  })

  test('a mount added in etc/fstab.d since the unpack is a changed tree; what MSYS2 writes into etc/ itself is not', async () => {
    const w = workPaths(join(dir, 'work'))
    const root = mingwRoot(w)
    tree(root)
    put(join(root, 'etc', 'fstab'), 'none / cygdrive binary,posix=0,noacl,user 0 0\n')
    writeStamp(w.state, 'mingw', mingwPinsDigest(), { root, treeDigest: hashTree(root, (rel) => mingwTreeFile(rel)) })
    const log = new Log(join(dir, 'build.log'), true)
    const noTar = join(dir, 'no-tar.exe')
    put(join(root, 'etc', 'hosts'), '127.0.0.1 localhost\n')
    put(join(root, 'etc', 'pacman.d', 'gnupg', 'pubring.gpg'), 'k')
    expect(await ensureMingw(w, log, noFetch, noTar)).toBe(root)
    put(join(root, 'etc', 'fstab.d', 'builder'), 'C:/elsewhere /mingw64 ntfs binary 0 0\n')
    await expect(ensureMingw(w, log, noFetch, noTar)).rejects.toThrow(/not found/)
    expect(readFileSync(log.path, 'utf8')).toContain('changed since it was unpacked; unpacking it again')
  })

  test('a changed tree, other pins or a missing tool: unpacked again, which needs tar', async () => {
    const w = workPaths(join(dir, 'work'))
    const root = mingwRoot(w)
    tree(root)
    const digest = hashTree(root, (rel) => mingwTreeFile(rel))
    const log = new Log(join(dir, 'build.log'), true)
    const noTar = join(dir, 'no-tar.exe')
    // A changed file.
    writeStamp(w.state, 'mingw', mingwPinsDigest(), { root, treeDigest: digest })
    put(join(root, 'mingw64', 'include', 'stdio.h'), 'changed')
    await expect(ensureMingw(w, log, noFetch, noTar)).rejects.toThrow(/no-tar\.exe not found: the MinGW-w64 toolchain is unpacked with Windows' own tar/)
    expect(readFileSync(log.path, 'utf8')).toContain('changed since it was unpacked; unpacking it again')
    // Other pins.
    put(join(root, 'mingw64', 'include', 'stdio.h'), 'header')
    writeStamp(w.state, 'mingw', 'other pins', { root, treeDigest: digest })
    await expect(ensureMingw(w, log, noFetch, noTar)).rejects.toThrow(/not found/)
    // No stamp, or a tool missing.
    rmSync(join(w.state, 'mingw.json'))
    await expect(ensureMingw(w, log, noFetch, noTar)).rejects.toThrow(/not found/)
    writeStamp(w.state, 'mingw', mingwPinsDigest(), { root, treeDigest: digest })
    rmSync(join(root, 'usr', 'bin', 'm4.exe'))
    await expect(ensureMingw(w, log, noFetch, noTar)).rejects.toThrow(/not found/)
    // Nothing was downloaded before tar was found missing.
    expect(existsSync(join(w.mingw, 'downloads'))).toBe(false)
  })

  test('a downloaded package that is not the pinned one stops the unpacking before tar runs', async () => {
    const w = workPaths(join(dir, 'work'))
    const tar = join(dir, 'tar.exe')
    put(tar, 'not run')
    const fetcher: Fetcher = async () => new TextEncoder().encode('not the base system')
    const log = new Log(join(dir, 'build.log'), true)
    await expect(ensureMingw(w, log, fetcher, tar)).rejects.toThrow(/cannot get msys2-base-x86_64-20260927\.tar\.xz/)
    expect(readStamp(w.state, 'mingw')).toBeUndefined()
    expect(existsSync(mingwRoot(w))).toBe(false)
  })
})
