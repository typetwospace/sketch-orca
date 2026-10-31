// protocol/: the JSON Schemas take every example and refuse every invalid one, and the engine's
// own validators (scripts/lib/job.ts, stage.ts, smoke.ts) agree with them on each. The lists the
// specification and the schemas give (commands, error codes, warnings, stages, option types) are
// the ones the engine's sources use.

import { describe, expect, test } from 'bun:test'
import { readdirSync, readFileSync } from 'node:fs'
import { join, posix, win32 } from 'node:path'
import { Ajv2020, type ValidateFunction } from 'ajv/dist/2020'
import { CUSTOM_GCODE_TYPES, JOB_COMMANDS, JOB_FORMAT, MODEL_EXTENSIONS, ORCA_EXIT_CODES, OVERRIDE_KEY_RE, parseProgress, RESERVED_OVERRIDES, RESULT_FORMAT, validateJob, validateResult } from '../scripts/lib/job.ts'
import { parseConfigDef } from '../scripts/lib/smoke.ts'
import { ENGINE_FORMAT, isSafeRelPath, parseVersionOutput, STAGE_VERSION, validateManifest } from '../scripts/lib/stage.ts'

const ROOT = join(import.meta.dir, '..')
const PROTOCOL = join(ROOT, 'protocol')
const EXAMPLES = join(PROTOCOL, 'examples')
const KINDS = ['engine-manifest', 'config-def', 'progress', 'version', 'result', 'job'] as const
type Kind = (typeof KINDS)[number]

const read = (path: string) => readFileSync(path, 'utf8')
const readJson = (path: string): unknown => JSON.parse(read(path))
const schemaOf = (k: Kind) => readJson(join(PROTOCOL, 'schema', `${k}.schema.json`)) as Record<string, any>
const idOf = (k: Kind) => `urn:sketch-orca:schema:${k}:1`

// Strict, so a keyword the schemas misspell or misplace is an error. Two things strict mode would
// refuse are plain JSON Schema and allowed by name: union types ("string" or "null"), and a
// prefixItems that fixes only the first items of a longer list (a layer profile starts at 0).
const ajv = new Ajv2020({ strict: true, allowUnionTypes: true, strictTuples: false, allErrors: true })
for (const k of KINDS) ajv.addSchema(schemaOf(k))
const validate = Object.fromEntries(KINDS.map((k) => [k, ajv.getSchema(idOf(k))!])) as Record<Kind, ValidateFunction>
const absolutePath = ajv.compile({ $ref: `${idOf('job')}#/$defs/absolutePath` })
const relativePath = ajv.compile({ $ref: `${idOf('engine-manifest')}#/$defs/relativePath` })

/** The format an example is of, by its file name: job-slice.json is a job, progress.ndjson progress lines. */
function kindOf(file: string): Kind {
  const k = KINDS.find((s) => file.startsWith(`${s}.`) || file.startsWith(`${s}-`))
  if (!k) throw new Error(`${file}: no schema for it`)
  return k
}

function schemaErrors(kind: Kind, path: string): string[] {
  const values = kind === 'progress' ? read(path).split(/\r?\n/).filter((l) => l.trim() !== '').map((l) => JSON.parse(l) as unknown) : [readJson(path)]
  return values.flatMap((v) => (validate[kind](v) ? [] : (validate[kind].errors ?? []).map((e) => `${e.instancePath} ${e.message}`)))
}

/** What the engine's own validator says of a file: undefined when it takes it, else why not. */
function ownRefusal(kind: Kind, path: string): string | undefined {
  try {
    const text = read(path)
    if (kind === 'job') validateJob(JSON.parse(text))
    else if (kind === 'result') validateResult(JSON.parse(text))
    else if (kind === 'progress') parseProgress(text)
    else if (kind === 'engine-manifest') validateManifest(JSON.parse(text))
    else if (kind === 'config-def') parseConfigDef(JSON.parse(text))
    else if (parseVersionOutput(text) === undefined) return 'not a JSON object'
    return undefined
  } catch (err) {
    return (err as Error).message
  }
}

const files = (dir: string) => readdirSync(dir, { withFileTypes: true }).filter((d) => d.isFile()).map((d) => d.name).sort()
const valid = files(EXAMPLES)
const invalid = files(join(EXAMPLES, 'invalid'))

function job(over: Record<string, unknown>): Record<string, unknown> {
  return { ...(readJson(join(EXAMPLES, 'job-info.json')) as Record<string, unknown>), ...over }
}

describe('the protocol examples', () => {
  test('there are examples of every format, and invalid ones of the job, result, progress and manifest', () => {
    expect(new Set(valid.map(kindOf))).toEqual(new Set(KINDS))
    expect(new Set(invalid.map(kindOf))).toEqual(new Set<Kind>(['job', 'result', 'progress', 'engine-manifest']))
  })

  for (const file of valid) {
    test(`${file} follows its schema, and the engine's validator takes it`, () => {
      const path = join(EXAMPLES, file)
      expect(schemaErrors(kindOf(file), path)).toEqual([])
      expect(ownRefusal(kindOf(file), path)).toBeUndefined()
    })
  }

  for (const file of invalid) {
    test(`invalid/${file} is refused by its schema and by the engine's validator`, () => {
      const path = join(EXAMPLES, 'invalid', file)
      expect(schemaErrors(kindOf(file), path)).not.toEqual([])
      expect(ownRefusal(kindOf(file), path)).toBeString()
    })
  }

  test("the results' engine is what version prints", () => {
    for (const file of valid.filter((f) => kindOf(f) === 'result')) {
      const engine = (readJson(join(EXAMPLES, file)) as Record<string, unknown>).engine
      if (engine !== undefined) expect(validate.version(engine)).toBe(true)
    }
  })

  test('the config-def example lists count options, each read by the engine\'s reader', () => {
    const defs = readJson(join(EXAMPLES, 'config-def.json')) as { count: number; options: { key: string }[] }
    expect(defs.options).toHaveLength(defs.count)
    expect([...parseConfigDef(defs).keys()]).toEqual(defs.options.map((o) => o.key))
  })

  test('the manifest example: its exe is one of its files, which are sorted, at the current stage version', () => {
    const m = validateManifest(readJson(join(EXAMPLES, 'engine-manifest.json')))
    expect(m.files.some((f) => f.path === m.engine.exe)).toBe(true)
    expect(m.files.map((f) => f.path)).toEqual(m.files.map((f) => f.path).sort())
    expect(m.stageVersion).toBe(STAGE_VERSION)
  })
})

describe('the schemas and the validators take the same values', () => {
  test('formats, commands, custom G-code types and model files', () => {
    expect(schemaOf('job').properties.format.const).toBe(JOB_FORMAT)
    expect(schemaOf('result').properties.format.const).toBe(RESULT_FORMAT)
    expect(schemaOf('engine-manifest').properties.format.const).toBe(ENGINE_FORMAT)
    expect(schemaOf('job').properties.command.enum).toEqual([...JOB_COMMANDS])
    expect(schemaOf('result').properties.command.enum).toEqual([...JOB_COMMANDS])
    expect(schemaOf('job').$defs.customGcode.properties.type.enum).toEqual([...CUSTOM_GCODE_TYPES])
    for (const ext of MODEL_EXTENSIONS) {
      for (const name of [`/m/part${ext}`, `/m/part${ext.toUpperCase()}`]) {
        const j = job({ model: { path: name } })
        expect(validate.job(j)).toBe(true)
        expect(() => validateJob(j)).not.toThrow()
      }
    }
    const other = job({ model: { path: '/m/part.step' } })
    expect(validate.job(other)).toBe(false)
    expect(() => validateJob(other)).toThrow()
  })

  test('override keys: the shape of Orca\'s keys, never a key of the preset files in any letter case', () => {
    expect(schemaOf('job').$defs.overrides.propertyNames.pattern).toBe(OVERRIDE_KEY_RE.source)
    const keys = ['layer_height', 'required_nozzle_HRC', 'Layer_Height', 'x1', '_x', '1x', 'a-b', 'a b', 'names', 'typed']
    for (const r of RESERVED_OVERRIDES) keys.push(r, r.toUpperCase(), r.charAt(0).toUpperCase() + r.slice(1))
    for (const key of keys) {
      const j = job({ overrides: { [key]: '1' } })
      let own = true
      try {
        validateJob(j)
      } catch {
        own = false
      }
      expect({ key, schema: validate.job(j) }).toEqual({ key, schema: own })
    }
  })

  test('override values: text, numbers and booleans', () => {
    for (const value of ['15%', 0.2, 3, true, false]) {
      const j = job({ overrides: { wall_loops: value } })
      expect(validate.job(j)).toBe(true)
      expect(() => validateJob(j)).not.toThrow()
    }
    for (const value of [null, [1], { a: 1 }]) {
      const j = job({ overrides: { wall_loops: value } })
      expect(validate.job(j)).toBe(false)
      expect(() => validateJob(j)).toThrow()
    }
  })

  test('an absolute path is one on Windows or on POSIX systems; the validator asks the system it runs on', () => {
    for (const p of ['/jobs/1', '\\jobs\\1', 'C:\\jobs\\1', 'c:/jobs/1', '\\\\server\\share\\1', 'jobs/1', 'C:jobs', './jobs', '']) {
      expect({ p, schema: absolutePath(p) }).toEqual({ p, schema: win32.isAbsolute(p) || posix.isAbsolute(p) })
      if (process.platform === 'win32') expect({ p, own: !throws(() => validateJob(job({ workDir: p }))) }).toEqual({ p, own: win32.isAbsolute(p) })
    }
  })

  test('a manifest path is isSafeRelPath', () => {
    for (const p of ['bin/sketch-orca.exe', 'LICENSES/deps/GMP/gmp.README', '.hidden', 'a/...', '', '/bin/x', 'C:/x', 'c:x', 'bin\\x', 'a//b', './a', 'a/./b', 'a/..', '..', 'a/'])
      expect({ p, schema: relativePath(p) }).toEqual({ p, schema: isSafeRelPath(p) })
  })

  test('custom G-codes: what each type needs', () => {
    const cases: [Record<string, unknown>, boolean][] = [
      [{ printZ: 1, type: 'pause' }, true],
      [{ printZ: 1, type: 'pause', note: null, extruder: null, color: null, gcode: null }, true],
      [{ printZ: 0, type: 'pause' }, false],
      [{ printZ: 1, type: 'color', color: '#A0b0C0' }, true],
      [{ printZ: 1, type: 'color' }, false],
      [{ printZ: 1, type: 'color', color: 'red' }, false],
      [{ printZ: 1, type: 'filament', extruder: 2 }, true],
      [{ printZ: 1, type: 'filament', extruder: 0 }, false],
      [{ printZ: 1, type: 'filament', extruder: 1.5 }, false],
      [{ printZ: 1, type: 'filament', extruder: null }, false],
      [{ printZ: 1, type: 'custom', gcode: 'M117 hi' }, true],
      [{ printZ: 1, type: 'custom', gcode: ' \n ' }, false],
      [{ printZ: 1, type: 'custom' }, false],
      [{ printZ: 1, type: 'wipe' }, false],
    ]
    for (const [item, ok] of cases) {
      const j = job({ customGcodes: [item] })
      expect({ item, schema: validate.job(j), own: !throws(() => validateJob(j)) }).toEqual({ item, schema: ok, own: ok })
    }
  })

  test('layer heights: fixed, adaptive within 0..1, a profile from z 0', () => {
    const cases: [unknown, boolean][] = [
      [null, true],
      [{ mode: 'fixed' }, true],
      [{ mode: 'adaptive' }, true],
      [{ mode: 'adaptive', quality: 1 }, true],
      [{ mode: 'adaptive', quality: 1.5 }, false],
      [{ mode: 'profile', profile: [0, 0.2, 10, 0.2] }, true],
      [{ mode: 'profile', profile: [0, 0.2] }, false],
      [{ mode: 'profile', profile: [1, 0.2, 10, 0.2] }, false],
      [{ mode: 'profile' }, false],
      [{ mode: 'variable' }, false],
    ]
    for (const [layerHeights, ok] of cases) {
      const j = job({ objects: [{ index: 0, layerHeights }] })
      expect({ layerHeights, schema: validate.job(j), own: !throws(() => validateJob(j)) }).toEqual({ layerHeights, schema: ok, own: ok })
    }
  })

  test('a slice needs its G-code file; layers and info do not', () => {
    for (const [command, output, ok] of [
      ['slice', { gcode: '/o/a.gcode' }, true],
      ['slice', { gcode: '/o/a.txt' }, false],
      ['slice', { gcode: '' }, false],
      ['slice', null, false],
      ['layers', null, true],
      ['layers', { gcode: '', bambu3mf: null, stats: null }, true],
      ['info', { bambu3mf: '/o/a.gcode.3mf' }, true],
      ['info', { bambu3mf: '/o/a.zip' }, false],
    ] as [string, unknown, boolean][]) {
      const j = job({ command, output })
      expect({ command, output, schema: validate.job(j), own: !throws(() => validateJob(j)) }).toEqual({ command, output, schema: ok, own: ok })
    }
  })

  test('a result: ok with code ok and exit code 0, a failure with neither', () => {
    const base = { format: RESULT_FORMAT, message: '' }
    for (const [r, ok] of [
      [{ ok: true, code: 'ok' }, true],
      [{ ok: true, code: 'ok', exitCode: 0 }, true],
      [{ ok: false, code: 'slice-failed', exitCode: -100, objectIndex: 0 }, true],
      [{ ok: false, code: 'ok' }, false],
      [{ ok: false, code: 'slice-failed', exitCode: 0 }, false],
      [{ ok: true, code: 'ok', exitCode: 3 }, false],
      [{ ok: false, code: 'slice-failed', objectIndex: -1 }, false],
      [{ ok: 'yes', code: 'ok' }, false],
    ] as [Record<string, unknown>, boolean][]) {
      const v = { ...base, ...r }
      expect({ r, schema: validate.result(v), own: !throws(() => validateResult(v)) }).toEqual({ r, schema: ok, own: ok })
    }
  })
})

describe('the specification names what the engine does', () => {
  const spec = read(join(PROTOCOL, 'README.md'))
  const cpp = (name: string) => read(join(ROOT, 'cli', name))

  test('the error table is the engine\'s failure table (cli/common.cpp), with the stock exit codes', () => {
    const engine = [...cpp('common.cpp').matchAll(/^\s*\{"([a-z0-9-]+)", (-?\d+)\},/gm)].map((m) => [m[1]!, Number(m[2])] as const).filter(([c]) => c !== 'ok')
    const table = [...spec.matchAll(/^\| `([a-z0-9-]+)` \| (-?\d+) \|/gm)].map((m) => [m[1]!, Number(m[2])] as const)
    expect(engine.length).toBeGreaterThan(20)
    expect(table).toEqual(engine)
    for (const [, exit] of table) expect(ORCA_EXIT_CODES[exit]).toBeString()
  })

  test('the warnings it lists are the ones the engine adds', () => {
    const added = new Set<string>()
    for (const f of readdirSync(join(ROOT, 'cli')).filter((n) => n.endsWith('.cpp'))) for (const m of cpp(f).matchAll(/warnings\.add\("([a-z0-9-]+)"/g)) added.add(m[1]!)
    const paragraph = spec.slice(spec.indexOf('Their codes include'), spec.indexOf('a client shows the message'))
    const listed = new Set([...paragraph.matchAll(/`([a-z0-9-]+)`/g)].map((m) => m[1]!))
    expect(listed).toEqual(added)
  })

  test('the progress stages it lists are the ones the engine writes, in order', () => {
    const run = cpp('run.cpp')
    const written = new Set([...run.matchAll(/progress\.line\("([a-z0-9]+)"/g)].map((m) => m[1]!))
    for (const s of ['done', 'failed']) expect(cpp('common.cpp')).toContain(`"${s}"`)
    const paragraph = spec.slice(spec.indexOf('The stages come in this order'), spec.indexOf('The percent never goes down'))
    const listed = [...paragraph.matchAll(/`([a-z0-9]+)`/g)].map((m) => m[1]!)
    expect(listed.slice(-2)).toEqual(['done', 'failed'])
    expect(new Set(listed.slice(0, -2))).toEqual(written)
    expect(schemaOf('progress').properties.stage.description).toContain(listed.slice(0, -2).join(', '))
  })

  test('the config-def schema names the types, modes and GUI types cli/config_def.cpp writes', () => {
    const source = cpp('config_def.cpp')
    const returns = (fn: string) => {
      const start = source.indexOf(`const char *${fn}(`)
      const body = source.slice(start, source.indexOf('\n}\n', start))
      return [...body.matchAll(/return "([A-Za-z0-9_]*)";/g)].map((m) => m[1]!).filter((s) => s !== '')
    }
    const option = schemaOf('config-def').$defs.option.properties
    expect(new Set(option.type.enum)).toEqual(new Set(returns('type_name')))
    expect(new Set(option.mode.enum)).toEqual(new Set(returns('mode_name')))
    expect(new Set(option.guiType.enum)).toEqual(new Set(returns('gui_type_name')))
    const written = [...source.matchAll(/o\["([A-Za-z]+)"\]/g)].map((m) => m[1]!)
    expect(new Set(Object.keys(option))).toEqual(new Set(written))
  })

  test('the version schema names the fields cli/run.cpp writes', () => {
    const run = cpp('run.cpp')
    const body = run.slice(run.indexOf('json version_json()'), run.indexOf('\n}\n', run.indexOf('json version_json()')))
    const written = [...body.matchAll(/j\["([A-Za-z]+)"\]/g)].map((m) => m[1]!)
    expect(schemaOf('version').required).toEqual(written)
  })

  test('it is under the MIT licence, and says so', () => {
    expect(read(join(PROTOCOL, 'LICENSE'))).toStartWith('MIT License\n\nCopyright (c) 2026 The sketch-orca authors\n')
    expect(spec).toContain('This folder is licensed under the MIT licence (`LICENSE` here)')
  })
})

function throws(f: () => unknown): boolean {
  try {
    f()
    return false
  } catch {
    return true
  }
}
