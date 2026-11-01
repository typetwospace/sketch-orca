import { describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pidAlive, readRunLock, RUN_LOCK_DIR, RUN_LOCK_FILE, runLockHolder, takeRunLock } from '../scripts/lib/runlock.ts'
import { EXIT } from '../scripts/lib/run.ts'
import { exitCodeFor, LOCKED_COMMANDS } from '../scripts/orca.ts'

function withHome(fn: (home: string) => void): void {
  const home = mkdtempSync(join(tmpdir(), 'orca-runlock-'))
  try {
    fn(home)
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
}

describe('run lock', () => {
  test('pidAlive: this process is running, a finished one is not', () => {
    expect(pidAlive(process.pid)).toBe(true)
    const done = spawnSync(process.execPath, ['-e', '0'], { windowsHide: true })
    expect(done.status).toBe(0)
    expect(pidAlive(done.pid!)).toBe(false)
  })

  test('takes the lock, refuses a second run while the first holds it, and gives it back', () => {
    withHome((home) => {
      const release = takeRunLock(home, 'build', { pid: 101, alive: () => true })
      expect(runLockHolder(home)).toMatchObject({ pid: 101, command: 'build' })
      expect(() => takeRunLock(home, 'stage', { pid: 202, alive: (pid) => pid === 101 })).toThrow(/another pipeline run \(build, process 101, started .*\) is using/)
      // The refused run left the lock alone.
      expect(runLockHolder(home)?.pid).toBe(101)
      release()
      expect(runLockHolder(home)).toBeUndefined()
      const again = takeRunLock(home, 'stage', { pid: 202, alive: () => true })
      expect(runLockHolder(home)).toMatchObject({ pid: 202, command: 'stage' })
      again()
      // Only the newest record is kept.
      expect(readdirSync(join(home, RUN_LOCK_DIR))).toEqual(['4.json'])
    })
  })

  test('a lock whose process is gone is taken over', () => {
    withHome((home) => {
      mkdirSync(join(home, RUN_LOCK_DIR))
      writeFileSync(join(home, RUN_LOCK_DIR, '7.json'), JSON.stringify({ pid: 999_999, command: 'smoke', startedAt: '2026-10-30T00:00:00.000Z' }))
      // The earlier pipeline's lock file, of a run that is gone, does not hold the folder either.
      writeFileSync(join(home, RUN_LOCK_FILE), JSON.stringify({ pid: 999_998, command: 'build', startedAt: '2026-10-29T00:00:00.000Z' }))
      const release = takeRunLock(home, 'build', { pid: 303, alive: (pid) => pid === 303 })
      expect(runLockHolder(home)).toMatchObject({ pid: 303, command: 'build' })
      expect(readdirSync(join(home, RUN_LOCK_DIR))).toEqual(['8.json'])
      release()
    })
  })

  test('of two runs that find the holder gone, only one takes the lock, and neither removes the other', () => {
    withHome((home) => {
      mkdirSync(join(home, RUN_LOCK_DIR))
      writeFileSync(join(home, RUN_LOCK_DIR, '1.json'), JSON.stringify({ pid: 999_999, command: 'smoke', startedAt: 'then' }))
      // Run A reads the dead holder; while it decides, run B takes the lock over, then A goes on.
      let releaseB: (() => void) | undefined
      const aliveForA = (pid: number): boolean => {
        if (pid === 999_999 && !releaseB) {
          releaseB = takeRunLock(home, 'stage', { pid: 202, alive: (p) => p === 202 })
          return false
        }
        return pid === 202
      }
      expect(() => takeRunLock(home, 'build', { pid: 101, alive: aliveForA })).toThrow(/another pipeline run \(stage, process 202/)
      expect(runLockHolder(home)).toMatchObject({ pid: 202, command: 'stage' })
      releaseB!()
      expect(runLockHolder(home)).toBeUndefined()
    })
  })

  test('a run of an earlier pipeline that holds run.lock holds the work folder', () => {
    withHome((home) => {
      writeFileSync(join(home, RUN_LOCK_FILE), JSON.stringify({ pid: 101, command: 'build', startedAt: 'now' }))
      expect(() => takeRunLock(home, 'stage', { pid: 202, alive: (pid) => pid === 101 })).toThrow(/another pipeline run \(build, process 101, started now\) is using .*run\.lock\)/)
      expect(readRunLock(join(home, RUN_LOCK_FILE))?.pid).toBe(101)
    })
  })

  test('an unreadable lock is a run that is starting while it is young, and stale after 10 s', () => {
    withHome((home) => {
      mkdirSync(join(home, RUN_LOCK_DIR))
      const path = join(home, RUN_LOCK_DIR, '1.json')
      writeFileSync(path, '')
      const t = Date.now()
      expect(() => takeRunLock(home, 'build', { pid: 404, alive: () => true, now: () => t })).toThrow(/is starting/)
      utimesSync(path, new Date(t - 60_000), new Date(t - 60_000))
      const release = takeRunLock(home, 'build', { pid: 404, alive: () => true, now: () => t })
      expect(runLockHolder(home)?.pid).toBe(404)
      release()
    })
  })

  test('giving the lock back leaves a lock another run has taken since', () => {
    withHome((home) => {
      const release = takeRunLock(home, 'build', { pid: 505, alive: () => false })
      // A second run took it over (for example after this one was thought gone).
      const second = takeRunLock(home, 'stage', { pid: 606, alive: (pid) => pid === 606 })
      release()
      expect(runLockHolder(home)).toMatchObject({ pid: 606, command: 'stage' })
      second()
      expect(runLockHolder(home)).toBeUndefined()
    })
  })

  test('the commands that write in the work folder hold it; check and env do not', () => {
    expect(LOCKED_COMMANDS).toEqual(['update', 'fetch', 'build', 'smoke', 'stage', 'source', 'release'])
  })
})

describe('exit codes', () => {
  test('check keeps 1 (newer) and 2 (tag moved); update and release keep 2; every other non-zero code is a failure, 3', () => {
    expect(EXIT).toEqual({ ok: 0, newer: 1, tagMoved: 2, failed: 3, usage: 64 })
    expect(exitCodeFor('check', 0)).toBe(0)
    expect(exitCodeFor('check', 1)).toBe(1)
    expect(exitCodeFor('check', 2)).toBe(2)
    expect(exitCodeFor('update', 2)).toBe(2)
    expect(exitCodeFor('release', 2)).toBe(2)
    // smoke returns 1 when cases fail; release passes on a failed step's code.
    expect(exitCodeFor('smoke', 1)).toBe(3)
    expect(exitCodeFor('release', 1)).toBe(3)
    expect(exitCodeFor('build', 2)).toBe(3)
  })

  test('the command line: an unknown command exits 64, a bad option 64, a failure 3 (never 1 or 2)', () => {
    withHome((home) => {
      const orca = join(import.meta.dir, '..', 'scripts', 'orca.ts')
      const run = (...args: string[]) => spawnSync(process.execPath, [orca, ...args], { env: { ...process.env, SKETCH_ORCA_HOME: home }, encoding: 'utf8', windowsHide: true })
      const bogus = run('bogus')
      expect(bogus.status).toBe(64)
      expect(bogus.stderr).toContain('usage:')
      expect(run('build', '--step', 'everything').status).toBe(64)
      // stage with no engine built: a failure.
      const stage = run('stage')
      expect(stage.status).toBe(3)
      expect(stage.stdout + stage.stderr).toMatch(/stage: FAILED: no engine at/)
      // The lock was given back.
      expect(existsSync(join(home, RUN_LOCK_DIR))).toBe(true)
      expect(runLockHolder(home)).toBeUndefined()
      // While another run holds the work folder, a writing command fails and leaves its lock alone.
      const release = takeRunLock(home, 'build')
      const busy = run('stage')
      expect(busy.status).toBe(3)
      expect(busy.stdout + busy.stderr).toMatch(/another pipeline run \(build, process \d+/)
      expect(runLockHolder(home)?.pid).toBe(process.pid)
      release()
      // And so does a run of an earlier pipeline, which holds run.lock.
      writeFileSync(join(home, RUN_LOCK_FILE), JSON.stringify({ pid: process.pid, command: 'smoke', startedAt: 'now' }))
      const old = run('stage')
      expect(old.status).toBe(3)
      expect(old.stdout + old.stderr).toMatch(/another pipeline run \(smoke, process \d+/)
      expect(readRunLock(join(home, RUN_LOCK_FILE))?.pid).toBe(process.pid)
    })
  })
})
