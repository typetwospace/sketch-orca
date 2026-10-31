// The Orca checkout: a shallow clone of the locked tag, checked against the locked commit, with
// our patches applied in order. Re-running is cheap: a checkout at the right commit is kept, and
// patches already applied (same names, same bytes) are not applied again.

import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import type { OrcaLock } from './lock.ts'
import { sha256File } from './paths.ts'
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

export interface AppliedState {
  commit: string
  applied: { name: string; sha256: string }[]
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
    await runLogged(git, ['-C', src, 'clean', '-fdq'], { log })
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

/** Applies our patches to the checkout (see planPatches). Returns the hash list for stamps. */
export async function applyPatches(git: string, lock: OrcaLock, src: string, patchesDir: string, stateDir: string, log: Log): Promise<PatchInfo[]> {
  verifyCommit(git, lock, src)
  const wanted = listPatches(patchesDir)
  const stateFile = join(stateDir, 'patches.json')
  const plan = planPatches(readApplied(stateFile), lock.commit, wanted)
  if (plan.reset) {
    const dirty = captureOk(git, ['-C', src, 'status', '--porcelain', '--untracked-files=no']).trim()
    if (dirty !== '') log.line('patches: resetting the checkout to the locked commit before applying')
    await runLogged(git, ['-C', src, 'reset', '--hard', '--quiet', lock.commit], { log })
    // Files a previous patch created are untracked now; remove them so it applies again.
    await runLogged(git, ['-C', src, 'clean', '-fdq'], { log })
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
  return wanted
}
