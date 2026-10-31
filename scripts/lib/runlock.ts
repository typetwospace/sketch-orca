// One pipeline run at a time in a work folder: the commands that write there (update, fetch,
// build, smoke, stage, source and release) hold <work>/run.lock while they run. Two runs would
// otherwise reset the same checkout, rename the same dist/engine and overwrite each other's
// stamps. A lock whose process is gone (a run that was killed) is taken over.

import { closeSync, existsSync, mkdirSync, openSync, readFileSync, rmSync, statSync, writeSync } from 'node:fs'
import { join } from 'node:path'
import { StepError } from './run.ts'

export const RUN_LOCK_FILE = 'run.lock'

export interface RunLockInfo {
  pid: number
  command: string
  startedAt: string
}

/** Whether a process with this id is running; EPERM means it runs under another user. */
export function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch (err) {
    return (err as NodeJS.ErrnoException).code === 'EPERM'
  }
}

export function readRunLock(path: string): RunLockInfo | undefined {
  try {
    const v = JSON.parse(readFileSync(path, 'utf8')) as Partial<RunLockInfo>
    if (typeof v.pid === 'number' && Number.isInteger(v.pid) && v.pid > 0) return { pid: v.pid, command: String(v.command ?? '?'), startedAt: String(v.startedAt ?? '?') }
  } catch {
    // Missing, or not written yet.
  }
  return undefined
}

export interface RunLockOptions {
  pid?: number
  alive?: (pid: number) => boolean
  now?: () => number
}

/**
 * Takes `<home>/run.lock` for `command`, or throws a StepError naming the run that holds it. The
 * returned function gives the lock back (only if this run still holds it). A lock file that cannot
 * be read is a run that is just starting while it is younger than 10 s, and stale after that.
 */
export function takeRunLock(home: string, command: string, opts: RunLockOptions = {}): () => void {
  const pid = opts.pid ?? process.pid
  const alive = opts.alive ?? pidAlive
  const now = opts.now ?? Date.now
  mkdirSync(home, { recursive: true })
  const path = join(home, RUN_LOCK_FILE)
  for (let attempt = 0; attempt < 3; attempt++) {
    let fd: number
    try {
      fd = openSync(path, 'wx')
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code !== 'EEXIST') throw err
      const held = readRunLock(path)
      if (held && held.pid !== pid && alive(held.pid)) {
        throw new StepError(`another pipeline run (${held.command}, process ${held.pid}, started ${held.startedAt}) is using ${home}; wait for it to finish (if no such run is going, delete ${path})`)
      }
      if (!held) {
        let age = Infinity
        try {
          age = now() - statSync(path).mtimeMs
        } catch {
          // Gone in the meantime: try again.
        }
        if (age < 10_000) throw new StepError(`another pipeline run is starting in ${home} (${path} was just created); wait for it to finish`)
      }
      rmSync(path, { force: true })
      continue
    }
    try {
      writeSync(fd, JSON.stringify({ pid, command, startedAt: new Date(now()).toISOString() } satisfies RunLockInfo) + '\n')
    } finally {
      closeSync(fd)
    }
    let released = false
    return () => {
      if (released) return
      released = true
      if (existsSync(path) && readRunLock(path)?.pid === pid) rmSync(path, { force: true })
    }
  }
  throw new StepError(`cannot take ${path}`)
}
