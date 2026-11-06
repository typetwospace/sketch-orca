// GitHub's REST answers for "latest stable release", "which commit is this tag" and "the release of
// this tag and its assets", and the download of one release asset (downloadReleaseAsset), checked
// against its listed size and a SHA-256 while it streams. Parsing is kept apart from the network so
// the tests run on fixture JSON and a fake fetch.

import { createHash } from 'node:crypto'
import { closeSync, mkdirSync, openSync, renameSync, rmSync, writeSync } from 'node:fs'
import { dirname } from 'node:path'
import type { ReleaseRef } from './lock.ts'

export class ReleaseError extends Error {}

export interface LatestRelease {
  tag: string
  name: string
  publishedAt: string
  htmlUrl: string
}

function obj(value: unknown, what: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new ReleaseError(`${what}: not a JSON object`)
  return value as Record<string, unknown>
}

/**
 * `GET /repos/{repo}/releases/latest`. GitHub already skips drafts and pre-releases there;
 * a draft or pre-release in the answer is refused anyway.
 */
export function parseLatestRelease(value: unknown): LatestRelease {
  const o = obj(value, 'release')
  if (typeof o.message === 'string' && o.tag_name === undefined) throw new ReleaseError(`release: GitHub answered "${o.message}"`)
  if (typeof o.tag_name !== 'string' || o.tag_name === '') throw new ReleaseError('release: no tag_name')
  if (o.draft === true) throw new ReleaseError(`release ${o.tag_name} is a draft`)
  if (o.prerelease === true) throw new ReleaseError(`release ${o.tag_name} is a pre-release`)
  return {
    tag: o.tag_name,
    name: typeof o.name === 'string' ? o.name : '',
    publishedAt: typeof o.published_at === 'string' ? o.published_at : '',
    htmlUrl: typeof o.html_url === 'string' ? o.html_url : '',
  }
}

export interface GitObjectRef {
  type: 'commit' | 'tag'
  sha: string
}

const SHA_RE = /^[0-9a-f]{40}$/

/** `GET /repos/{repo}/git/ref/tags/{tag}`: a lightweight tag points at a commit, an annotated one at a tag object. */
export function parseTagRef(value: unknown, tag: string): GitObjectRef {
  if (Array.isArray(value)) throw new ReleaseError(`ref tags/${tag}: ambiguous (several refs)`)
  const o = obj(value, `ref tags/${tag}`)
  if (o.ref !== `refs/tags/${tag}`) throw new ReleaseError(`ref tags/${tag}: answer is for ${JSON.stringify(o.ref)}`)
  const target = obj(o.object, `ref tags/${tag} object`)
  const type = target.type
  const sha = target.sha
  if (type !== 'commit' && type !== 'tag') throw new ReleaseError(`ref tags/${tag}: unexpected object type ${JSON.stringify(type)}`)
  if (typeof sha !== 'string' || !SHA_RE.test(sha)) throw new ReleaseError(`ref tags/${tag}: bad sha ${JSON.stringify(sha)}`)
  return { type, sha }
}

/** `GET /repos/{repo}/git/tags/{sha}`: the annotated tag object; returns what it points at. */
export function parseTagObject(value: unknown): GitObjectRef {
  const o = obj(value, 'tag object')
  const target = obj(o.object, 'tag object target')
  const type = target.type
  const sha = target.sha
  if (type !== 'commit' && type !== 'tag') throw new ReleaseError(`tag object: unexpected target type ${JSON.stringify(type)}`)
  if (typeof sha !== 'string' || !SHA_RE.test(sha)) throw new ReleaseError(`tag object: bad sha ${JSON.stringify(sha)}`)
  return { type, sha }
}

export type JsonGetter = (url: string) => Promise<unknown>

/** Latest stable release and the commit its tag points to (annotated tags are followed, at most 4 levels). */
export async function resolveLatestRelease(repo: string, get: JsonGetter): Promise<ReleaseRef & { name: string; htmlUrl: string }> {
  const api = `https://api.github.com/repos/${repo}`
  const release = parseLatestRelease(await get(`${api}/releases/latest`))
  let ref = parseTagRef(await get(`${api}/git/ref/tags/${encodeURIComponent(release.tag)}`), release.tag)
  for (let depth = 0; ref.type === 'tag'; depth++) {
    if (depth >= 4) throw new ReleaseError(`tag ${release.tag}: too many nested tag objects`)
    ref = parseTagObject(await get(`${api}/git/tags/${ref.sha}`))
  }
  return { tag: release.tag, commit: ref.sha, publishedAt: release.publishedAt, name: release.name, htmlUrl: release.htmlUrl }
}

/** One asset of a release, as `GET /repos/{repo}/releases/tags/{tag}` lists it. */
export interface ReleaseAsset {
  name: string
  /** Bytes, as the release lists it. */
  size: number
  /** browser_download_url: https://github.com/<repo>/releases/download/<tag>/<name>. */
  url: string
  /** The SHA-256 GitHub computed on upload (its `digest`, "sha256:<hex>"), when the answer has one. */
  sha256?: string
}

export interface TaggedRelease {
  tag: string
  name: string
  publishedAt: string
  /** What the tag was made from (target_commitish), as GitHub gives it. */
  commitish: string
  assets: ReleaseAsset[]
}

const HEX64_RE = /^[0-9a-f]{64}$/

/**
 * `GET /repos/{repo}/releases/tags/{tag}`: a published release (no draft, no pre-release) of that
 * tag, and its uploaded assets. An asset still uploading, or one whose download address is not
 * https://github.com/, is refused.
 */
export function parseTaggedRelease(value: unknown, tag: string): TaggedRelease {
  const o = obj(value, `release ${tag}`)
  if (typeof o.message === 'string' && o.tag_name === undefined) throw new ReleaseError(`release ${tag}: GitHub answered "${o.message}"`)
  if (o.tag_name !== tag) throw new ReleaseError(`release ${tag}: answer is for ${JSON.stringify(o.tag_name)}`)
  if (o.draft === true) throw new ReleaseError(`release ${tag} is a draft`)
  if (o.prerelease === true) throw new ReleaseError(`release ${tag} is a pre-release`)
  if (!Array.isArray(o.assets)) throw new ReleaseError(`release ${tag}: no asset list`)
  const assets = o.assets.map((v, i): ReleaseAsset => {
    const a = obj(v, `release ${tag} asset ${i}`)
    if (typeof a.name !== 'string' || a.name === '') throw new ReleaseError(`release ${tag} asset ${i}: no name`)
    if (typeof a.size !== 'number' || !Number.isSafeInteger(a.size) || a.size < 0) throw new ReleaseError(`release ${tag} asset ${a.name}: bad size ${JSON.stringify(a.size)}`)
    if (a.state !== undefined && a.state !== 'uploaded') throw new ReleaseError(`release ${tag} asset ${a.name} is ${JSON.stringify(a.state)}, not uploaded`)
    if (typeof a.browser_download_url !== 'string') throw new ReleaseError(`release ${tag} asset ${a.name}: no download address`)
    const url = a.browser_download_url
    let parsed: URL
    try {
      parsed = new URL(url)
    } catch {
      throw new ReleaseError(`release ${tag} asset ${a.name}: bad download address ${JSON.stringify(url)}`)
    }
    if (parsed.protocol !== 'https:' || parsed.hostname !== 'github.com') throw new ReleaseError(`release ${tag} asset ${a.name}: download address ${url} is not on https://github.com/`)
    let sha256: string | undefined
    if (typeof a.digest === 'string') {
      const m = /^sha256:([0-9a-f]{64})$/.exec(a.digest)
      if (m) sha256 = m[1]
    }
    return { name: a.name, size: a.size, url, ...(sha256 ? { sha256 } : {}) }
  })
  return {
    tag,
    name: typeof o.name === 'string' ? o.name : '',
    publishedAt: typeof o.published_at === 'string' ? o.published_at : '',
    commitish: typeof o.target_commitish === 'string' ? o.target_commitish : '',
    assets,
  }
}

/** The release of `tag` in `repo`, with its assets. */
export async function resolveTaggedRelease(repo: string, tag: string, get: JsonGetter): Promise<TaggedRelease> {
  return parseTaggedRelease(await get(`https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`), tag)
}

/** The asset of a release named `name`; throws when the release has none, or two. */
export function releaseAsset(release: TaggedRelease, name: string): ReleaseAsset {
  const found = release.assets.filter((a) => a.name === name)
  if (found.length !== 1) throw new ReleaseError(`release ${release.tag} has ${found.length === 0 ? 'no' : 'more than one'} asset ${name}`)
  return found[0]!
}

/**
 * Where a release download may go: GitHub itself, then the storage hosts it redirects downloads to
 * (objects.githubusercontent.com, release-assets.githubusercontent.com), over https only.
 */
export function isGithubDownloadUrl(url: string): boolean {
  let u: URL
  try {
    u = new URL(url)
  } catch {
    return false
  }
  return u.protocol === 'https:' && u.username === '' && u.password === '' && (u.hostname === 'github.com' || u.hostname.endsWith('.githubusercontent.com'))
}

/** The part of fetch a download uses; tests hand in a fake. */
export type FetchLike = (url: string, init: { redirect: 'manual'; headers: Record<string, string> }) => Promise<Response>

export interface DownloadOptions {
  /**
   * The SHA-256 the file must have (from SHA256SUMS). Without it, the one GitHub lists for the asset
   * (ReleaseAsset.sha256); with both, they must agree. A download with neither is refused.
   */
  sha256?: string
  fetch?: FetchLike
  /** Redirects followed at most (default 5). */
  maxRedirects?: number
}

export interface Downloaded {
  path: string
  size: number
  sha256: string
  /** The address the bytes came from, after the redirects. */
  from: string
}

/**
 * Downloads a release asset to `dest`. Redirects are followed by hand (at most maxRedirects), each
 * to https on github.com or a githubusercontent.com host only. The bytes go to a temporary name next
 * to `dest` while they are counted and hashed; the download stops as soon as it passes the size
 * the release lists, and is refused when it ends at another size or with another SHA-256. Only a
 * download that passes both is renamed to `dest`, so `dest` is never a partial or unchecked file.
 */
export async function downloadReleaseAsset(asset: ReleaseAsset, dest: string, o: DownloadOptions = {}): Promise<Downloaded> {
  const want = (o.sha256 ?? asset.sha256)?.toLowerCase()
  if (!want || !HEX64_RE.test(want)) throw new ReleaseError(`${asset.name}: no SHA-256 to check the download against`)
  if (asset.sha256 && asset.sha256 !== want) throw new ReleaseError(`${asset.name}: GitHub lists SHA-256 ${asset.sha256}, expected ${want}`)
  const fetchFn: FetchLike = o.fetch ?? ((url, init) => fetch(url, init))
  const maxRedirects = o.maxRedirects ?? 5
  const headers = { 'User-Agent': 'sketch-orca-engine-pipeline', Accept: 'application/octet-stream' }
  let url = asset.url
  let res: Response
  for (let hop = 0; ; hop++) {
    if (!isGithubDownloadUrl(url)) throw new ReleaseError(`${asset.name}: refusing to download from ${url} (only https on github.com and githubusercontent.com)`)
    res = await fetchFn(url, { redirect: 'manual', headers })
    if (res.status >= 300 && res.status < 400) {
      const location = res.headers.get('location')
      await res.body?.cancel()
      if (!location) throw new ReleaseError(`${asset.name}: HTTP ${res.status} from ${url} without a Location`)
      if (hop >= maxRedirects) throw new ReleaseError(`${asset.name}: more than ${maxRedirects} redirects`)
      url = new URL(location, url).toString()
      continue
    }
    break
  }
  if (!res.ok) {
    await res.body?.cancel()
    throw new ReleaseError(`${asset.name}: HTTP ${res.status} from ${url}`)
  }
  const length = res.headers.get('content-length')
  // A length on an encoded answer counts the encoded bytes; the count while reading still holds.
  if (length !== null && res.headers.get('content-encoding') === null && Number(length) !== asset.size) {
    await res.body?.cancel()
    throw new ReleaseError(`${asset.name}: the server sends ${length} bytes, the release lists ${asset.size}`)
  }
  if (!res.body) throw new ReleaseError(`${asset.name}: empty answer from ${url}`)
  mkdirSync(dirname(dest), { recursive: true })
  const tmp = `${dest}.partial-${process.pid}`
  const hash = createHash('sha256')
  let size = 0
  let fd: number | undefined = openSync(tmp, 'w')
  const reader = res.body.getReader()
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      size += value.length
      if (size > asset.size) throw new ReleaseError(`${asset.name}: the download passes the ${asset.size} bytes the release lists`)
      hash.update(value)
      writeSync(fd, value)
    }
    closeSync(fd)
    fd = undefined
    if (size !== asset.size) throw new ReleaseError(`${asset.name}: downloaded ${size} bytes, the release lists ${asset.size}`)
    const got = hash.digest('hex')
    if (got !== want) throw new ReleaseError(`${asset.name}: SHA-256 ${got}, expected ${want}`)
    renameSync(tmp, dest)
    return { path: dest, size, sha256: got, from: url }
  } catch (err) {
    await reader.cancel().catch(() => undefined)
    if (fd !== undefined) closeSync(fd)
    rmSync(tmp, { force: true })
    throw err
  }
}

/** A JsonGetter on the global fetch; GITHUB_TOKEN is sent when set (a higher rate limit). */
export function githubGetter(token: string | undefined = process.env.GITHUB_TOKEN): JsonGetter {
  return async (url: string) => {
    const headers: Record<string, string> = {
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      'User-Agent': 'sketch-orca-engine-pipeline',
    }
    if (token) headers.Authorization = `Bearer ${token}`
    const res = await fetch(url, { headers })
    const text = await res.text()
    if (!res.ok) throw new ReleaseError(`GET ${url}: HTTP ${res.status} ${text.slice(0, 200)}`)
    try {
      return JSON.parse(text) as unknown
    } catch {
      throw new ReleaseError(`GET ${url}: answer is not JSON`)
    }
  }
}
