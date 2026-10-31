import { describe, expect, test } from 'bun:test'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  compareLock,
  compareVersions,
  LOCK_FORMAT,
  LockError,
  lockForRelease,
  parseLock,
  readLock,
  releaseDecision,
  serializeLock,
  validateLock,
  versionFromTag,
  writeLock,
  type OrcaLock,
} from '../scripts/lib/lock.ts'

const LOCK: OrcaLock = {
  format: LOCK_FORMAT,
  repo: 'OrcaSlicer/OrcaSlicer',
  tag: 'v2.4.2',
  commit: '8500fcdccaa10b5099ac20d252af3a7c560046f1',
  version: '2.4.2',
  publishedAt: '2026-07-07T03:33:49Z',
  license: 'AGPL-3.0',
}
const OTHER = 'a'.repeat(40)

describe('lock', () => {
  test('the checked-in lock is valid and pins v2.4.2', () => {
    const lock = readLock(join(import.meta.dir, '..', 'orca.lock.json'))
    expect(lock).toEqual(LOCK)
    expect(readFileSync(join(import.meta.dir, '..', 'orca.lock.json'), 'utf8')).toBe(serializeLock(lock))
  })

  test('validateLock names each bad field', () => {
    expect(validateLock({ ...LOCK })).toEqual(LOCK)
    const bad: [Record<string, unknown>, RegExp][] = [
      [{ ...LOCK, format: 'sketch-orca-lock/2' }, /format/],
      [{ ...LOCK, repo: 'OrcaSlicer' }, /"repo"/],
      [{ ...LOCK, repo: 7 }, /"repo" must be a string/],
      [{ ...LOCK, tag: 'v2.4.2 rc' }, /"tag"/],
      [{ ...LOCK, commit: '8500fcd' }, /"commit"/],
      [{ ...LOCK, commit: LOCK.commit.toUpperCase() }, /"commit"/],
      [{ ...LOCK, version: '2.4.1' }, /"version" 2.4.1 does not match tag v2.4.2/],
      [{ ...LOCK, publishedAt: 'yesterday' }, /"publishedAt"/],
      [{ ...LOCK, license: undefined }, /"license"/],
    ]
    for (const [value, re] of bad) expect(() => validateLock(value)).toThrow(re)
    expect(() => validateLock([])).toThrow(LockError)
    expect(() => validateLock(null)).toThrow(/not a JSON object/)
    expect(validateLock({ ...LOCK, publishedAt: '' }).publishedAt).toBe('')
  })

  test('parseLock refuses text that is not JSON', () => {
    expect(() => parseLock('{')).toThrow(/invalid JSON/)
    expect(parseLock(JSON.stringify(LOCK))).toEqual(LOCK)
  })

  test('serializeLock: fixed key order, two spaces, LF, final newline; extra keys dropped', () => {
    const shuffled = { license: LOCK.license, commit: LOCK.commit, tag: LOCK.tag, extra: 1, repo: LOCK.repo, format: LOCK.format, publishedAt: LOCK.publishedAt, version: LOCK.version } as unknown as OrcaLock
    const text = serializeLock(shuffled)
    expect(Object.keys(JSON.parse(text))).toEqual(['format', 'repo', 'tag', 'commit', 'version', 'publishedAt', 'license'])
    expect(text.endsWith('}\n')).toBe(true)
    expect(text.includes('\r')).toBe(false)
    expect(text.split('\n')[1]).toBe('  "format": "sketch-orca-lock/1",')
  })

  test('writeLock validates and round-trips', () => {
    const dir = mkdtempSync(join(tmpdir(), 'orca-lock-'))
    try {
      const p = join(dir, 'orca.lock.json')
      writeLock(p, LOCK)
      expect(readLock(p)).toEqual(LOCK)
      expect(() => writeLock(p, { ...LOCK, commit: 'nope' })).toThrow(/commit/)
      expect(readLock(p)).toEqual(LOCK)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('versionFromTag drops a leading v before a digit only', () => {
    expect(versionFromTag('v2.4.2')).toBe('2.4.2')
    expect(versionFromTag('2.4.2')).toBe('2.4.2')
    expect(versionFromTag('version-1')).toBe('version-1')
  })

  test('compareVersions compares numerically', () => {
    expect(compareVersions('2.4.10', '2.4.9')).toBe(1)
    expect(compareVersions('2.4.9', '2.4.10')).toBe(-1)
    expect(compareVersions('2.4', '2.4.0')).toBe(0)
    expect(compareVersions('2.5.0', '2.4.99')).toBe(1)
    expect(compareVersions('3.0.0-beta', '3.0.0-alpha')).toBe(1)
  })

  test('lockForRelease keeps repo and licence and derives the version', () => {
    const next = lockForRelease(LOCK, { tag: 'v2.5.0', commit: OTHER, publishedAt: '2026-11-01T00:00:00Z' })
    expect(next).toEqual({ ...LOCK, tag: 'v2.5.0', commit: OTHER, version: '2.5.0', publishedAt: '2026-11-01T00:00:00Z' })
    expect(() => lockForRelease(LOCK, { tag: 'v2.5.0', commit: 'short', publishedAt: '' })).toThrow(/commit/)
  })

  test('compareLock: same, retagged, newer and older releases', () => {
    expect(compareLock(LOCK, { tag: 'v2.4.2', commit: LOCK.commit, publishedAt: '' })).toBe('same')
    expect(compareLock(LOCK, { tag: 'v2.4.2', commit: OTHER, publishedAt: '' })).toBe('retagged')
    expect(compareLock(LOCK, { tag: 'v2.4.10', commit: OTHER, publishedAt: '' })).toBe('newer-release')
    expect(compareLock(LOCK, { tag: 'v2.4.1', commit: OTHER, publishedAt: '' })).toBe('older-release')
  })

  test('releaseDecision: what check returns and whether release updates and goes on', () => {
    const rel = (tag: string, commit = OTHER) => ({ tag, commit, publishedAt: '' })
    const pick = (tag: string, commit?: string) => {
      const r = rel(tag, commit)
      const d = releaseDecision(compareLock(LOCK, r), LOCK, r)
      return [d.update, d.proceed, d.checkCode]
    }
    expect(pick('v2.4.2', LOCK.commit)).toEqual([false, true, 0])
    expect(pick('v2.5.0')).toEqual([true, true, 1])
    expect(pick('v2.4.0')).toEqual([false, true, 0])
    expect(pick('v2.4.2')).toEqual([false, false, 2])
    const moved = releaseDecision('retagged', LOCK, rel('v2.4.2'))
    expect(moved.message).toContain(OTHER)
    expect(moved.message).toContain(LOCK.commit)
  })
})
