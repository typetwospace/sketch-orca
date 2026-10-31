import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  byPath,
  copyFile,
  copyTree,
  fileEntry,
  hashEntries,
  hashTree,
  listFiles,
  nativePath,
  parseSha256Sums,
  sha256Bytes,
  sha256Sums,
  treeEntries,
  verifyTree,
  writeText,
} from '../scripts/lib/files.ts'

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'orca-files-'))
})
afterEach(() => {
  rmSync(dir, { recursive: true, force: true })
})

function put(rel: string, text: string): void {
  writeText(nativePath(dir, rel), text)
}

describe('files', () => {
  test('listFiles: every file, sorted by code unit, forward slashes; keep can leave files and folders out', () => {
    put('b.txt', 'b')
    put('a/Z.txt', 'z')
    put('a/a.txt', 'a')
    put('node_modules/x.js', 'x')
    expect(listFiles(dir)).toEqual(['a/Z.txt', 'a/a.txt', 'b.txt', 'node_modules/x.js'])
    expect(listFiles(dir, (rel, isDir) => !(isDir && rel === 'node_modules'))).toEqual(['a/Z.txt', 'a/a.txt', 'b.txt'])
    expect(listFiles(join(dir, 'missing'))).toEqual([])
    expect(['b', 'B', 'a'].sort(byPath)).toEqual(['B', 'a', 'b'])
  })

  test('entries, hashes and tree hashes', () => {
    put('x/y.txt', 'hello')
    expect(sha256Bytes('hello')).toBe('2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824')
    expect(fileEntry(dir, 'x/y.txt')).toEqual({ path: 'x/y.txt', size: 5, sha256: sha256Bytes('hello') })
    const entries = treeEntries(dir)
    expect(hashEntries(entries)).toBe(hashEntries([...entries].reverse()))
    const before = hashTree(dir)
    put('x/y.txt', 'hellO')
    expect(hashTree(dir)).not.toBe(before)
    expect(hashTree(join(dir, 'missing'))).toBe(hashEntries([]))
  })

  test('copyFile creates the folder and replaces the target; with link it shares the bytes when it can', () => {
    put('src/a.bin', 'one')
    const to = join(dir, 'out', 'deep', 'a.bin')
    copyFile(join(dir, 'src', 'a.bin'), to)
    expect(readFileSync(to, 'utf8')).toBe('one')
    put('src/a.bin', 'two')
    copyFile(join(dir, 'src', 'a.bin'), to, { link: true })
    expect(readFileSync(to, 'utf8')).toBe('two')
    expect(statSync(to).nlink).toBeGreaterThanOrEqual(1)
  })

  test('copyTree copies what listFiles lists and returns it', () => {
    put('t/a.txt', 'a')
    put('t/skip/b.txt', 'b')
    const copied = copyTree(join(dir, 't'), join(dir, 'u'), (rel, isDir) => !(isDir && rel === 'skip'))
    expect(copied).toEqual(['a.txt'])
    expect(existsSync(join(dir, 'u', 'a.txt'))).toBe(true)
    expect(existsSync(join(dir, 'u', 'skip'))).toBe(false)
  })

  test('sha256sum format round-trips and refuses other lines', () => {
    const entries = [
      { path: 'b/c.txt', size: 1, sha256: 'b'.repeat(64) },
      { path: 'a.txt', size: 2, sha256: 'a'.repeat(64) },
    ]
    const text = sha256Sums(entries)
    expect(text).toBe(`${'a'.repeat(64)}  a.txt\n${'b'.repeat(64)}  b/c.txt\n`)
    expect([...parseSha256Sums(text)]).toEqual([
      ['a.txt', 'a'.repeat(64)],
      ['b/c.txt', 'b'.repeat(64)],
    ])
    expect(parseSha256Sums(`${'c'.repeat(64)} *bin.exe\n\n`).get('bin.exe')).toBe('c'.repeat(64))
    expect(() => parseSha256Sums('nonsense  file\n')).toThrow(/not a sha256sum line/)
  })

  test('verifyTree finds unexpected, missing and changed files, and skips the ignored ones', () => {
    put('keep.txt', 'k')
    put('change.txt', 'c')
    const expected = [fileEntry(dir, 'keep.txt'), fileEntry(dir, 'change.txt'), { path: 'gone.txt', size: 1, sha256: 'd'.repeat(64) }]
    put('change.txt', 'C')
    put('extra.txt', 'e')
    put('manifest.json', '{}')
    expect(verifyTree(dir, expected, ['manifest.json'])).toEqual(['unexpected file extra.txt', 'changed change.txt', 'missing gone.txt'])
    rmSync(join(dir, 'extra.txt'))
    put('change.txt', 'c')
    mkdirSync(join(dir, 'empty'), { recursive: true })
    expect(verifyTree(dir, expected.slice(0, 2), ['manifest.json'])).toEqual([])
  })

  test('writeText writes the text as given (LF stays LF)', () => {
    writeText(join(dir, 'n', 'f.txt'), 'a\nb\n')
    expect(readFileSync(join(dir, 'n', 'f.txt'))).toEqual(Buffer.from('a\nb\n'))
  })
})
