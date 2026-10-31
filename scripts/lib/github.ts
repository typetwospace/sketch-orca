// GitHub's REST answers for "latest stable release" and "which commit is this tag".
// Parsing is kept apart from the network so the tests run on fixture JSON.

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
