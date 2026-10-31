// The engine lock: which OrcaSlicer release the engine is built from.
// `orca.lock.json` pins the tag and the commit it points to; `fetch` refuses a
// checkout whose HEAD is not that commit.

import { readFileSync, writeFileSync } from 'node:fs'

export const LOCK_FORMAT = 'sketch-orca-lock/1'

export interface OrcaLock {
  format: typeof LOCK_FORMAT
  /** GitHub `owner/name` of the upstream repository. */
  repo: string
  /** Release tag, such as `v2.4.2`. */
  tag: string
  /** Full 40-hex commit the tag points to. */
  commit: string
  /** Version without the leading `v`, such as `2.4.2`. */
  version: string
  /** When GitHub published the release (ISO 8601), or '' when unknown. */
  publishedAt: string
  /** Upstream licence, recorded for the source bundle. */
  license: string
}

export class LockError extends Error {}

const COMMIT_RE = /^[0-9a-f]{40}$/
const REPO_RE = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/
const TAG_RE = /^[A-Za-z0-9_.\-+]+$/

/** `v2.4.2` -> `2.4.2`; a tag without a leading `v` is its own version. */
export function versionFromTag(tag: string): string {
  return /^v\d/.test(tag) ? tag.slice(1) : tag
}

/** Checks a parsed lock object and returns it typed, or throws a LockError naming the bad field. */
export function validateLock(value: unknown): OrcaLock {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new LockError('lock: not a JSON object')
  const o = value as Record<string, unknown>
  if (o.format !== LOCK_FORMAT) throw new LockError(`lock: format must be "${LOCK_FORMAT}", got ${JSON.stringify(o.format)}`)
  const str = (key: string): string => {
    const v = o[key]
    if (typeof v !== 'string') throw new LockError(`lock: "${key}" must be a string`)
    return v
  }
  const repo = str('repo')
  if (!REPO_RE.test(repo)) throw new LockError(`lock: "repo" must be owner/name, got "${repo}"`)
  const tag = str('tag')
  if (!TAG_RE.test(tag)) throw new LockError(`lock: "tag" has unexpected characters: "${tag}"`)
  const commit = str('commit')
  if (!COMMIT_RE.test(commit)) throw new LockError(`lock: "commit" must be 40 lowercase hex characters, got "${commit}"`)
  const version = str('version')
  if (version !== versionFromTag(tag)) throw new LockError(`lock: "version" ${version} does not match tag ${tag}`)
  const publishedAt = str('publishedAt')
  if (publishedAt !== '' && Number.isNaN(Date.parse(publishedAt))) throw new LockError(`lock: "publishedAt" is not a date: "${publishedAt}"`)
  const license = str('license')
  return { format: LOCK_FORMAT, repo, tag, commit, version, publishedAt, license }
}

export function parseLock(text: string): OrcaLock {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch (err) {
    throw new LockError(`lock: invalid JSON (${(err as Error).message})`)
  }
  return validateLock(value)
}

/** Stable key order, two-space indent, LF, trailing newline. */
export function serializeLock(lock: OrcaLock): string {
  const ordered: OrcaLock = {
    format: LOCK_FORMAT,
    repo: lock.repo,
    tag: lock.tag,
    commit: lock.commit,
    version: lock.version,
    publishedAt: lock.publishedAt,
    license: lock.license,
  }
  return JSON.stringify(ordered, null, 2) + '\n'
}

export function readLock(path: string): OrcaLock {
  return parseLock(readFileSync(path, 'utf8'))
}

export function writeLock(path: string, lock: OrcaLock): void {
  writeFileSync(path, serializeLock(validateLock(lock)))
}

export interface ReleaseRef {
  tag: string
  commit: string
  publishedAt: string
}

/** The lock moved to another release; repo and licence are kept. */
export function lockForRelease(previous: OrcaLock, release: ReleaseRef): OrcaLock {
  return validateLock({
    ...previous,
    tag: release.tag,
    commit: release.commit,
    version: versionFromTag(release.tag),
    publishedAt: release.publishedAt,
  })
}

export type LockComparison = 'same' | 'newer-release' | 'older-release' | 'retagged'

/**
 * `same`: the lock is at the release; `retagged`: same tag, another commit (upstream moved the
 * tag); `newer-release`: the release's version is above the lock's; `older-release`: it is not
 * (the lock was moved ahead by hand, or upstream withdrew a release).
 */
export function compareLock(lock: OrcaLock, release: ReleaseRef): LockComparison {
  if (lock.tag === release.tag) return lock.commit === release.commit ? 'same' : 'retagged'
  return compareVersions(versionFromTag(release.tag), lock.version) > 0 ? 'newer-release' : 'older-release'
}

export interface ReleaseDecision {
  /** Move the lock to the release before building. */
  update: boolean
  /** Go on to fetch, build, smoke, stage and source. */
  proceed: boolean
  /** The exit code of `check`: 0 nothing to do, 1 a newer release is out, 2 the tag moved. */
  checkCode: number
  message: string
}

/** What `check` reports and `release` does for a comparison of the lock with the latest stable release. */
export function releaseDecision(cmp: LockComparison, lock: OrcaLock, release: ReleaseRef): ReleaseDecision {
  switch (cmp) {
    case 'same':
      return { update: false, proceed: true, checkCode: 0, message: `the lock is at the latest stable release ${lock.tag}` }
    case 'newer-release':
      return { update: true, proceed: true, checkCode: 1, message: `a newer stable release is out: ${release.tag} (the lock is at ${lock.tag})` }
    case 'older-release':
      return { update: false, proceed: true, checkCode: 0, message: `the lock (${lock.tag}) is ahead of the latest stable release ${release.tag}; it is kept` }
    case 'retagged':
      return {
        update: false,
        proceed: false,
        checkCode: 2,
        message: `${release.tag} now points at ${release.commit}, not the locked ${lock.commit}. Upstream moved the tag; review it before moving the lock.`,
      }
  }
}

/** Numeric comparison of dotted versions (`2.4.10` > `2.4.9`); non-numeric parts compare as strings. */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(/[.\-+]/)
  const pb = b.split(/[.\-+]/)
  const n = Math.max(pa.length, pb.length)
  for (let i = 0; i < n; i++) {
    const x = pa[i] ?? '0'
    const y = pb[i] ?? '0'
    const nx = /^\d+$/.test(x) ? Number(x) : NaN
    const ny = /^\d+$/.test(y) ? Number(y) : NaN
    if (!Number.isNaN(nx) && !Number.isNaN(ny)) {
      if (nx !== ny) return nx < ny ? -1 : 1
    } else if (x !== y) {
      return x < y ? -1 : 1
    }
  }
  return 0
}
