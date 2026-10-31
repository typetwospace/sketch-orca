// Files and folders for stage and source: listing, hashing and copying in a stable order, with
// forward-slash paths relative to a root, so manifests read the same on every platform.

import { createHash } from 'node:crypto'
import { copyFileSync, existsSync, linkSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

export interface FileEntry {
  /** Relative to the root, with forward slashes. */
  path: string
  size: number
  sha256: string
}

/** Byte order of the UTF-16 code units; the same on every platform and locale. */
export function byPath(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0
}

/** `rel` (forward slashes) under `root`, as a native path. */
export function nativePath(root: string, rel: string): string {
  return join(root, ...rel.split('/'))
}

/**
 * Every file under `dir`, recursively, as sorted relative paths. `keep` sees each relative path
 * (folders too) and can leave it out. Symbolic links are refused, so a manifest never depends on
 * where a link points.
 */
export function listFiles(dir: string, keep?: (rel: string, isDir: boolean) => boolean): string[] {
  const out: string[] = []
  const walk = (abs: string, rel: string): void => {
    for (const ent of readdirSync(abs, { withFileTypes: true })) {
      const r = rel === '' ? ent.name : `${rel}/${ent.name}`
      const a = join(abs, ent.name)
      if (ent.isSymbolicLink()) throw new Error(`${a} is a symbolic link; refusing to follow it`)
      if (ent.isDirectory()) {
        if (!keep || keep(r, true)) walk(a, r)
      } else if (ent.isFile()) {
        if (!keep || keep(r, false)) out.push(r)
      }
    }
  }
  if (existsSync(dir)) walk(dir, '')
  return out.sort(byPath)
}

export function sha256Bytes(bytes: Uint8Array | string): string {
  return createHash('sha256').update(bytes).digest('hex')
}

export function fileEntry(root: string, rel: string): FileEntry {
  const bytes = readFileSync(nativePath(root, rel))
  return { path: rel, size: bytes.length, sha256: sha256Bytes(bytes) }
}

export function treeEntries(dir: string, keep?: (rel: string, isDir: boolean) => boolean): FileEntry[] {
  return listFiles(dir, keep).map((rel) => fileEntry(dir, rel))
}

/** One hash for a list of files: names, sizes and contents (the list is sorted first). */
export function hashEntries(entries: FileEntry[]): string {
  const lines = [...entries].sort((a, b) => byPath(a.path, b.path)).map((e) => `${e.path}\u0000${e.size}\u0000${e.sha256}\n`)
  return sha256Bytes(lines.join(''))
}

/** The hash of a folder's files (a missing folder hashes like an empty one). */
export function hashTree(dir: string, keep?: (rel: string, isDir: boolean) => boolean): string {
  return hashEntries(treeEntries(dir, keep))
}

/**
 * Copies one file, creating its folder. With `link`, a hard link is tried first: same bytes, no
 * extra space when both paths are on one volume (the work folder). The target is replaced.
 */
export function copyFile(from: string, to: string, opts: { link?: boolean } = {}): void {
  mkdirSync(dirname(to), { recursive: true })
  if (existsSync(to)) rmSync(to, { force: true })
  if (opts.link) {
    try {
      linkSync(from, to)
      return
    } catch {
      // Another volume or no permission: copy instead.
    }
  }
  copyFileSync(from, to)
}

/** Copies the files of a folder (see listFiles for `keep`); returns the relative paths copied. */
export function copyTree(fromDir: string, toDir: string, keep?: (rel: string, isDir: boolean) => boolean, opts: { link?: boolean } = {}): string[] {
  const files = listFiles(fromDir, keep)
  for (const rel of files) copyFile(nativePath(fromDir, rel), nativePath(toDir, rel), opts)
  return files
}

/** Writes text with LF line endings as given, creating the folder. */
export function writeText(path: string, text: string): void {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, text)
}

/** `sha256sum` format: one `<hash>  <path>` line per file, sorted by path. */
export function sha256Sums(entries: FileEntry[]): string {
  return [...entries]
    .sort((a, b) => byPath(a.path, b.path))
    .map((e) => `${e.sha256}  ${e.path}\n`)
    .join('')
}

export function parseSha256Sums(text: string): Map<string, string> {
  const out = new Map<string, string>()
  for (const line of text.split('\n')) {
    if (line.trim() === '') continue
    const m = /^([0-9a-f]{64}) [ *](.+)$/.exec(line)
    if (!m) throw new Error(`not a sha256sum line: ${JSON.stringify(line)}`)
    out.set(m[2]!, m[1]!)
  }
  return out
}

/**
 * Checks a folder against a list of expected files: every listed file present with its size and
 * hash, and no other file (apart from `ignore`). Returns the problems, empty when it matches.
 */
export function verifyTree(dir: string, expected: FileEntry[], ignore: string[] = []): string[] {
  const problems: string[] = []
  const want = new Map(expected.map((e) => [e.path, e]))
  const skip = new Set(ignore)
  const present = listFiles(dir).filter((p) => !skip.has(p))
  for (const p of present) if (!want.has(p)) problems.push(`unexpected file ${p}`)
  for (const e of expected) {
    const abs = nativePath(dir, e.path)
    if (!existsSync(abs)) {
      problems.push(`missing ${e.path}`)
      continue
    }
    const got = fileEntry(dir, e.path)
    if (got.size !== e.size || got.sha256 !== e.sha256) problems.push(`changed ${e.path}`)
  }
  return problems
}
