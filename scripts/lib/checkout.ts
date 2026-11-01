// The Orca checkout: a shallow clone of the locked tag, checked against the locked commit, with
// our patches applied in order. Re-running is cheap: a checkout at the right commit is kept, and
// patches already applied (same names, same bytes) are not applied again, as long as the checkout
// is exactly the commit with them applied (sourceDigest, patchedDigest).

import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, dirname } from 'node:path'
import type { OrcaLock } from './lock.ts'
import { sha256File, sha256Text } from './paths.ts'
import { capture, captureOk, Log, runLogged, StepError } from './run.ts'

export interface PatchInfo {
  name: string
  path: string
  sha256: string
}

/** `patches/*.patch`, in name order (the numeric prefix sets the order). */
export function listPatches(dir: string): PatchInfo[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir)
    .filter((n) => n.endsWith('.patch'))
    .sort()
    .map((name) => ({ name, path: join(dir, name), sha256: sha256File(join(dir, name)) }))
}

/** Repo paths a patch changes, from its `diff --git a/<path> b/<path>` lines. */
export function patchedPaths(patchText: string): string[] {
  const out: string[] = []
  for (const m of patchText.matchAll(/^diff --git a\/(\S+) b\/(\S+)$/gm)) out.push(m[2]!)
  return out
}

/** The patches that change deps/, in order: the dependency build depends only on them. */
export function depsPatchesOf(patches: PatchInfo[]): PatchInfo[] {
  return patches.filter((p) => patchedPaths(readFileSync(p.path, 'utf8')).some((f) => f.startsWith('deps/')))
}

export interface AppliedState {
  commit: string
  applied: { name: string; sha256: string }[]
}

/** The paths of git's `--name-status -z` output, with the files it removes apart. */
function nameStatus(out: string): { present: string[]; removed: string[] } {
  const fields = out.split('\0')
  const present: string[] = []
  const removed: string[] = []
  for (let i = 0; i + 1 < fields.length; i += 2) (fields[i] === 'D' ? removed : present).push(fields[i + 1]!)
  return { present, removed }
}

/** One digest of the files that differ from a commit: path and git blob, '-' for a removed file, in path order. */
function digestOf(entries: [string, string][]): string {
  const sorted = [...entries].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
  return sha256Text(sorted.map(([path, blob]) => `${path}\0${blob}\n`).join(''))
}

function gitBase(src: string): string[] {
  return ['--no-optional-locks', '--literal-pathspecs', '-C', src, '-c', 'core.quotePath=false']
}

/**
 * What the checkout's files are against `commit`: every file that differs from it (an edit, staged
 * or not) or that git does not track (the files a patch adds, and any other, .gitignore's too: the
 * build reads what is in the checkout whatever git ignores), by the git blob of its content, and
 * every file removed. patchedDigest gives the same digest for the commit with the patches applied.
 */
export function sourceDigest(git: string, src: string, commit: string): string {
  const base = gitBase(src)
  const { present, removed } = nameStatus(captureOk(git, [...base, 'diff', '--name-status', '-z', '--no-renames', '--no-ext-diff', commit, '--']))
  const untracked = captureOk(git, [...base, 'ls-files', '-z', '--others']).split('\0').filter((rel) => rel !== '')
  const paths = [...present, ...untracked]
  const blobs = paths.length === 0 ? [] : captureOk(git, [...base, 'hash-object', '--stdin-paths'], { input: paths.join('\n') + '\n' }).trim().split(/\r?\n/)
  if (blobs.length !== paths.length) throw new StepError(`git hash-object gave ${blobs.length} hashes for ${paths.length} files in ${src}`)
  return digestOf([...paths.map((p, i): [string, string] => [p, blobs[i]!]), ...removed.map((p): [string, string] => [p, '-'])])
}

/**
 * The sourceDigest of `commit` with `patches` applied in order and nothing else, worked out in a
 * scratch index: the checkout's files are not touched.
 */
export function patchedDigest(git: string, src: string, commit: string, patches: { path: string }[]): string {
  const base = gitBase(src)
  const scratch = mkdtempSync(join(tmpdir(), 'sketch-orca-index-'))
  try {
    const env: Record<string, string> = {}
    for (const [k, v] of Object.entries(process.env)) if (v !== undefined) env[k] = v
    env.GIT_INDEX_FILE = join(scratch, 'index')
    captureOk(git, [...base, 'read-tree', commit], { env })
    for (const p of patches) captureOk(git, [...base, 'apply', '--cached', '--whitespace=nowarn', p.path], { env })
    const { present, removed } = nameStatus(captureOk(git, [...base, 'diff', '--cached', '--name-status', '-z', '--no-renames', commit, '--'], { env }))
    const blobs = new Map<string, string>()
    for (const entry of captureOk(git, [...base, 'ls-files', '-s', '-z'], { env }).split('\0')) {
      const m = /^\d+ ([0-9a-f]+) \d+\t(.*)$/s.exec(entry)
      if (m) blobs.set(m[2]!, m[1]!)
    }
    return digestOf([...present.map((p): [string, string] => [p, blobs.get(p) ?? '?']), ...removed.map((p): [string, string] => [p, '-'])])
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

/**
 * What to do to get from the applied patches to the wanted ones: apply the rest when the applied
 * list is a prefix of the wanted list for the same commit, otherwise reset and apply all.
 */
export function planPatches(state: AppliedState | undefined, commit: string, wanted: PatchInfo[]): { reset: boolean; apply: PatchInfo[] } {
  if (!state || state.commit !== commit || state.applied.length > wanted.length) return { reset: true, apply: wanted }
  for (let i = 0; i < state.applied.length; i++) {
    const a = state.applied[i]!
    const w = wanted[i]!
    if (a.name !== w.name || a.sha256 !== w.sha256) return { reset: true, apply: wanted }
  }
  return { reset: false, apply: wanted.slice(state.applied.length) }
}

export function repoUrl(lock: OrcaLock): string {
  return `https://github.com/${lock.repo}.git`
}

export function headCommit(git: string, src: string): string | undefined {
  if (!existsSync(join(src, '.git'))) return undefined
  const r = capture(git, ['-C', src, 'rev-parse', 'HEAD^{commit}'])
  return r.code === 0 ? r.stdout.trim() : undefined
}

/** Clones the locked tag (depth 1) unless the checkout is already at the locked commit; verifies the commit. */
export async function fetchSource(git: string, lock: OrcaLock, src: string, log: Log): Promise<'kept' | 'cloned' | 'moved'> {
  const head = headCommit(git, src)
  if (head === lock.commit) {
    const origin = capture(git, ['-C', src, 'remote', 'get-url', 'origin']).stdout.trim()
    if (origin !== repoUrl(lock)) throw new StepError(`${src} is a checkout of ${origin}, not ${repoUrl(lock)}; remove it and fetch again`)
    log.line(`fetch: ${src} is already at ${lock.tag} (${lock.commit})`)
    return 'kept'
  }
  if (head !== undefined) {
    // A checkout of another release: fetch the locked tag into it.
    log.line(`fetch: ${src} is at ${head}; fetching ${lock.tag}`)
    await runLogged(git, ['-C', src, 'fetch', '--depth', '1', '--no-tags', 'origin', `+refs/tags/${lock.tag}:refs/tags/${lock.tag}`], { log })
    await runLogged(git, ['-C', src, 'reset', '--hard', '--quiet'], { log })
    await runLogged(git, ['-C', src, 'clean', '-fdxq'], { log })
    await runLogged(git, ['-C', src, 'checkout', '--quiet', '--detach', `refs/tags/${lock.tag}`], { log })
    verifyCommit(git, lock, src)
    return 'moved'
  }
  if (existsSync(src) && readdirSync(src).length > 0) throw new StepError(`${src} exists but is not a git checkout; remove it and fetch again`)
  mkdirSync(dirname(src), { recursive: true })
  log.line(`fetch: cloning ${repoUrl(lock)} at ${lock.tag}`)
  await runLogged(
    git,
    [
      '-c', 'core.autocrlf=false',
      '-c', 'core.longpaths=true',
      'clone', '--depth', '1', '--branch', lock.tag, '--no-tags',
      '--config', 'core.autocrlf=false',
      '--config', 'core.longpaths=true',
      '--config', 'core.eol=lf',
      '--config', 'advice.detachedHead=false',
      repoUrl(lock), src,
    ],
    { log },
  )
  verifyCommit(git, lock, src)
  return 'cloned'
}

export function verifyCommit(git: string, lock: OrcaLock, src: string): void {
  const head = headCommit(git, src)
  if (head !== lock.commit) {
    throw new StepError(`${lock.tag} checked out at ${head ?? '(nothing)'}, but the lock says ${lock.commit}. The tag moved upstream or the lock is wrong; refusing to build.`)
  }
}

function readApplied(stateFile: string): AppliedState | undefined {
  if (!existsSync(stateFile)) return undefined
  try {
    return JSON.parse(readFileSync(stateFile, 'utf8')) as AppliedState
  } catch {
    return undefined
  }
}

/**
 * Applies our patches to the checkout (see planPatches). The patches on record as applied count
 * only while the checkout is exactly the commit with them applied (sourceDigest against
 * patchedDigest): an edit, an added or a removed file resets it, and every patch is applied again.
 * Then it must be the commit with all of patches/ applied. Returns the hash list for stamps.
 */
export async function applyPatches(git: string, lock: OrcaLock, src: string, patchesDir: string, stateDir: string, log: Log): Promise<PatchInfo[]> {
  verifyCommit(git, lock, src)
  const wanted = listPatches(patchesDir)
  const stateFile = join(stateDir, 'patches.json')
  let plan = planPatches(readApplied(stateFile), lock.commit, wanted)
  if (!plan.reset && sourceDigest(git, src, lock.commit) !== patchedDigest(git, src, lock.commit, wanted.slice(0, wanted.length - plan.apply.length))) {
    log.line('patches: the checkout is not the locked commit with the recorded patches applied (a file edited, added or removed); resetting it')
    plan = { reset: true, apply: wanted }
  }
  if (plan.reset) {
    const dirty = captureOk(git, ['-C', src, 'status', '--porcelain', '--untracked-files=no']).trim()
    if (dirty !== '') log.line('patches: resetting the checkout to the locked commit before applying')
    await runLogged(git, ['-C', src, 'reset', '--hard', '--quiet', lock.commit], { log })
    // Files a previous patch created are untracked now; remove them so it applies again, and the
    // files .gitignore leaves out with them (sourceDigest counts them).
    await runLogged(git, ['-C', src, 'clean', '-fdxq'], { log })
  }
  const applied: AppliedState = { commit: lock.commit, applied: plan.reset ? [] : (readApplied(stateFile)?.applied ?? []) }
  for (const p of plan.apply) {
    await runLogged(git, ['-C', src, 'apply', '--whitespace=nowarn', p.path], { log })
    applied.applied.push({ name: p.name, sha256: p.sha256 })
    mkdirSync(stateDir, { recursive: true })
    writeFileSync(stateFile, JSON.stringify(applied, null, 2) + '\n')
  }
  if (plan.apply.length === 0) log.line(`patches: ${wanted.length} already applied`)
  else log.line(`patches: applied ${plan.apply.map((p) => p.name).join(', ')}`)
  if (wanted.length === 0) {
    mkdirSync(stateDir, { recursive: true })
    writeFileSync(stateFile, JSON.stringify(applied, null, 2) + '\n')
  }
  if (sourceDigest(git, src, lock.commit) !== patchedDigest(git, src, lock.commit, wanted)) {
    throw new StepError(`${src} is not the locked commit with patches/ applied, after applying them (a patch that does not apply the same way to the index?)`)
  }
  return wanted
}
