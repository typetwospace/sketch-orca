// One pipeline run at a time in a work folder: the commands that write there (update, fetch,
// build, smoke, stage, source and release) hold <work>/run.lock.d while they run. Two runs would
// otherwise reset the same checkout, rename the same dist/engine and overwrite each other's
// stamps. A lock whose process is gone (a run that was killed) is taken over.
//
// The lock is a numbered series of records (<n>.json), each created once, exclusively, and never
// written again: the highest names the run that holds the lock, or says it is free. A run takes the
// lock by creating the next number, so of two runs that both find the holder gone only one gets it,
// and no run removes the record of a run that may hold the lock (only lower numbers). It gives the
// lock back by creating the next number again, as free, which fails when another run has taken it
// over since.
//
// A record is published whole (sketch-orca, 2026-11-01): it is written to a file of the run's own
// first and then hard-linked as <n>.json, which fails when that number exists. So no run ever sees
// a record half written, and a record that cannot be read is never taken for a dead run's, however
// old it is: the lock stays held until someone removes it.

import { existsSync, linkSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import { join } from 'node:path'
import { StepError } from './run.ts'

export const RUN_LOCK_DIR = 'run.lock.d'
/** The lock file of earlier versions of the pipeline: a run of one that is still going holds the work folder too. */
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

/** A run's record: the run, 'free', or undefined when missing or not written yet. */
function readRecord(path: string): RunLockInfo | 'free' | undefined {
  try {
    const v = JSON.parse(readFileSync(path, 'utf8')) as Partial<RunLockInfo> & { free?: unknown }
    if (v.free === true) return 'free'
    if (typeof v.pid === 'number' && Number.isInteger(v.pid) && v.pid > 0) return { pid: v.pid, command: String(v.command ?? '?'), startedAt: String(v.startedAt ?? '?') }
  } catch {
    // Missing, or not written yet.
  }
  return undefined
}

/** The run a lock record or the earlier lock file names, if any. */
export function readRunLock(path: string): RunLockInfo | undefined {
  const r = readRecord(path)
  return r === 'free' ? undefined : r
}

/** The numbers of the records in `dir`, lowest first. */
function generations(dir: string): number[] {
  const out: number[] = []
  for (const name of readdirSync(dir)) {
    const m = /^(\d+)\.json$/.exec(name)
    if (m) out.push(Number(m[1]))
  }
  return out.sort((a, b) => a - b)
}

/**
 * Creates record `n`, whole: written to a file of this run's own, then linked as <n>.json (a link
 * is made at once, and not over a file that exists). False when record `n` exists. `beforePublish`
 * runs between the two (tests).
 */
function createRecord(dir: string, n: number, record: RunLockInfo | { free: true }, beforePublish?: (n: number) => void): boolean {
  const own = join(dir, `${process.pid}-${randomUUID()}.tmp`)
  writeFileSync(own, JSON.stringify(record) + '\n', { flag: 'wx' })
  try {
    beforePublish?.(n)
    linkSync(own, join(dir, `${n}.json`))
    return true
  } catch (err) {
    const code = (err as NodeJS.ErrnoException).code
    if (code === 'EEXIST') return false
    if (code === 'EPERM' || code === 'ENOTSUP' || code === 'ENOSYS') {
      throw new StepError(`cannot create the lock record ${join(dir, `${n}.json`)}: the file system of the work folder makes no hard links (${code}); use a work folder on one that does (SKETCH_ORCA_HOME)`)
    }
    throw err
  } finally {
    rmSync(own, { force: true })
  }
}

function removeBelow(dir: string, n: number): void {
  for (const g of generations(dir)) if (g < n) rmSync(join(dir, `${g}.json`), { force: true })
}

/** The run that holds the lock of `home`, if one does (it may have gone since). */
export function runLockHolder(home: string): RunLockInfo | undefined {
  const dir = join(home, RUN_LOCK_DIR)
  let gens: number[]
  try {
    gens = generations(dir)
  } catch {
    return undefined
  }
  return gens.length === 0 ? undefined : readRunLock(join(dir, `${gens[gens.length - 1]}.json`))
}

export interface RunLockOptions {
  pid?: number
  alive?: (pid: number) => boolean
  now?: () => number
  /** Runs when record `n` is written and not yet published (tests). */
  beforePublish?: (n: number) => void
}

function busy(held: RunLockInfo, home: string, path: string): StepError {
  return new StepError(`another pipeline run (${held.command}, process ${held.pid}, started ${held.startedAt}) is using ${home}; wait for it to finish (if no such run is going, delete ${path})`)
}

/**
 * Takes the lock of `home` (<home>/run.lock.d) for `command`, or throws a StepError naming the run
 * that holds it (or the run of an earlier pipeline that holds <home>/run.lock). The returned
 * function gives the lock back, unless another run has taken it over since. A record that cannot
 * be read (damaged, or being written by an earlier version of the pipeline, which did not publish
 * records whole) holds the lock however old it is.
 */
export function takeRunLock(home: string, command: string, opts: RunLockOptions = {}): () => void {
  const pid = opts.pid ?? process.pid
  const alive = opts.alive ?? pidAlive
  const now = opts.now ?? Date.now
  const dir = join(home, RUN_LOCK_DIR)
  mkdirSync(dir, { recursive: true })
  const legacy = readRunLock(join(home, RUN_LOCK_FILE))
  if (legacy && legacy.pid !== pid && alive(legacy.pid)) throw busy(legacy, home, join(home, RUN_LOCK_FILE))
  for (let attempt = 0; attempt < 5; attempt++) {
    const gens = generations(dir)
    const top = gens.length > 0 ? gens[gens.length - 1]! : 0
    if (gens.length > 0) {
      const path = join(dir, `${top}.json`)
      const held = readRecord(path)
      if (held === undefined) {
        // Removed in the meantime (a higher number came): look again.
        if (!existsSync(path)) continue
        throw new StepError(`the pipeline lock record ${path} cannot be read; another run may hold ${home}. If no pipeline run is going, delete ${dir}`)
      } else if (held !== 'free' && held.pid !== pid && alive(held.pid)) {
        throw busy(held, home, dir)
      }
    }
    // The holder is gone, or the lock is free: the next number is the lock, for whichever run creates it.
    const mine = top + 1
    if (!createRecord(dir, mine, { pid, command, startedAt: new Date(now()).toISOString() }, opts.beforePublish)) continue
    // A run that read an older number may have created one below ours since lower numbers were
    // removed; it is not the highest, and that run gives up below. Ours is lower than another only
    // the same way.
    if (generations(dir).some((g) => g > mine)) {
      rmSync(join(dir, `${mine}.json`), { force: true })
      continue
    }
    removeBelow(dir, mine)
    let released = false
    return () => {
      if (released) return
      released = true
      if (createRecord(dir, mine + 1, { free: true })) removeBelow(dir, mine + 1)
    }
  }
  throw new StepError(`cannot take ${dir}`)
}
