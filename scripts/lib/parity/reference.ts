// The reference engines of the parity harness: published sketch-orca engines, downloaded from this
// repository's GitHub releases and unpacked into <work>/parity/reference/<version>. The engine zip
// alone is downloaded, never the source zip next to it (611 MB for 0.1.0): the source of a
// reference is this repository at the release's tag.
//
// Each reference is pinned (REFERENCES): its tag, the asset's name, size and SHA-256. A download
// is checked while it streams against the size the release lists and the SHA-256 that the
// release's SHA256SUMS gives, and that SHA-256 must be the pinned one (and GitHub's own digest of
// the asset, when it lists one). The unpacked folder must be the staged engine its manifest
// describes: every file with its size and SHA-256 (stage.ts verifyStaged), the pinned stage
// version and engine version. It is unpacked next to its place and moved there whole, and
// <reference>/<version>.json records what it was unpacked from, so a second run only checks it.

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { nativePath, parseSha256Sums, sha256Bytes } from '../files.ts'
import { downloadReleaseAsset, ReleaseError, releaseAsset, resolveTaggedRelease, type FetchLike, type JsonGetter, type ReleaseAsset } from '../github.ts'
import type { ParityPaths } from '../paths.ts'
import { isSafeRelPath, MANIFEST_FILE, readManifest, verifyStaged, type EngineManifest } from '../stage.ts'
import { extractZipEntry, readZipEntries } from '../zip.ts'

/** This repository on GitHub, whose releases carry the published engines. */
export const SKETCH_ORCA_REPO = 'typetwospace/sketch-orca'

/** The checksum list every release carries next to its zips (sha256sum format). */
export const SUMS_ASSET = 'SHA256SUMS'

export interface ReferencePin {
  version: string
  tag: string
  /** The engine zip (never the -source.zip). */
  asset: string
  size: number
  sha256: string
  /** What its manifest must say. */
  stageVersion: number
  platform: 'win32' | 'linux' | 'darwin'
  arch: string
}

/**
 * The published engines the harness can download. 0.1.0: the release "sketch-orca 0.1.0
 * (OrcaSlicer 2.4.2)", published 2026-11-02 from 0da2256.
 */
export const REFERENCES: Record<string, ReferencePin> = {
  '0.1.0': {
    version: '0.1.0',
    tag: 'v0.1.0',
    asset: 'sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip',
    size: 40_813_895,
    sha256: 'a8f40e74ae4d5b91a819cee2d752e9863869900f1327a8f8032a74a7c45d6ebd',
    stageVersion: 3,
    platform: 'win32',
    arch: 'x64',
  },
}

export const DEFAULT_REFERENCE = '0.1.0'

/** The folder a reference is unpacked to, and the record of it. */
export function referenceDir(paths: Pick<ParityPaths, 'reference'>, version: string): { dir: string; record: string; downloads: string } {
  return { dir: join(paths.reference, version), record: join(paths.reference, `${version}.json`), downloads: join(paths.reference, 'downloads', version) }
}

/** What is wrong with an unpacked reference's manifest for its pin (empty when it fits). */
export function referenceManifestProblems(m: EngineManifest, pin: ReferencePin): string[] {
  const out: string[] = []
  if (m.stageVersion !== pin.stageVersion) out.push(`its manifest has stage version ${m.stageVersion}, the pin ${pin.stageVersion}`)
  if (m.engine.version !== pin.version) out.push(`its manifest names engine version ${m.engine.version}, the pin ${pin.version}`)
  if (m.platform !== pin.platform || m.arch !== pin.arch) out.push(`it is for ${m.platform}-${m.arch}, the pin for ${pin.platform}-${pin.arch}`)
  if (!m.smoke.passed) out.push('its manifest says it did not pass smoke')
  return out
}

/**
 * Unpacks a zip into `dest` (made empty first). Every entry must be a relative path inside it
 * (stage.ts isSafeRelPath) and name a file once, in any letter case; each entry's CRC-32 and size
 * are checked (zip.ts). Returns the files, as forward-slash paths.
 */
export function unpackZip(zip: Buffer, dest: string): string[] {
  rmSync(dest, { recursive: true, force: true })
  mkdirSync(dest, { recursive: true })
  const files: string[] = []
  const seen = new Set<string>()
  for (const e of readZipEntries(zip)) {
    const isDir = e.name.endsWith('/')
    const rel = isDir ? e.name.slice(0, -1) : e.name
    if (!isSafeRelPath(rel)) throw new Error(`zip: refusing the entry ${JSON.stringify(e.name)}: not a relative path inside the folder`)
    const key = rel.toLowerCase()
    if (seen.has(key)) throw new Error(`zip: ${JSON.stringify(e.name)} is there twice`)
    seen.add(key)
    const to = nativePath(dest, rel)
    if (isDir) {
      mkdirSync(to, { recursive: true })
      continue
    }
    mkdirSync(join(to, '..'), { recursive: true })
    writeFileSync(to, extractZipEntry(zip, e))
    files.push(rel)
  }
  return files.sort()
}

/** The folder of an unpacked engine zip that holds manifest.json: the zip's root, or its one top folder. */
export function engineRoot(unpacked: string): string {
  if (existsSync(join(unpacked, MANIFEST_FILE))) return unpacked
  const top = readdirSync(unpacked)
  if (top.length === 1 && statSync(join(unpacked, top[0]!)).isDirectory() && existsSync(join(unpacked, top[0]!, MANIFEST_FILE))) return join(unpacked, top[0]!)
  throw new Error(`the zip has no ${MANIFEST_FILE} at its root or in its one top folder`)
}

/** A file already downloaded is kept when it has the size and SHA-256 wanted. */
function haveFile(path: string, size: number, sha256: string): boolean {
  if (!existsSync(path)) return false
  const bytes = readFileSync(path)
  return bytes.length === size && sha256Bytes(bytes) === sha256
}

export interface ReferenceRecord {
  format: 'sketch-orca-parity-reference/1'
  version: string
  tag: string
  repo: string
  asset: string
  size: number
  sha256: string
  /** The release's download address of the zip (before GitHub's redirects, whose addresses are signed and expire). */
  url: string
  unpackedAt: string
}

export interface ReferenceInstall {
  dir: string
  manifest: EngineManifest
  /** True when the reference was already there and checked, and nothing was downloaded. */
  kept: boolean
}

/** A reference unpacked and recorded earlier, when it still is what its manifest lists; else undefined. */
export function keptReference(paths: Pick<ParityPaths, 'reference'>, pin: ReferencePin): ReferenceInstall | undefined {
  const { dir, record } = referenceDir(paths, pin.version)
  if (!existsSync(record) || !existsSync(dir)) return undefined
  try {
    const r = JSON.parse(readFileSync(record, 'utf8')) as Partial<ReferenceRecord>
    if (r.asset !== pin.asset || r.sha256 !== pin.sha256 || r.size !== pin.size) return undefined
    const manifest = readManifest(dir)
    if (referenceManifestProblems(manifest, pin).length > 0 || verifyStaged(dir, manifest).length > 0) return undefined
    return { dir, manifest, kept: true }
  } catch {
    return undefined
  }
}

export interface FetchReferenceOptions {
  pin: ReferencePin
  paths: Pick<ParityPaths, 'reference'>
  get: JsonGetter
  fetch?: FetchLike
  repo?: string
  log: (msg: string) => void
}

/**
 * Downloads, checks and unpacks a reference engine (see the top of this file); keeps one that is
 * already there and still checks out. Throws on any mismatch, leaving no partial folder behind.
 */
export async function fetchReference(o: FetchReferenceOptions): Promise<ReferenceInstall> {
  const { pin } = o
  const repo = o.repo ?? SKETCH_ORCA_REPO
  const kept = keptReference(o.paths, pin)
  if (kept) {
    o.log(`${pin.version} is unpacked in ${kept.dir} and every file is what its manifest lists; nothing to download`)
    return kept
  }
  const { dir, record, downloads } = referenceDir(o.paths, pin.version)
  const release = await resolveTaggedRelease(repo, pin.tag, o.get)
  const zipAsset = releaseAsset(release, pin.asset)
  if (zipAsset.size !== pin.size) throw new ReleaseError(`${pin.asset}: the release lists ${zipAsset.size} bytes, the pin ${pin.size}`)
  if (zipAsset.sha256 !== undefined && zipAsset.sha256 !== pin.sha256) throw new ReleaseError(`${pin.asset}: GitHub lists SHA-256 ${zipAsset.sha256}, the pin ${pin.sha256}`)
  const sumsAsset: ReleaseAsset = releaseAsset(release, SUMS_ASSET)
  mkdirSync(downloads, { recursive: true })
  const sumsPath = join(downloads, SUMS_ASSET)
  if (!sumsAsset.sha256 || !haveFile(sumsPath, sumsAsset.size, sumsAsset.sha256)) {
    o.log(`downloading ${SUMS_ASSET} of ${repo} ${pin.tag}`)
    await downloadReleaseAsset(sumsAsset, sumsPath, { fetch: o.fetch })
  }
  const sums = parseSha256Sums(readFileSync(sumsPath, 'utf8').replace(/\r\n/g, '\n'))
  const listed = sums.get(pin.asset)
  if (!listed) throw new ReleaseError(`${SUMS_ASSET} of ${pin.tag} does not list ${pin.asset}`)
  if (listed !== pin.sha256) throw new ReleaseError(`${SUMS_ASSET} of ${pin.tag} gives ${pin.asset} the SHA-256 ${listed}, the pin ${pin.sha256}`)
  const zipPath = join(downloads, pin.asset)
  if (haveFile(zipPath, pin.size, listed)) {
    o.log(`${pin.asset} is already downloaded, with the size and SHA-256 of ${SUMS_ASSET}`)
  } else {
    o.log(`downloading ${pin.asset} (${pin.size} bytes) of ${repo} ${pin.tag}`)
    await downloadReleaseAsset(zipAsset, zipPath, { sha256: listed, fetch: o.fetch })
  }
  o.log(`${pin.asset}: ${pin.size} bytes, SHA-256 ${listed}, as ${SUMS_ASSET} lists it`)
  const partial = `${dir}.partial`
  try {
    const zip = readFileSync(zipPath)
    // Checked once more as read: the file may have changed since it was downloaded.
    if (zip.length !== pin.size || sha256Bytes(zip) !== listed) throw new ReleaseError(`${zipPath} is no longer the file ${SUMS_ASSET} lists`)
    unpackZip(zip, partial)
    const root = engineRoot(partial)
    const manifest = readManifest(root)
    const problems = [...referenceManifestProblems(manifest, pin), ...verifyStaged(root, manifest)]
    if (problems.length > 0) throw new Error(`the unpacked ${pin.asset} is not the engine its manifest describes: ${problems.slice(0, 10).join('; ')}`)
    rmSync(record, { force: true })
    rmSync(dir, { recursive: true, force: true })
    renameSync(root, dir)
    const rec: ReferenceRecord = { format: 'sketch-orca-parity-reference/1', version: pin.version, tag: pin.tag, repo, asset: pin.asset, size: pin.size, sha256: listed, url: zipAsset.url, unpackedAt: new Date().toISOString() }
    writeFileSync(record, JSON.stringify(rec, null, 2) + '\n')
    o.log(`${pin.version} unpacked to ${dir}: ${manifest.files.length} files, each with the size and SHA-256 its manifest lists (stage version ${manifest.stageVersion}, engine ${manifest.engine.version}, OrcaSlicer ${manifest.orca.tag})`)
    return { dir, manifest, kept: false }
  } finally {
    rmSync(partial, { recursive: true, force: true })
  }
}
