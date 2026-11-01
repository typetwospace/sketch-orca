import { describe, expect, test } from 'bun:test'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { enginePaths, readStamp, sha256File, sha256Text, WINDOWS_WORK_PATH_MAX, workHome, workPathProblem, workPaths, writeStamp } from '../scripts/lib/paths.ts'

describe('paths', () => {
  test('workHome: SKETCH_ORCA_HOME first, then LOCALAPPDATA on Windows, XDG_DATA_HOME or ~/.local/share elsewhere', () => {
    expect(workHome({ SKETCH_ORCA_HOME: 'x/y', LOCALAPPDATA: 'L' }, 'win32', 'H')).toBe(resolve('x/y'))
    expect(workHome({ LOCALAPPDATA: 'L' }, 'win32', 'H')).toBe(join('L', 'SketchDev', 'orca-engine'))
    expect(workHome({}, 'win32', 'H')).toBe(join('H', 'AppData', 'Local', 'SketchDev', 'orca-engine'))
    expect(workHome({ XDG_DATA_HOME: 'X' }, 'linux', 'H')).toBe(join('X', 'SketchDev', 'orca-engine'))
    expect(workHome({}, 'darwin', 'H')).toBe(join('H', '.local', 'share', 'SketchDev', 'orca-engine'))
  })

  test('workPathProblem: on Windows a work folder too long for Visual C++ is refused, and the default one of any user name fits', () => {
    expect(WINDOWS_WORK_PATH_MAX).toBe(65)
    const longest = 'C:\\' + 'w'.repeat(WINDOWS_WORK_PATH_MAX - 3)
    expect(workPathProblem(longest, 'win32')).toBeUndefined()
    const problem = workPathProblem(`${longest}x`, 'win32')
    expect(problem).toContain(`is ${WINDOWS_WORK_PATH_MAX + 1} characters long`)
    expect(problem).toContain('SKETCH_ORCA_HOME')
    // Elsewhere there is no such limit.
    expect(workPathProblem(`/${'w'.repeat(300)}`, 'linux')).toBeUndefined()
    // A Windows user name has at most 20 characters.
    const home = workHome({}, 'win32', `C:\\Users\\${'u'.repeat(20)}`)
    expect(home.length).toBe(WINDOWS_WORK_PATH_MAX)
    expect(workPathProblem(home, 'win32')).toBeUndefined()
  })

  test('workPaths lays the work folder out', () => {
    const w = workPaths('W')
    expect(w).toEqual({
      home: 'W',
      src: join('W', 'src'),
      deps: join('W', 'deps'),
      depsPrefix: join('W', 'deps', 'OrcaSlicer_dep', 'usr', 'local'),
      downloads: join('W', 'downloads'),
      build: join('W', 'build'),
      dist: join('W', 'dist'),
      mingw: join('W', 'mingw'),
      logs: join('W', 'logs'),
      state: join('W', 'state'),
      smoke: join('W', 'smoke'),
    })
  })

  test('enginePaths: lock, patches, cli, scripts, licences, README, NOTICE and AUTHORS of the repository (no ci folder)', () => {
    expect(enginePaths('E')).toEqual({
      engine: 'E',
      lock: join('E', 'orca.lock.json'),
      patches: join('E', 'patches'),
      cli: join('E', 'cli'),
      scripts: join('E', 'scripts'),
      licenses: join('E', 'licenses'),
      readme: join('E', 'README.md'),
      notice: join('E', 'NOTICE'),
      authors: join('E', 'AUTHORS'),
    })
  })

  test('hashes and stamps', () => {
    const dir = mkdtempSync(join(tmpdir(), 'orca-paths-'))
    try {
      expect(sha256Text('abc')).toBe('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
      writeFileSync(join(dir, 'f.txt'), 'abc')
      expect(sha256File(join(dir, 'f.txt'))).toBe(sha256Text('abc'))
      const state = join(dir, 'state')
      expect(readStamp(state, 'build')).toBeUndefined()
      writeStamp(state, 'build', 'inputs-hash', { exe: 'x' })
      const s = readStamp(state, 'build')!
      expect(s.inputs).toBe('inputs-hash')
      expect(s.detail).toEqual({ exe: 'x' })
      expect(Number.isNaN(Date.parse(s.finishedAt))).toBe(false)
      writeFileSync(join(state, 'broken.json'), '{')
      expect(readStamp(state, 'broken')).toBeUndefined()
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
