// parity same and what it stands on: the environment a client gives the engine (engineEnv.ts),
// the clock and entropy masks (masks.ts, parity/masks.json) and the recording and comparison of
// engine runs (same.ts). Hermetic: temporary folders and a fake engine process only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { engineEnvProblems, parityEngineEnv, PASSED_ON_WIN32 } from '../scripts/lib/parity/engineEnv.ts'
import { centralRecords, compareOutput, globRegExp, MASKS_FILE, masksFor, parseMasks, readMasks, zeroZipTimes, type Mask } from '../scripts/lib/parity/masks.ts'
import { compareRun, headEngineProblems, portableArgs, recordingExec, runStore, sameFiles, sameReport, snapshotChanges, snapshotDir, type CaseReport, type RecordedRun, type RunProcess } from '../scripts/lib/parity/same.ts'
import type { EngineManifest } from '../scripts/lib/stage.ts'
import { SMOKE_VERSION } from '../scripts/lib/smoke.ts'
import { STAGE_VERSION } from '../scripts/lib/stage.ts'
import { writeZip } from './support/zip-writer.ts'

let work: string
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-same-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

const { masks } = readMasks()

describe('engine environment', () => {
  test('Windows: the system folders, the engine library path first on PATH, the user folders, and nothing else', () => {
    const base = { SystemRoot: 'C:\\Windows', TEMP: 'C:\\T', TMP: 'C:\\T', LOCALAPPDATA: 'C:\\U\\L', APPDATA: 'C:\\U\\R', USERPROFILE: 'C:\\U', USERNAME: 'me', USER: 'me', SLIC3R_LOGLEVEL: '5', Path: 'C:\\tools', HOME: 'C:\\U' }
    const env = parityEngineEnv({ libraryPath: 'E:\\engine\\bin' }, base, 'win32')
    expect(env).toEqual({ SystemRoot: 'C:\\Windows', windir: 'C:\\Windows', PATH: 'E:\\engine\\bin;C:\\Windows\\System32;C:\\Windows', TEMP: 'C:\\T', TMP: 'C:\\T', LOCALAPPDATA: 'C:\\U\\L', APPDATA: 'C:\\U\\R', USERPROFILE: 'C:\\U' })
    expect(engineEnvProblems(env)).toEqual([])
    expect([...PASSED_ON_WIN32]).toEqual(['TEMP', 'TMP', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE'])
  })

  test('Windows names are found in any letter case; a variable not set is left out; SystemRoot defaults to C:\\Windows', () => {
    const env = parityEngineEnv({ libraryPath: 'B' }, { SYSTEMROOT: 'D:\\Win', temp: 'D:\\t' }, 'win32')
    expect(env).toEqual({ SystemRoot: 'D:\\Win', windir: 'D:\\Win', PATH: 'B;D:\\Win\\System32;D:\\Win', TEMP: 'D:\\t' })
    expect(parityEngineEnv({ libraryPath: 'B' }, {}, 'win32').SystemRoot).toBe('C:\\Windows')
  })

  test('Linux and macOS: a plain PATH, HOME, TMPDIR and the library path', () => {
    expect(parityEngineEnv({ libraryPath: '/e/bin' }, { HOME: '/h', USER: 'me' }, 'linux')).toEqual({ PATH: '/usr/bin:/bin', HOME: '/h', TMPDIR: '/tmp', LD_LIBRARY_PATH: '/e/bin' })
    expect(parityEngineEnv({ libraryPath: '/e/bin' }, {}, 'darwin')).toEqual({ PATH: '/usr/bin:/bin', HOME: '/tmp', TMPDIR: '/tmp', DYLD_LIBRARY_PATH: '/e/bin' })
  })

  test('a user name or a SLIC3R_* variable, in any letter case, is refused', () => {
    expect(engineEnvProblems({ USER: 'a', username: 'b', LogName: 'c', slic3r_x: '1', PATH: 'p' })).toHaveLength(4)
  })
})

describe('masks', () => {
  test("parity/masks.json: the G-code header's time, the 3MF dates, the plate MD5, the zip entry times and a crash's place in the program, each with its source line", () => {
    expect(MASKS_FILE.replace(/\\/g, '/')).toMatch(/\/parity\/masks\.json$/)
    expect(masks.map((m) => [m.id, m.kind])).toEqual([
      ['gcode-generated-on', 'text'],
      ['3mf-metadata-dates', 'text'],
      ['3mf-gcode-md5', 'md5-of'],
      ['zip-entry-times', 'zip-times'],
      ['crash-location', 'text'],
    ])
    for (const m of masks) expect(m.source).toMatch(/\.(cpp|h)|miniz/)
  })

  test("a crash result's place in the program is masked; its exception code and stage are not", () => {
    const result = (where: string, code = 'C0000005', stage = 'slice') => Buffer.from(`{\n  "message": "the engine crashed (exception 0x${code} at ${where}) during '${stage}', in Orca's code",\n}\n`)
    expect(compareOutput('slice/result.json', result('sketch-orca.exe+0x77c95'), result('sketch-orca-oracle.exe+0x7a7e5'), masks)).toEqual({ status: 'masked', masks: ['crash-location'] })
    expect(compareOutput('slice/result.json', result('sketch-orca.exe+0x77c95'), result('sketch-orca.exe+0x77c95', 'C0000094'), masks).status).toBe('different')
    expect(compareOutput('slice/result.json', result('sketch-orca.exe+0x77c95'), result('sketch-orca.exe+0x1', 'C0000005', 'export'), masks).status).toBe('different')
    expect(compareOutput('slice/out.gcode', result('sketch-orca.exe+0x77c95'), result('sketch-orca-oracle.exe+0x7a7e5'), masks).status).toBe('different')
  })

  test('parseMasks refuses a wrong format, ids given twice, no source, a pattern that matches empty text and unknown kinds', () => {
    const one = (m: Record<string, unknown>) => ({ format: 'sketch-orca-parity-masks/1', masks: [m] })
    const ok = { id: 'a', kind: 'text', files: ['*.gcode'], pattern: 'x', replace: 'y', source: 's', why: 'w' }
    expect(parseMasks(one(ok))).toHaveLength(1)
    expect(() => parseMasks({ format: 'other', masks: [] })).toThrow(/format/)
    expect(() => parseMasks({ format: 'sketch-orca-parity-masks/1', masks: [ok, ok] })).toThrow(/twice/)
    expect(() => parseMasks(one({ ...ok, source: '' }))).toThrow(/source/)
    expect(() => parseMasks(one({ ...ok, pattern: 'x*' }))).toThrow(/empty/)
    expect(() => parseMasks(one({ ...ok, kind: 'other' }))).toThrow(/unknown kind/)
    expect(() => parseMasks(one({ ...ok, kind: 'md5-of', files: ['*.gcode'] }))).toThrow(/\.md5/)
    expect(() => parseMasks(one({ ...ok, kind: 'zip-times', files: ['a.3mf!x'] }))).toThrow(/archives/)
  })

  test('globs: * stays within one name, never across / or !', () => {
    expect(globRegExp('*.gcode').test('plate_1.gcode')).toBe(true)
    expect(globRegExp('*.gcode').test('a/plate_1.gcode')).toBe(false)
    expect(globRegExp('*.3mf!Metadata/plate_*.gcode').test('out.gcode.3mf!Metadata/plate_1.gcode')).toBe(true)
    expect(globRegExp('*.3mf!Metadata/plate_*.gcode').test('out.gcode.3mf!Metadata/x/plate_1.gcode')).toBe(false)
    expect(masksFor(masks, 'out.gcode').map((m) => m.id)).toEqual(['gcode-generated-on'])
  })

  const gcode = (time: string, body = 'G1 X1\n') => Buffer.from(`; HEADER_BLOCK_START\n; generated by OrcaSlicer 2.4.2 on ${time}\n${body}`)

  test("a G-code file that differs only in its header's time is masked; any other difference is different, at its line", () => {
    expect(compareOutput('a/out.gcode', gcode('2026-11-06 at 07:00:00'), gcode('2026-11-06 at 07:00:00'), masks)).toEqual({ status: 'identical', masks: [] })
    expect(compareOutput('a/out.gcode', gcode('2026-11-06 at 07:00:00'), gcode('2026-11-07 at 08:01:02'), masks)).toEqual({ status: 'masked', masks: ['gcode-generated-on'] })
    const d = compareOutput('a/out.gcode', gcode('2026-11-06 at 07:00:00'), gcode('2026-11-07 at 08:01:02', 'G1 X2\n'), masks)
    expect(d.status).toBe('different')
    expect(d.detail).toBe('line 3: "G1 X1" against "G1 X2"')
    // The generator's name and version stay compared.
    expect(compareOutput('out.gcode', Buffer.from('; generated by OrcaSlicer 2.4.2 on 2026-11-06 at 07:00:00\n'), Buffer.from('; generated by OrcaSlicer 2.4.3 on 2026-11-06 at 07:00:01\n'), masks).status).toBe('different')
    // The mask names *.gcode only: the same bytes in a .json differ.
    expect(compareOutput('result.json', gcode('2026-11-06 at 07:00:00'), gcode('2026-11-07 at 08:01:02'), masks).status).toBe('different')
  })

  test('an output only one engine wrote is different, naming which', () => {
    expect(compareOutput('x.gcode', Buffer.from('a'), undefined, masks)).toEqual({ status: 'different', masks: [], detail: 'only the reference wrote it' })
    expect(compareOutput('x.gcode', undefined, Buffer.from('a'), masks)).toEqual({ status: 'different', masks: [], detail: 'only the head wrote it' })
    expect(compareOutput('x.gcode', undefined, undefined, masks).status).toBe('identical')
  })

  /** A Bambu .gcode.3mf of one plate: its G-code, the G-code's MD5 (upper-case hex) and a model. */
  function project(time: string, o: { md5?: string; model?: string; dos?: number } = {}): Buffer {
    const g = gcode(time)
    const md5 = o.md5 ?? createHash('md5').update(g).digest('hex').toUpperCase()
    const zip = writeZip([
      { name: '3D/3dmodel.model', data: o.model ?? '<model><metadata name="CreationDate"></metadata></model>' },
      { name: 'Metadata/plate_1.gcode', data: g },
      { name: 'Metadata/plate_1.gcode.md5', data: md5 },
    ])
    if (o.dos !== undefined) {
      for (const r of centralRecords(zip)) {
        zip.writeUInt32LE(o.dos, r.start + 12)
        zip.writeUInt32LE(o.dos, r.local + 10)
      }
    }
    return zip
  }

  test('a .gcode.3mf whose plate G-code differs in its time, with its MD5 and entry times, is masked by those masks', () => {
    const a = project('2026-11-06 at 07:00:00', { dos: 0x5b663800 })
    const b = project('2026-11-07 at 08:01:02', { dos: 0x5b673840 })
    expect(compareOutput('out.gcode.3mf', a, a, masks).status).toBe('identical')
    expect(compareOutput('out.gcode.3mf', a, b, masks)).toEqual({ status: 'masked', masks: ['3mf-gcode-md5', 'gcode-generated-on', 'zip-entry-times'] })
    // Only the entry times differ.
    expect(compareOutput('out.gcode.3mf', project('2026-11-06 at 07:00:00', { dos: 1 }), project('2026-11-06 at 07:00:00', { dos: 2 }), masks)).toEqual({ status: 'masked', masks: ['zip-entry-times'] })
    expect(zeroZipTimes(a).equals(zeroZipTimes(project('2026-11-06 at 07:00:00', { dos: 7 })))).toBe(true)
  })

  test('a 3MF MD5 that is not that of its own G-code, or a model that differs, is different', () => {
    const a = project('2026-11-06 at 07:00:00')
    expect(compareOutput('out.gcode.3mf', a, project('2026-11-07 at 08:01:02', { md5: '0'.repeat(32) }), masks)).toMatchObject({ status: 'different', detail: expect.stringContaining('plate_1.gcode.md5') })
    expect(compareOutput('out.gcode.3mf', a, project('2026-11-06 at 07:00:00', { model: '<model><metadata name="Title">x</metadata></model>' }), masks)).toMatchObject({ status: 'different', detail: expect.stringContaining('3D/3dmodel.model') })
    // An entry more or less, or in another order.
    const fewer = writeZip([{ name: '3D/3dmodel.model', data: 'm' }])
    expect(compareOutput('out.gcode.3mf', a, fewer, masks)).toMatchObject({ status: 'different', detail: expect.stringContaining('entries differ') })
  })

  test('the 3MF dates mask empties the creation and modification dates', () => {
    const m = (d: string) => project('2026-11-06 at 07:00:00', { model: `<model><metadata name="CreationDate">${d}</metadata><metadata name="ModificationDate">${d}</metadata></model>` })
    expect(compareOutput('out.3mf', m('2026-11-06'), m('2026-11-07'), masks)).toEqual({ status: 'masked', masks: ['3mf-metadata-dates'] })
  })

  test('the bytes differing with no mask that names what is different, not masked', () => {
    const none: Mask[] = []
    expect(compareOutput('out.gcode', gcode('2026-11-06 at 07:00:00'), gcode('2026-11-07 at 08:01:02'), none)).toMatchObject({ status: 'different' })
  })
})

describe('recording and comparing engine runs', () => {
  test('snapshots leave out the jobs\' work folders; changes are files made, changed or gone', () => {
    const dir = join(work, 'case')
    mkdirSync(join(dir, 'work'), { recursive: true })
    writeFileSync(join(dir, 'job.json'), '{}')
    writeFileSync(join(dir, 'work', 'tmp_123'), 'x')
    const before = snapshotDir(dir)
    expect([...before.keys()]).toEqual(['job.json'])
    writeFileSync(join(dir, 'job.json'), '{"a":1}')
    writeFileSync(join(dir, 'out.gcode'), 'G1')
    const after = snapshotDir(dir)
    expect(snapshotChanges(before, after)).toEqual({ changed: ['job.json', 'out.gcode'], gone: [] })
    expect(snapshotChanges(after, new Map())).toEqual({ changed: [], gone: ['job.json', 'out.gcode'] })
  })

  test("portableArgs writes the cases' folder as <cases>", () => {
    expect(portableArgs(['slice', 'C:\\w\\cases\\cube\\job.json', '--x'], 'C:\\w\\cases')).toEqual(['slice', '<cases>\\cube\\job.json', '--x'])
  })

  test('recordingExec runs the engine hidden in the case folder with the engine environment, and keeps its outputs', async () => {
    const root = join(work, 'cases')
    const caseDir = join(root, 'cube')
    mkdirSync(caseDir, { recursive: true })
    const store = join(work, 'store', 'cube')
    const seen: { exe: string; args: string[]; cwd: string; env: Record<string, string> }[] = []
    const run: RunProcess = async (exe, args, o) => {
      seen.push({ exe, args, cwd: o.cwd, env: o.env })
      writeFileSync(join(o.cwd, 'out.gcode'), 'G1 X1')
      return { code: 0, timedOut: false, stdout: Buffer.from('{"ok":true}\n'), stderr: Buffer.from('warn\n') }
    }
    const runs: RecordedRun[] = []
    const env = { SystemRoot: 'C:\\Windows', PATH: 'x' }
    const exec = recordingExec({ exe: 'E.exe', caseDir, root, store, env, timeoutMs: 1000, runs, run })
    const r = await exec(['slice', join(caseDir, 'job.json')], { SKETCH_ORCA_TEST_CRASH: '1' })
    expect(r).toEqual({ code: 0, timedOut: false, stdout: '{"ok":true}\n' })
    expect(seen[0]!.cwd).toBe(caseDir)
    expect(seen[0]!.env).toEqual({ ...env, SKETCH_ORCA_TEST_CRASH: '1' })
    expect(runs).toEqual([{ index: 0, args: ['slice', join('<cases>', 'cube', 'job.json')], extraEnv: ['SKETCH_ORCA_TEST_CRASH'], code: 0, timedOut: false, files: ['out.gcode'], gone: [] }])
    const s = runStore(store, 0)
    expect(readFileSync(s.stdout, 'utf8')).toBe('{"ok":true}\n')
    expect(readFileSync(s.stderr, 'utf8')).toBe('warn\n')
    expect(readFileSync(join(s.files, 'out.gcode'), 'utf8')).toBe('G1 X1')
    // An environment that names the user is refused before the engine runs.
    await expect(exec(['version'], { USERNAME: 'me' })).rejects.toThrow(/USERNAME/)
    expect(seen).toHaveLength(1)
  })

  test('compareRun: exit code, stdout, stderr and every file, masked or not; sameReport counts them', () => {
    const stores = { reference: join(work, 'ref'), head: join(work, 'head') }
    const put = (store: string, rel: string, data: string | Buffer) => {
      const p = join(runStore(store, 0).dir, rel)
      mkdirSync(join(p, '..'), { recursive: true })
      writeFileSync(p, data)
    }
    put(stores.reference, 'stdout', 'ok')
    put(stores.head, 'stdout', 'ok')
    put(stores.reference, 'stderr', '')
    put(stores.head, 'stderr', 'extra')
    put(stores.reference, 'files/out.gcode', gcodeText('07:00:00'))
    put(stores.head, 'files/out.gcode', gcodeText('08:00:00'))
    put(stores.head, 'files/only.txt', 'x')
    const rec = (files: string[], code = 0): RecordedRun => ({ index: 0, args: ['slice', '<cases>/c/job.json'], extraEnv: [], code, timedOut: false, files, gone: [] })
    const r = compareRun(rec(['out.gcode']), rec(['only.txt', 'out.gcode'], 1), stores, masks)
    expect(r.outputs.map((o) => [o.name, o.status])).toEqual([
      ['exit', 'different'],
      ['stdout', 'identical'],
      ['stderr', 'different'],
      ['files/only.txt', 'different'],
      ['files/out.gcode', 'masked'],
    ])
    const c: CaseReport = { id: 'c', smoke: { reference: { ok: true, problems: [] }, head: { ok: true, problems: [] } }, runs: [r] }
    const eng = { dir: 'd', version: '0.1.0', stageVersion: 3, orcaCommit: 'c', exeSha256: 'e', manifestSha256: 'm' }
    const report = sameReport({ cases: [c], engines: { reference: eng, head: eng }, masks: { file: 'parity/masks.json', sha256: 's' }, env: ['PATH'] })
    expect(report.summary).toEqual({ cases: 1, runs: 1, outputs: 5, identical: 1, masked: 1, different: 3 })
    expect(report.different).toEqual(['c/0/exit', 'c/0/stderr', 'c/0/files/only.txt'])
    expect(report.smokeVersion).toBe(SMOKE_VERSION)
    // A run only one engine made is different.
    expect(compareRun(rec([]), undefined, stores, masks).outputs).toEqual([{ name: 'run', status: 'different', masks: [], detail: 'only the reference engine ran it' }])
  })

  test('headEngineProblems: the stage version, a full smoke run of this version, the commit, patches/ and cli/', () => {
    const m = { stageVersion: STAGE_VERSION, smoke: { passed: true, smokeVersion: SMOKE_VERSION }, orca: { commit: 'c' }, patches: [{ name: 'p', sha256: 's' }], cli: 'h' } as unknown as EngineManifest
    const want = { commit: 'c', patches: [{ name: 'p', sha256: 's' }], cli: 'h' }
    expect(headEngineProblems(m, want)).toEqual([])
    const old = { ...m, stageVersion: 2, smoke: { passed: true, smokeVersion: 5 }, patches: [] } as EngineManifest
    expect(headEngineProblems(old, want)).toHaveLength(3)
  })

  test('same.json and the run folders are in <work>/parity', () => {
    expect(sameFiles({ parity: 'P', runs: join('P', 'runs') })).toEqual({ report: join('P', 'same.json'), root: join('P', 'runs', 'same', 'cases'), stores: { reference: join('P', 'runs', 'same', 'reference'), head: join('P', 'runs', 'same', 'head') } })
  })
})

function gcodeText(time: string): string {
  return `; generated by OrcaSlicer 2.4.2 on 2026-11-06 at ${time}\nG1 X1\n`
}
