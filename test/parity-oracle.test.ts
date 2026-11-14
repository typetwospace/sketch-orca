// The oracle and the neutrality gate: the patches of parity/oracle/patches (numbered, dated, each
// with the header and the knob it adds or, for 0006 and 0007, the dump knob it works under; 0007's
// line rule for OracleDump.cpp), the knob names, the refusals of parity oracle (free space,
// the work folder's length, the platform), its build settings, the check of a staged oracle, the
// SOP1 reader and what parity neutral makes of runs and dumps. Hermetic: temporary folders, fake
// engine processes and hand-made SOP1 files only.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { orcaConfigureArgs, type OrcaArgs } from '../scripts/lib/cmake.ts'
import { sha256File, WINDOWS_WORK_PATH_MAX } from '../scripts/lib/paths.ts'
import { engineEnvProblems } from '../scripts/lib/parity/engineEnv.ts'
import { arenaReport, compareDumps, knobRunner, NEUTRAL_THREADS, neutralFiles, PINNED_KNOBS, pairReport, parseHead, parseRun, readSop1, sop1Differences, unfinishedRuns } from '../scripts/lib/parity/neutral.ts'
import {
  freeBytes,
  freeSpaceProblem,
  ORACLE_KNOB_EXIT,
  ORACLE_KNOBS,
  ORACLE_MAX_JOBS,
  ORACLE_MIN_FREE_BYTES,
  oracleConfigureArgs,
  oracleJobs,
  oraclePatches,
  oraclePatchesDir,
  oraclePatchHeader,
  oraclePatchProblems,
  oracleProblems,
  oracleRefusals,
  type OracleManifest,
} from '../scripts/lib/parity/oracle.ts'
import type { CaseRecord, RunProcess } from '../scripts/lib/parity/same.ts'
import { exitCodeFor } from '../scripts/orca.ts'

const ROOT = join(import.meta.dir, '..')
const GB = 1024 ** 3

let work: string
beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-oracle-'))
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

/** Which knob each patch adds. */
const PATCH_KNOBS: Record<string, string> = {
  '0001-oracle-clock.patch': ORACLE_KNOBS.time,
  '0002-oracle-seeds.patch': ORACLE_KNOBS.seed,
  '0003-oracle-threads.patch': ORACLE_KNOBS.threads,
  '0004-oracle-fma3.patch': ORACLE_KNOBS.fma3,
  '0005-oracle-dumps.patch': ORACLE_KNOBS.dump,
  // The front end's objects in full: no knob of its own, under 0005's dump folder.
  '0006-oracle-front-objects.patch': ORACLE_KNOBS.dump,
  // Every step boundary in full and the end of process(): no knob of its own either.
  '0007-oracle-step-dumps.patch': ORACLE_KNOBS.dump,
}

describe('the oracle patches', () => {
  const dir = oraclePatchesDir(ROOT)
  const names = readdirSync(dir).filter((n) => n.endsWith('.patch')).sort()

  test('parity/oracle/patches holds the seven, numbered from 0001 with no gap, and nothing else', () => {
    expect(names).toEqual(Object.keys(PATCH_KNOBS))
    expect(readdirSync(dir).sort()).toEqual(names)
    names.forEach((n, i) => expect(n.slice(0, 4)).toBe(String(i + 1).padStart(4, '0')))
  })

  test('each has the header, its own number, a real date, a description, a --- line and a diff', () => {
    for (const n of names) {
      const text = readFileSync(join(dir, n), 'utf8')
      expect({ n, problems: oraclePatchProblems(n, text) }).toEqual({ n, problems: [] })
      const h = oraclePatchHeader(text)!
      expect(h.number).toBe(n.slice(0, 4))
      expect(h.date).toMatch(/^20\d\d-\d\d-\d\d$/)
      const lines = text.split('\n')
      expect(lines[0]).toBe(`sketch-orca oracle patch ${h.number}: ${h.title}`)
      expect(lines[1]).toBe(`Modified for Sketch, ${h.date}.`)
      expect(lines[2]).toBe('')
      expect(lines[3]!.length).toBeGreaterThan(0)
    }
  })

  test('each names its knob in its title and reads it in its diff, says what happens unset or with a bad value, and adds no other knob', () => {
    for (const n of names) {
      const text = readFileSync(join(dir, n), 'utf8')
      const knob = PATCH_KNOBS[n]!
      const h = oraclePatchHeader(text)!
      expect({ n, title: h.title.includes(knob) }).toEqual({ n, title: true })
      const description = text.slice(0, text.search(/^---$/m))
      const diff = text.slice(text.search(/^diff --git /m))
      expect({ n, read: diff.includes(`"${knob}"`) }).toEqual({ n, read: true })
      // Each acts only when its knob is set, and a bad value stops the oracle with exit code 78.
      expect({ n, unset: /Unset, nothing (changes|is written and nothing changes)|as unset does/.test(description.replace(/\s+/g, ' ')) }).toEqual({ n, unset: true })
      expect({ n, exit: description.includes(`exit code ${ORACLE_KNOB_EXIT}`) }).toEqual({ n, exit: true })
      // The knobs a patch reads (getenv in its added lines) are its own; the RUN record of 0005 names the others.
      const added = diff.split('\n').filter((l) => l.startsWith('+') && !l.startsWith('+++')).join('\n')
      const getenvs = [...added.matchAll(/getenv\("(SKETCH_ORCA_ORACLE_[A-Z0-9_]+)"\)/g)].map((m) => m[1]!)
      for (const g of getenvs) expect({ n, g, own: g === knob }).toEqual({ n, g, own: true })
    }
  })

  test('the patches are LF and ASCII, and patch the clone only (no file of patches/, cli/ or this repository)', () => {
    for (const n of names) {
      const bytes = readFileSync(join(dir, n))
      expect({ n, cr: bytes.includes(13) }).toEqual({ n, cr: false })
      expect({ n, ascii: bytes.every((b) => b < 128) }).toEqual({ n, ascii: true })
      const files = [...bytes.toString('utf8').matchAll(/^\+\+\+ b\/(.+)$/gm)].map((m) => m[1]!)
      expect(files.length).toBeGreaterThan(0)
      for (const f of files) expect({ n, f, inClone: /^(src|deps_src|sketch-orca\/cli)\//.test(f) }).toEqual({ n, f, inClone: true })
    }
  })

  test('oraclePatches reads them, sorted, with their SHA-256', () => {
    const patches = oraclePatches(ROOT)
    expect(patches.map((p) => p.name)).toEqual(names)
    for (const p of patches) expect(p.sha256).toBe(sha256File(p.path))
  })

  test("0007 keeps OracleDump.cpp's lines 1-1017 where they were: five change in place, every new line comes after 1017", () => {
    const text = readFileSync(join(dir, '0007-oracle-step-dumps.patch'), 'utf8')
    // The file as 0005 makes it and 0006 changes it, then as 0007 leaves it, each patch's hunks
    // applied in memory: what is compared is the file, whatever shape git gave the diff.
    const patched = (n: string, base: string[]) => applyHunks(base, fileHunks(readFileSync(join(dir, n), 'utf8'), 'src/libslic3r/OracleDump.cpp'))
    const before = patched('0006-oracle-front-objects.patch', patched('0005-oracle-dumps.patch', []))
    const after = applyHunks(before, fileHunks(text, 'src/libslic3r/OracleDump.cpp'))
    expect(before.length).toBe(1017)
    expect(after.length).toBeGreaterThan(1017)
    const changed = before.flatMap((line, i) => (after[i] === line ? [] : [i + 1]))
    expect(changed).toEqual([18, 66, 770, 783, 1013])
    // Each changed line keeps what it held and adds to it.
    for (const n of changed) expect({ n, kept: after[n - 1]!.includes(before[n - 1]!.trim()) }).toEqual({ n, kept: true })
    // The end of process() is dumped after the conflict check, before the log line that ends it.
    const print = text.slice(text.indexOf('diff --git a/src/libslic3r/Print.cpp'))
    expect(print).toContain('+        Oracle::dump_print_step(*this, int(psConflictCheck));')
    // The next line is context: its mark, a space, then its own four spaces.
    expect(print).toMatch(/\+ {8}Oracle::dump_print_step\(\*this, int\(psConflictCheck\)\);\n {5}BOOST_LOG_TRIVIAL\(info\) << "Slicing process finished\."/)
  })
})

/** A hunk of a unified diff: the old file's first line (1-based; 0 for a new file) and its lines, each with its mark (' ', '-' or '+'). */
interface Hunk {
  oldStart: number
  lines: string[]
}

/** The hunks a patch gives the file `path` (its `diff --git a/<path>` section). */
function fileHunks(patch: string, path: string): Hunk[] {
  const start = patch.indexOf(`diff --git a/${path} `)
  expect({ path, found: start >= 0 }).toEqual({ path, found: true })
  const next = patch.indexOf('\ndiff --git ', start + 1)
  const hunks: Hunk[] = []
  for (const line of patch.slice(start, next < 0 ? undefined : next).split('\n')) {
    const h = /^@@ -(\d+)(?:,\d+)? \+\d+(?:,\d+)? @@/.exec(line)
    if (h) hunks.push({ oldStart: Number(h[1]), lines: [] })
    else if (hunks.length > 0 && /^[ +-]/.test(line)) hunks[hunks.length - 1]!.lines.push(line)
  }
  return hunks
}

/** `base` with `hunks` applied as git applies them at their line numbers; each context and removed line must be the base's. */
function applyHunks(base: string[], hunks: Hunk[]): string[] {
  const out: string[] = []
  let at = 0
  for (const h of hunks) {
    const from = Math.max(h.oldStart - 1, 0)
    out.push(...base.slice(at, from))
    at = from
    for (const line of h.lines) {
      const text = line.slice(1)
      if (line[0] === '+') out.push(text)
      else {
        expect({ line: at + 1, text: base[at] }).toEqual({ line: at + 1, text })
        if (line[0] === ' ') out.push(text)
        at += 1
      }
    }
  }
  return [...out, ...base.slice(at)]
}

describe('an oracle patch that is not right', () => {
  const good = 'sketch-orca oracle patch 0007: a knob.\nModified for Sketch, 2026-11-06.\n\nWhat it does.\n---\ndiff --git a/x b/x\n'

  test('a good one has no problem', () => {
    expect(oraclePatchProblems('0007-oracle-a-knob.patch', good)).toEqual([])
    expect(oraclePatchHeader(good)).toEqual({ number: '0007', title: 'a knob.', date: '2026-11-06' })
  })

  test('each fault is named', () => {
    const name = '0007-oracle-a-knob.patch'
    expect(oraclePatchProblems('7-knob.patch', good)[0]).toContain('NNNN-oracle-<words>.patch')
    expect(oraclePatchProblems('0008-oracle-a-knob.patch', good)).toEqual(['0008-oracle-a-knob.patch: the header says patch 0007, the name 0008'])
    expect(oraclePatchProblems(name, good.replace('2026-11-06', '2026-02-30'))).toEqual([`${name}: 2026-02-30 is not a date`])
    expect(oraclePatchProblems(name, good.replace('Modified for Sketch, 2026-11-06.\n', ''))[0]).toContain('the header is not')
    expect(oraclePatchProblems(name, good.replace('Modified for Sketch, 2026-11-06.', 'Modified, 2026-11-06.'))[0]).toContain('the header is not')
    expect(oraclePatchProblems(name, good.replace('---\n', ''))).toEqual([`${name}: no '---' line between the description and the diff`])
    expect(oraclePatchProblems(name, good.replace('diff --git a/x b/x\n', ''))).toEqual([`${name}: has no diff`])
    expect(oraclePatchProblems(name, good.replace('What it does.\n', 'What it does.\r\n'))).toEqual([`${name}: has CR bytes (the patches are LF, as the checkout is)`])
  })

  test('oraclePatches refuses a folder with a bad patch, or none', () => {
    const repo = join(work, 'repo')
    const dir = oraclePatchesDir(repo)
    mkdirSync(dir, { recursive: true })
    expect(() => oraclePatches(repo)).toThrow('no oracle patches')
    writeFileSync(join(dir, '0001-oracle-a.patch'), good.replace('0007', '0002'))
    expect(() => oraclePatches(repo)).toThrow('the header says patch 0002, the name 0001')
  })
})

describe('the knobs', () => {
  test('five, each SKETCH_ORCA_ORACLE_<NAME>', () => {
    expect(ORACLE_KNOBS).toEqual({
      time: 'SKETCH_ORCA_ORACLE_TIME',
      seed: 'SKETCH_ORCA_ORACLE_SEED',
      threads: 'SKETCH_ORCA_ORACLE_THREADS',
      fma3: 'SKETCH_ORCA_ORACLE_FMA3',
      dump: 'SKETCH_ORCA_ORACLE_DUMP',
    })
    expect(ORACLE_KNOB_EXIT).toBe(78)
  })

  test('the harness may give them to an engine: they are not the variables it refuses', () => {
    const env = Object.fromEntries(Object.values(ORACLE_KNOBS).map((k) => [k, '1']))
    expect(engineEnvProblems(env)).toEqual([])
  })

  test('parity/oracle/README.md explains each knob and says the oracle never ships; SOP1.md lists the RUN knobs', () => {
    const readme = readFileSync(join(ROOT, 'parity', 'oracle', 'README.md'), 'utf8')
    for (const k of Object.values(ORACLE_KNOBS)) expect(readme).toContain(`### \`${k}\``)
    expect(readme).toContain('**The oracle never ships.**')
    expect(readme).toContain('exits with 78')
    const sop1 = readFileSync(join(ROOT, 'parity', 'SOP1.md'), 'utf8')
    expect(sop1).toContain('`SKETCH_ORCA_ORACLE_TIME`, `_SEED`, `_THREADS` and `_FMA3`')
  })

  test('parity neutral pins the time and the seed, and compares 1 and 8 threads', () => {
    expect(PINNED_KNOBS).toEqual({ SKETCH_ORCA_ORACLE_TIME: '1767225600', SKETCH_ORCA_ORACLE_SEED: '1' })
    expect(new Date(Number(PINNED_KNOBS.SKETCH_ORCA_ORACLE_TIME) * 1000).toISOString()).toBe('2026-01-01T00:00:00.000Z')
    expect([...NEUTRAL_THREADS]).toEqual([1, 8])
  })
})

describe('parity oracle refuses to start', () => {
  const home = 'D:\\SketchDev\\orca-engine'

  test('with less than 40 GB free on the work folder\'s drive', () => {
    expect(ORACLE_MIN_FREE_BYTES).toBe(40 * GB)
    expect(freeSpaceProblem(40 * GB, home)).toBeUndefined()
    expect(freeSpaceProblem(40 * GB - 1, home)).toContain('parity oracle needs 40.0 GB free on the drive of')
    expect(oracleRefusals({ home, platform: 'win32', free: 12.5 * GB })).toEqual([`parity oracle needs 40.0 GB free on the drive of ${home} (the oracle's clone and build); there are 12.5 GB`])
  })

  test('with a work folder longer than WINDOWS_WORK_PATH_MAX', () => {
    const long = `C:\\${'x'.repeat(WINDOWS_WORK_PATH_MAX)}`
    const refused = oracleRefusals({ home: long, platform: 'win32', free: 100 * GB })
    expect(refused.length).toBe(1)
    expect(refused[0]).toContain(String(WINDOWS_WORK_PATH_MAX))
    // At the limit it starts.
    expect(oracleRefusals({ home: `C:\\${'x'.repeat(WINDOWS_WORK_PATH_MAX - 3)}`, platform: 'win32', free: 100 * GB })).toEqual([])
  })

  test('off Windows, and with every refusal at once', () => {
    expect(oracleRefusals({ home, platform: 'win32', free: 100 * GB })).toEqual([])
    expect(oracleRefusals({ home: '/srv/sketch-dev/orca-engine', platform: 'linux', free: 100 * GB })[0]).toContain('builds on Windows only')
    expect(oracleRefusals({ home: `C:\\${'x'.repeat(100)}`, platform: 'win32', free: 0 }).length).toBe(2)
  })

  test('freeBytes reads the drive of the nearest folder that exists', () => {
    expect(freeBytes(work)).toBeGreaterThan(0)
    expect(freeBytes(join(work, 'not', 'made', 'yet'))).toBe(freeBytes(work))
  })
})

describe('the oracle build', () => {
  const args: OrcaArgs = {
    ninja: 'C:/ninja.exe',
    sourceDir: 'W/oracle/src',
    buildDir: 'W/oracle/build',
    depsPrefix: 'W/deps/OrcaSlicer_dep/usr/local',
    cliDir: 'W/oracle/src/sketch-orca/cli',
    commit: 'c'.repeat(40),
    tag: 'v2.4.2',
    ignorePrefixes: [],
    platform: 'win32',
  }

  test("the engine's configuration, Ninja and the static runtime, plus the linker map", () => {
    const engine = orcaConfigureArgs(args)
    const oracle = oracleConfigureArgs(args)
    expect(oracle.slice(0, engine.length)).toEqual(engine)
    expect(oracle.slice(engine.length)).toEqual(['-DCMAKE_EXE_LINKER_FLAGS_INIT=/MAP'])
    expect(oracle).toContain('Ninja')
    expect(oracle).toContain('-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded')
    expect(oracleConfigureArgs({ ...args, platform: 'linux' })).toEqual(orcaConfigureArgs({ ...args, platform: 'linux' }))
  })

  test('at most 8 jobs, whatever SKETCH_ORCA_JOBS says', () => {
    expect(ORACLE_MAX_JOBS).toBe(8)
    expect(oracleJobs({ SKETCH_ORCA_JOBS: '32' })).toBe(8)
    expect(oracleJobs({ SKETCH_ORCA_JOBS: '3' })).toBe(3)
    expect(oracleJobs({})).toBeLessThanOrEqual(8)
  })

  test('parity exits 1 when a gate fails (parity neutral); its other codes are failures', () => {
    expect(exitCodeFor('parity', 0)).toBe(0)
    expect(exitCodeFor('parity', 1)).toBe(1)
    expect(exitCodeFor('parity', 2)).toBe(3)
    expect(exitCodeFor('parity', 78)).toBe(3)
  })
})

describe('a staged oracle', () => {
  function stage(): { dist: string; manifest: OracleManifest } {
    const dist = join(work, 'dist')
    mkdirSync(join(dist, 'bin'), { recursive: true })
    for (const r of ['profiles', 'info', 'flush']) mkdirSync(join(dist, 'resources', r), { recursive: true })
    writeFileSync(join(dist, 'bin', 'sketch-orca-oracle.exe'), 'exe')
    writeFileSync(join(dist, 'bin', 'sketch-orca-oracle.map'), 'map')
    writeFileSync(join(dist, 'bin', 'TKernel.dll'), 'dll')
    const sha = (p: string) => sha256File(join(dist, p))
    const manifest: OracleManifest = {
      format: 'sketch-orca-oracle/1',
      stagedAt: '2026-11-06T00:00:00.000Z',
      platform: 'win32',
      orca: { tag: 'v2.4.2', commit: 'c'.repeat(40) },
      patches: [{ name: '0001-a.patch', sha256: 'a' }],
      oraclePatches: [{ name: '0001-oracle-clock.patch', sha256: 'b' }],
      cli: 'cli',
      sourceDigest: 'src',
      depsInputs: 'deps',
      configure: [],
      knobs: Object.values(ORACLE_KNOBS),
      exe: { path: 'bin/sketch-orca-oracle.exe', sha256: sha('bin/sketch-orca-oracle.exe') },
      map: { path: 'bin/sketch-orca-oracle.map', sha256: sha('bin/sketch-orca-oracle.map') },
      runtime: [{ name: 'TKernel.dll', sha256: sha('bin/TKernel.dll') }],
      resources: [{ name: 'profiles', sha256: 'p' }],
    }
    return { dist, manifest }
  }
  const want = { commit: 'c'.repeat(40), patches: [{ name: '0001-a.patch', sha256: 'a' }], oraclePatches: [{ name: '0001-oracle-clock.patch', sha256: 'b' }], cli: 'cli' }

  test('is the one this checkout makes when its inputs and files are what oracle.json lists', () => {
    const { dist, manifest } = stage()
    expect(oracleProblems(dist, manifest, want)).toEqual([])
  })

  test('is not when the commit, patches/, the oracle patches, cli/ or a file differ', () => {
    const { dist, manifest } = stage()
    expect(oracleProblems(dist, manifest, { ...want, commit: 'd'.repeat(40) })[0]).toContain('the lock is at')
    expect(oracleProblems(dist, manifest, { ...want, patches: [] })).toEqual(['it is built with other patches than patches/ has now'])
    expect(oracleProblems(dist, manifest, { ...want, oraclePatches: [{ name: '0001-oracle-clock.patch', sha256: 'x' }] })).toEqual(['it is built with other oracle patches than parity/oracle/patches has now'])
    expect(oracleProblems(dist, manifest, { ...want, cli: 'other' })).toEqual(['it is built from another cli/'])
    writeFileSync(join(dist, 'bin', 'TKernel.dll'), 'changed')
    rmSync(join(dist, 'bin', 'sketch-orca-oracle.map'))
    expect(oracleProblems(dist, manifest, want)).toEqual(['bin/sketch-orca-oracle.map is missing', 'bin/TKernel.dll is not the file oracle.json lists'])
  })
})

// ---------------------------------------------------------------- SOP1

/** Writes an SOP1 file as OracleDump.cpp does. */
class Sop1Writer {
  parts: Buffer[] = []
  count = 0
  constructor(version = 1) {
    const v = Buffer.alloc(4)
    v.writeUInt32LE(version)
    this.parts.push(Buffer.from('SOP1', 'latin1'), v)
  }
  record(kind: string, payload: Buffer): this {
    const len = Buffer.alloc(8)
    len.writeBigUInt64LE(BigInt(payload.length))
    this.parts.push(Buffer.from(kind, 'latin1'), len, payload)
    this.count++
    return this
  }
  end(count = this.count): Buffer {
    const p = Buffer.alloc(8)
    p.writeBigUInt64LE(BigInt(count))
    this.record('END ', p)
    return Buffer.concat(this.parts)
  }
}

const u32 = (v: number) => {
  const b = Buffer.alloc(4)
  b.writeUInt32LE(v)
  return b
}
const i32 = (v: number) => {
  const b = Buffer.alloc(4)
  b.writeInt32LE(v)
  return b
}
const u64 = (v: number) => {
  const b = Buffer.alloc(8)
  b.writeBigUInt64LE(BigInt(v))
  return b
}
const i64 = (v: number) => {
  const b = Buffer.alloc(8)
  b.writeBigInt64LE(BigInt(v))
  return b
}
const str = (s: string) => Buffer.concat([u32(Buffer.byteLength(s)), Buffer.from(s, 'utf8')])
const bool = (v: boolean) => Buffer.from([v ? 1 : 0])

function headPayload(source: string, name: string, object: number, occurrence: number): Buffer {
  return Buffer.concat([str(source), str(name), i64(object), u32(occurrence)])
}

function runPayload(threads: string | undefined, maxConcurrency: number, parallelism: number, fma3: number): Buffer {
  const knobs: [string, string | undefined][] = [
    ['SKETCH_ORCA_ORACLE_TIME', '1767225600'],
    ['SKETCH_ORCA_ORACLE_SEED', '1'],
    ['SKETCH_ORCA_ORACLE_THREADS', threads],
    ['SKETCH_ORCA_ORACLE_FMA3', undefined],
  ]
  return Buffer.concat([u64(knobs.length), ...knobs.flatMap(([k, v]) => [str(k), bool(v !== undefined), str(v ?? '')]), i32(maxConcurrency), u64(parallelism), i32(fma3)])
}

function dump(threads: string, maxConcurrency: number, layer: Buffer): Buffer {
  return new Sop1Writer().record('HEAD', headPayload('object', 'posSlice', 0, 1)).record('RUN ', runPayload(threads, maxConcurrency, 1, 1)).record('OBJ ', i64(0)).record('LAYR', layer).end()
}

describe('SOP1', () => {
  test('reads the container: magic, version, records, END', () => {
    const f = readSop1(dump('1', 1, Buffer.from([1, 2, 3])))
    expect(f.version).toBe(1)
    expect(f.records.map((r) => r.kind)).toEqual(['HEAD', 'RUN ', 'OBJ ', 'LAYR'])
    expect(parseHead(f.records[0]!.payload)).toEqual({ source: 'object', name: 'posSlice', object: 0n, occurrence: 1 })
    expect(parseRun(f.records[1]!.payload)).toEqual({
      knobs: [
        { name: 'SKETCH_ORCA_ORACLE_TIME', set: true, value: '1767225600' },
        { name: 'SKETCH_ORCA_ORACLE_SEED', set: true, value: '1' },
        { name: 'SKETCH_ORCA_ORACLE_THREADS', set: true, value: '1' },
        { name: 'SKETCH_ORCA_ORACLE_FMA3', set: false, value: '' },
      ],
      maxConcurrency: 1,
      maxAllowedParallelism: 1,
      fma3Enable: 1,
    })
    expect(parseHead(headPayload('print', 'psSkirtBrim', -1, 2)).object).toBe(-1n)
  })

  test('refuses what is not a whole SOP1 file of version 1', () => {
    const good = dump('1', 1, Buffer.alloc(0))
    expect(() => readSop1(Buffer.concat([Buffer.from('SOP2'), good.subarray(4)]))).toThrow('no SOP1 magic')
    expect(() => readSop1(new Sop1Writer(2).end())).toThrow('SOP1 version 2')
    expect(() => readSop1(good.subarray(0, good.length - 3))).toThrow()
    expect(() => readSop1(new Sop1Writer().record('HEAD', Buffer.alloc(0)).end(5))).toThrow('the END record counts 5 records, the file has 1')
    expect(() => readSop1(Buffer.concat([good, Buffer.from([0])]))).toThrow('1 bytes after the END record')
    expect(() => readSop1(new Sop1Writer().record('HEAD', Buffer.alloc(0)).parts.reduce((a, b) => Buffer.concat([a, b]), Buffer.alloc(0)))).toThrow('no END record')
    expect(() => parseRun(Buffer.concat([runPayload('1', 1, 1, 1), Buffer.from([0])]))).toThrow('longer than its fields')
  })

  test('two dumps differ where a record other than RUN differs', () => {
    const a = readSop1(dump('1', 1, Buffer.from([1, 2])))
    const b = readSop1(dump('8', 8, Buffer.from([1, 2])))
    const c = readSop1(dump('8', 8, Buffer.from([1, 3])))
    expect(sop1Differences(a, b)).toEqual([])
    expect(sop1Differences(a, c)).toEqual(['record 3 (LAYR)'])
  })
})

describe('parity neutral', () => {
  test('knobRunner adds the knobs, and a new dump folder per engine run of each case', async () => {
    const seen: { cwd: string; env: Record<string, string> }[] = []
    const fake: RunProcess = async (_exe, _args, o) => {
      seen.push({ cwd: o.cwd, env: { ...o.env } as Record<string, string> })
      return { code: 0, timedOut: false, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0) }
    }
    const dumps = join(work, 'dumps')
    const run = knobRunner({ ...PINNED_KNOBS, SKETCH_ORCA_ORACLE_THREADS: '8' }, dumps, fake)
    const o = (c: string) => ({ cwd: join(work, 'cases', c), env: { PATH: 'x' }, timeoutMs: 1000 })
    await run('e', [], o('cube'))
    await run('e', [], o('cube'))
    await run('e', [], o('info'))
    expect(seen.map((s) => s.env.SKETCH_ORCA_ORACLE_DUMP)).toEqual([join(dumps, 'cube', '0'), join(dumps, 'cube', '1'), join(dumps, 'info', '0')])
    for (const s of seen) {
      expect(s.env.SKETCH_ORCA_ORACLE_TIME).toBe('1767225600')
      expect(s.env.SKETCH_ORCA_ORACLE_THREADS).toBe('8')
      expect(s.env.PATH).toBe('x')
      expect(readdirSync(s.env.SKETCH_ORCA_ORACLE_DUMP!)).toEqual([])
    }
    const plain = knobRunner({}, undefined, fake)
    await plain('e', [], o('cube'))
    expect(seen[3]!.env).toEqual({ PATH: 'x' })
  })

  test('compares the dumps of two thread counts and reads what TBB reported from their RUN records', () => {
    const files = neutralFiles({ parity: join(work, 'parity'), runs: join(work, 'parity', 'runs') })
    const put = (root: string, rel: string, bytes: Buffer) => {
      mkdirSync(join(root, rel, '..'), { recursive: true })
      writeFileSync(join(root, rel), bytes)
    }
    put(files.dumps.t1, 'cube/0/object0-posSlice-1.sop1', dump('1', 1, Buffer.from([1])))
    put(files.dumps.t8, 'cube/0/object0-posSlice-1.sop1', dump('8', 8, Buffer.from([1])))
    put(files.dumps.t1, 'cube/0/object0-posPerimeters-1.sop1', dump('1', 1, Buffer.from([1])))
    put(files.dumps.t8, 'cube/0/object0-posPerimeters-1.sop1', dump('8', 8, Buffer.from([2])))
    put(files.dumps.t8, 'cube/0/print-psSkirtBrim-1.sop1', dump('8', 4, Buffer.from([2])))
    put(files.dumps.t1, 'cube/1/front-config-1.sop1', Buffer.from('nope'))
    put(files.dumps.t8, 'cube/1/front-config-1.sop1', dump('8', 8, Buffer.alloc(0)))
    const d = compareDumps(files.dumps.t1, files.dumps.t8)
    expect(d.compared).toBe(3)
    expect(d.identical).toBe(1)
    expect(d.different).toEqual([{ file: 'cube/0/object0-posPerimeters-1.sop1', records: ['record 3 (LAYR)'] }])
    expect(d.only).toEqual(['cube/0/print-psSkirtBrim-1.sop1'])
    expect(d.unreadable).toEqual(['cube/1/front-config-1.sop1: not an SOP1 file (no SOP1 magic)'])
    const a1 = arenaReport(files.dumps.t1, 1)
    expect({ dumps: a1.dumps, reportsP: a1.reportsP, oneThread: a1.oneThread, unreadable: a1.unreadable.length }).toEqual({ dumps: 2, reportsP: true, oneThread: true, unreadable: 1 })
    const a8 = arenaReport(files.dumps.t8, 8)
    expect(a8.reportsP).toBe(false)
    expect(a8.seen.map((s) => [s.maxConcurrency, s.dumps])).toEqual([
      [8, 3],
      [4, 1],
    ])
    expect(arenaReport(join(work, 'none'), 8)).toEqual({ threads: 8, dumps: 0, seen: [], reportsP: false, oneThread: false, unreadable: [] })
  })

  test('a pair report names its sides, counts what differs, and finds the runs that timed out', () => {
    const stores: [string, string] = [join(work, 'a'), join(work, 'b')]
    const run = (index: number, timedOut = false) => ({ index, args: ['--x'], extraEnv: [], code: timedOut ? null : 0, timedOut, files: [], gone: [] })
    for (const [i, s] of stores.entries()) {
      mkdirSync(join(s, 'cube', '0'), { recursive: true })
      writeFileSync(join(s, 'cube', '0', 'stdout'), i === 0 ? 'same' : 'other')
      writeFileSync(join(s, 'cube', '0', 'stderr'), '')
    }
    const a: CaseRecord[] = [{ id: 'cube', ok: true, problems: [], warnings: [], runs: [run(0)] }]
    const b: CaseRecord[] = [{ id: 'cube', ok: false, problems: ['p'], warnings: [], runs: [run(0, true)] }]
    const r = pairReport({ a, b, stores, sides: [{ label: 'oracle-threads-1', knobs: {} }, { label: 'oracle-threads-8', knobs: {} }], masks: [], masksFile: null })
    expect(r.summary).toEqual({ cases: 1, runs: 1, outputs: 3, identical: 1, masked: 0, different: 2 })
    expect(r.different).toEqual(['cube/0/exit', 'cube/0/stdout'])
    expect(r.cases[0]!.smoke).toEqual({ 'oracle-threads-1': { ok: true, problems: [] }, 'oracle-threads-8': { ok: false, problems: ['p'] } })
    expect(unfinishedRuns('threads-8', b)).toEqual(['threads-8/cube/0'])
    expect(unfinishedRuns('threads-1', a)).toEqual([])
  })
})
