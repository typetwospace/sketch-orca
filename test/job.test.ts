import { afterAll, describe, expect, test } from 'bun:test'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import {
  JOB_FORMAT,
  JobError,
  ORCA_EXIT_CODES,
  OVERRIDE_KEY_RE,
  RESERVED_OVERRIDES,
  RESULT_FORMAT,
  normalizeLayerHeights,
  normalizeOverrides,
  parseProgress,
  parseResult,
  readJob,
  readResult,
  serializeJob,
  validateJob,
  validateResult,
  writeJob,
} from '../scripts/lib/job.ts'

const base = resolve(tmpdir(), 'sketch-orca-job-test')
const p = (...parts: string[]) => join(base, ...parts)

function job(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    format: JOB_FORMAT,
    command: 'slice',
    resources: p('resources'),
    workDir: p('work'),
    model: { path: p('model.stl') },
    presets: { machine: p('machine.json'), process: p('process.json'), filaments: [p('filament_1.json')] },
    output: { gcode: p('out.gcode') },
    ...extra,
  }
}

const scratch = mkdtempSync(join(tmpdir(), 'sketch-orca-job-'))
afterAll(() => rmSync(scratch, { recursive: true, force: true }))

describe('validateJob', () => {
  test('fills in the defaults', () => {
    expect(validateJob(job())).toEqual({
      format: JOB_FORMAT,
      command: 'slice',
      resources: p('resources'),
      workDir: p('work'),
      model: { path: p('model.stl') },
      presets: { machine: p('machine.json'), process: p('process.json'), filaments: [p('filament_1.json')] },
      overrides: {},
      objects: [],
      customGcodes: [],
      arrange: true,
      orient: false,
      output: { gcode: p('out.gcode'), bambu3mf: null, stats: true },
    })
  })

  test('layers and info need no output; slice does', () => {
    const { output: _, ...noOutput } = job()
    expect(validateJob({ ...noOutput, command: 'layers' }).output).toEqual({ gcode: '', bambu3mf: null, stats: false })
    expect(validateJob({ ...noOutput, command: 'info' }).output.gcode).toBe('')
    expect(() => validateJob(noOutput)).toThrow('a slice job needs output.gcode')
    expect(() => validateJob(job({ output: { gcode: '' } }))).toThrow(/output.gcode must be an absolute path/)
    expect(validateJob(job({ command: 'layers', output: { gcode: '' } })).output.gcode).toBe('')
  })

  test('names the field that is wrong', () => {
    const cases: [Record<string, unknown>, RegExp][] = [
      [{ format: 'sketch-orca-job/2' }, /format must be "sketch-orca-job\/1"/],
      [{ command: 'print' }, /command must be slice, layers, info/],
      [{ resources: 'resources' }, /resources must be an absolute path/],
      [{ model: { path: p('model.step') } }, /model.path must end with .3mf or .stl or .obj/],
      [{ model: 'x' }, /model must be an object/],
      [{ presets: { machine: p('m.json'), process: p('p.json'), filaments: [] } }, /at least one filament/],
      [{ presets: { machine: p('m.ini'), process: p('p.json'), filaments: [p('f.json')] } }, /presets.machine must end with .json/],
      [{ presets: { machine: p('m.json'), process: p('p.json'), filaments: ['f.json'] } }, /presets.filaments\[0\] must be an absolute path/],
      [{ output: { gcode: p('out.nc') } }, /output.gcode must end with .gcode/],
      [{ output: { gcode: p('out.gcode'), bambu3mf: p('out.zip') } }, /output.bambu3mf must end with .3mf/],
      [{ output: { gcode: p('out.gcode'), stats: 'yes' } }, /output.stats must be true or false/],
      [{ arrange: 1 }, /arrange must be true or false/],
      [{ objects: {} }, /objects must be a list/],
    ]
    for (const [extra, message] of cases) expect(() => validateJob(job(extra))).toThrow(message)
    expect(() => validateJob(null)).toThrow(JobError)
  })

  test('extensions are matched without regard to case', () => {
    const j = validateJob(job({ model: { path: p('Model.STL') }, output: { gcode: p('OUT.GCODE'), bambu3mf: p('out.gcode.3MF') } }))
    expect(j.output.bambu3mf).toBe(p('out.gcode.3MF'))
  })

  test('objects: indexes, layer heights and height ranges', () => {
    const j = validateJob(
      job({
        objects: [
          { index: 1, layerHeights: { mode: 'adaptive' } },
          { index: 0, overrides: { wall_loops: 3 }, heightRanges: [{ from: 0, to: 5, overrides: { sparse_infill_density: '40%' } }, { from: 5, to: 9 }] },
        ],
      }),
    )
    expect(j.objects).toEqual([
      { index: 1, overrides: {}, layerHeights: { mode: 'adaptive', quality: 0.5 }, heightRanges: [] },
      {
        index: 0,
        overrides: { wall_loops: '3' },
        layerHeights: { mode: 'fixed' },
        heightRanges: [
          { from: 0, to: 5, overrides: { sparse_infill_density: '40%' } },
          { from: 5, to: 9, overrides: {} },
        ],
      },
    ])
    expect(() => validateJob(job({ objects: [{ index: 0 }, { index: 0 }] }))).toThrow(/objects\[1\].index 0 is listed twice/)
    expect(() => validateJob(job({ objects: [{ index: -1 }] }))).toThrow(/whole number from 0/)
    expect(() => validateJob(job({ objects: [{ index: 0.5 }] }))).toThrow(/whole number from 0/)
    expect(() => validateJob(job({ objects: [{ index: 0, heightRanges: [{ from: 0, to: 5 }, { from: 4, to: 8 }] }] }))).toThrow(/starts at 4, inside the range before it/)
    expect(() => validateJob(job({ objects: [{ index: 0, heightRanges: [{ from: 3, to: 3 }] }] }))).toThrow(/0 <= from < to/)
  })

  test('custom G-codes: sorted by print Z, each type with what it needs', () => {
    const j = validateJob(
      job({
        customGcodes: [
          { printZ: 12.4, type: 'pause', note: 'Insert the 10 x 3 mm magnet' },
          { printZ: 3, type: 'color', color: '#D03030' },
          { printZ: 5, type: 'filament', extruder: 2 },
          { printZ: 7, type: 'custom', gcode: 'M117 hi' },
        ],
      }),
    )
    expect(j.customGcodes).toEqual([
      { printZ: 3, type: 'color', color: '#d03030' },
      { printZ: 5, type: 'filament', extruder: 2 },
      { printZ: 7, type: 'custom', gcode: 'M117 hi' },
      { printZ: 12.4, type: 'pause', note: 'Insert the 10 x 3 mm magnet' },
    ])
    const bad: [unknown, RegExp][] = [
      [{ printZ: 0, type: 'pause' }, /printZ must be above 0/],
      [{ printZ: 1, type: 'stop' }, /type must be pause, color, filament, custom/],
      [{ printZ: 1, type: 'filament' }, /needs the extruder/],
      [{ printZ: 1, type: 'filament', extruder: 0 }, /whole number from 1/],
      [{ printZ: 1, type: 'color' }, /needs the color/],
      [{ printZ: 1, type: 'color', color: 'red' }, /must be #rrggbb/],
      [{ printZ: 1, type: 'custom', gcode: '  ' }, /needs its gcode/],
      [{ printZ: 1, type: 'pause', note: 3 }, /note must be text/],
    ]
    for (const [c, message] of bad) expect(() => validateJob(job({ customGcodes: [c] }))).toThrow(message)
    expect(() => validateJob(job({ customGcodes: [{ printZ: 2, type: 'pause' }, { printZ: 2 + 1e-9, type: 'custom', gcode: 'M400' }] }))).toThrow(/two custom G-codes at print Z/)
  })
})

describe('overrides and layer heights', () => {
  test('override values become the text Orca writes', () => {
    expect(normalizeOverrides({ layer_height: 0.2, enable_support: true, ironing: false, brim_type: 'no_brim' }, 'overrides')).toEqual({
      layer_height: '0.2',
      enable_support: '1',
      ironing: '0',
      brim_type: 'no_brim',
    })
    expect(normalizeOverrides(undefined, 'overrides')).toEqual({})
    expect(() => normalizeOverrides({ 'Layer-Height': '1' }, 'overrides')).toThrow(/not an Orca setting key/)
    expect(() => normalizeOverrides({ inherits: 'x' }, 'overrides')).toThrow(/belongs to the preset files/)
    expect(() => normalizeOverrides({ compatible_printers: 'x' }, 'overrides')).toThrow(/belongs to the preset files/)
    expect(() => normalizeOverrides({ nozzle_diameter: [0.4] }, 'overrides')).toThrow(/overrides.nozzle_diameter must be text/)
    expect(() => normalizeOverrides({ layer_height: Number.NaN }, 'overrides')).toThrow(/must be text/)
    expect(() => normalizeOverrides([], 'overrides')).toThrow(/overrides must be an object/)
  })

  test("override keys: Orca's grammar, capital letters too; whether Orca has the key is the engine's to say", () => {
    // required_nozzle_HRC is a filament setting of Orca's (PrintConfig.cpp) with capital letters.
    expect(normalizeOverrides({ required_nozzle_HRC: 5 }, 'overrides')).toEqual({ required_nozzle_HRC: '5' })
    // Layer_Height is not one of Orca's keys, but it has their shape: the engine refuses it (setting-invalid).
    expect(normalizeOverrides({ Layer_Height: '0.2', x9: '1', a_: '1' }, 'overrides')).toEqual({ Layer_Height: '0.2', x9: '1', a_: '1' })
    for (const k of ['', '2layer', '_layer_height', 'layer-height', 'layer height', 'layer.height', `h${String.fromCharCode(0xf6)}he`, ' layer_height', 'layer_height\n'])
      expect(() => normalizeOverrides({ [k]: '1' }, 'overrides')).toThrow(/not an Orca setting key/)
    for (const k of ['Inherits', 'TYPE', 'Name', 'fRoM', 'Compatible_Printers'])
      expect(() => normalizeOverrides({ [k]: 'x' }, 'objects[0].overrides')).toThrow(new RegExp(`objects\\[0\\]\\.overrides: "${k}" belongs to the preset files`))
    // The objects' and height ranges' overrides take the same keys.
    const parsed = validateJob(
      job({
        overrides: { required_nozzle_HRC: '4' },
        objects: [{ index: 0, overrides: { required_nozzle_HRC: '2' }, heightRanges: [{ from: 0, to: 5, overrides: { required_nozzle_HRC: '1' } }] }],
      }),
    )
    expect(parsed.overrides).toEqual({ required_nozzle_HRC: '4' })
    expect(parsed.objects[0]!.overrides).toEqual({ required_nozzle_HRC: '2' })
    expect(parsed.objects[0]!.heightRanges[0]!.overrides).toEqual({ required_nozzle_HRC: '1' })
    expect(() => validateJob(job({ objects: [{ index: 0, heightRanges: [{ from: 0, to: 5, overrides: { 'wall-loops': '1' } }] }] }))).toThrow(
      /objects\[0\]\.heightRanges\[0\]\.overrides: "wall-loops" is not an Orca setting key/,
    )
  })

  test('cli/job.cpp takes the same override keys and refuses the same preset-file keys, in any letter case', () => {
    const cpp = readFileSync(join(import.meta.dir, '..', 'cli', 'job.cpp'), 'utf8')
    expect(cpp.match(/std::regex key_re\("([^"]*)"\)/)?.[1]).toBe(OVERRIDE_KEY_RE.source)
    expect(OVERRIDE_KEY_RE.flags).toBe('')
    const reserved = cpp.match(/std::set<std::string> reserved = \{([^}]*)\}/)?.[1]
    expect(reserved).toBeDefined()
    expect([...reserved!.matchAll(/"([^"]*)"/g)].map((m) => m[1]).sort()).toEqual([...RESERVED_OVERRIDES].sort())
    expect(cpp).toContain('reserved.count(lower(k))')
    for (const k of RESERVED_OVERRIDES) expect(k).toBe(k.toLowerCase())
  })

  test('layer heights: fixed, adaptive (0 finest to 1 coarsest) and a profile', () => {
    expect(normalizeLayerHeights(undefined, 'h')).toEqual({ mode: 'fixed' })
    expect(normalizeLayerHeights({ mode: 'adaptive', quality: 0 }, 'h')).toEqual({ mode: 'adaptive', quality: 0 })
    expect(() => normalizeLayerHeights({ mode: 'adaptive', quality: 1.5 }, 'h')).toThrow(/between 0 \(finest\) and 1 \(coarsest\)/)
    expect(normalizeLayerHeights({ mode: 'profile', profile: [0, 0.2, 10, 0.1] }, 'h')).toEqual({ mode: 'profile', profile: [0, 0.2, 10, 0.1] })
    expect(() => normalizeLayerHeights({ mode: 'profile', profile: [0, 0.2] }, 'h')).toThrow(/pairs of z and height, at least two/)
    expect(() => normalizeLayerHeights({ mode: 'profile', profile: [0, 0.2, 5] }, 'h')).toThrow(/pairs/)
    expect(() => normalizeLayerHeights({ mode: 'profile', profile: [1, 0.2, 5, 0.2] }, 'h')).toThrow(/must start at z 0/)
    expect(() => normalizeLayerHeights({ mode: 'profile', profile: [0, 0.2, 5, 0.2, 4, 0.2] }, 'h')).toThrow(/z 4 comes after z 5/)
    expect(() => normalizeLayerHeights({ mode: 'profile', profile: [0, 0.2, 5, 0] }, 'h')).toThrow(/height at z 5 must be above 0/)
    expect(() => normalizeLayerHeights({ mode: 'profile', profile: 'x' }, 'h')).toThrow(/must be a list/)
    expect(() => normalizeLayerHeights({ mode: 'variable' }, 'h')).toThrow(/mode must be fixed, adaptive or profile/)
  })
})

describe('job files', () => {
  test('writeJob writes the checked job (two-space JSON, LF) and readJob reads it back', () => {
    const path = join(scratch, 'job.json')
    const written = writeJob(path, job({ overrides: { layer_height: 0.2 } }))
    const text = readFileSync(path, 'utf8')
    expect(text).toBe(serializeJob(written))
    expect(text.endsWith('}\n')).toBe(true)
    expect(text).not.toContain('\r')
    expect(text.split('\n')[1]).toBe(`  "format": "${JOB_FORMAT}",`)
    expect(readJob(path)).toEqual(written)
    expect(written.overrides).toEqual({ layer_height: '0.2' })
  })

  test('writeJob refuses an invalid job and writes nothing', () => {
    const path = join(scratch, 'bad-job.json')
    expect(() => writeJob(path, job({ command: 'print' }))).toThrow(JobError)
    expect(() => readFileSync(path)).toThrow()
  })

  test('readJob refuses a file that is not JSON', () => {
    const path = join(scratch, 'not-json.json')
    writeFileSync(path, '{ "format": ')
    expect(() => readJob(path)).toThrow(/not JSON/)
  })
})

const okResult = {
  format: RESULT_FORMAT,
  ok: true,
  code: 'ok',
  message: '',
  warnings: [{ code: 'thin-wall', message: 'a wall is thinner than the nozzle', objectIndex: 0 }],
  layers: [{ objectIndex: 0, ranges: [[0, 0.2], [0.2, 0.4]] }],
  printZ: [0.2, 0.4],
  outputs: { gcode: p('out.gcode'), bambu3mf: '' },
  stats: { timeSeconds: 61.5, layerSeconds: [30, 31.5], filament: [{ extruder: 1, mm: 100, cm3: 0.24, g: 0.3, cost: 0.01 }], features: { 'Outer wall': { seconds: 20, mm: 50 } } },
}

describe('results', () => {
  test('a complete result reads back as written', () => {
    expect(validateResult(okResult)).toEqual(okResult as never)
  })

  test('missing lists and outputs default to empty; stats stay absent', () => {
    expect(validateResult({ format: RESULT_FORMAT, ok: false, code: 'slice-failed' })).toEqual({
      format: RESULT_FORMAT,
      ok: false,
      code: 'slice-failed',
      message: '',
      warnings: [],
      layers: [],
      printZ: [],
      outputs: { gcode: '', bambu3mf: '' },
    })
    expect(validateResult({ ...okResult, outputs: { gcode: null } }).outputs).toEqual({ gcode: '', bambu3mf: '' })
  })

  test('refuses a result that breaks the contract, naming the field', () => {
    const bad: [Record<string, unknown>, RegExp][] = [
      [{ format: 'sketch-orca-result/0' }, /result format must be/],
      [{ ok: 'yes' }, /result ok must be true or false/],
      [{ code: 'Slice Failed', ok: false }, /kebab-case/],
      [{ code: 'slice-failed' }, /result is ok but its code is "slice-failed"/],
      [{ ok: false }, /not ok but its code is "ok"/],
      [{ message: 3 }, /message must be text/],
      [{ warnings: [{ code: 'x' }] }, /warnings\[0\] needs code and message/],
      [{ layers: [{ objectIndex: 0, ranges: [[0.2, 0.2]] }] }, /top 0.2 not above bottom 0.2/],
      [{ layers: [{ objectIndex: 0, ranges: [[0, 0.2], [0, 0.1]] }] }, /ranges\[1\] is not above the layer before it/],
      [{ layers: [{ objectIndex: 0, ranges: [[0, 0.2, 0.4]] }] }, /must be \[bottom, top\]/],
      [{ layers: [{ objectIndex: -1, ranges: [] }] }, /objectIndex must be at least 0/],
      [{ printZ: [0.4, 0.2] }, /printZ\[1\] 0.2 is not above 0.4/],
      [{ printZ: ['0.2'] }, /printZ\[0\] must be a number/],
      [{ outputs: 'x' }, /outputs must be an object/],
      [{ outputs: { gcode: 5 } }, /outputs.gcode must be text/],
      [{ stats: { timeSeconds: -1 } }, /stats.timeSeconds must be at least 0/],
      [{ stats: { timeSeconds: 1, filament: [{ extruder: 0, mm: 0, cm3: 0, g: 0, cost: 0 }] } }, /stats.filament\[0\].extruder must be at least 1/],
      [{ stats: { timeSeconds: 1, features: { Wall: { seconds: 1 } } } }, /stats.features\["Wall"\].mm must be a number/],
    ]
    for (const [extra, message] of bad) expect(() => validateResult({ ...okResult, ...extra })).toThrow(message)
    expect(() => validateResult([])).toThrow(/not a JSON object/)
  })

  test("the engine's own fields read back: exitCode, objectIndex, command, info and engine", () => {
    const engine = { engine: 'sketch-orca', engineVersion: '0.1.0', orcaVersion: '2.4.2', orcaCommit: '8500fcdccaa10b5099ac20d252af3a7c560046f1', orcaTag: 'v2.4.2' }
    const refused = { format: RESULT_FORMAT, ok: false, code: 'arrange-failed', message: 'model.stl does not fit on the plate', objectIndex: 0, exitCode: -21, warnings: [], command: 'slice', engine }
    expect(validateResult(refused)).toEqual({ ...refused, layers: [], printZ: [], outputs: { gcode: '', bambu3mf: '' } } as never)
    expect(validateResult({ ...okResult, exitCode: 0 }).exitCode).toBe(0)
    // As `info` writes it (an excerpt of a real result): the objects typed, the rest kept as written.
    const info = {
      objects: [{ objectIndex: 0, name: 'model.stl', instances: 1, triangles: 12, min: [0.0, 0.0, 0.0], max: [20.0, 20.0, 20.0], size: [20.0, 20.0, 20.0] }],
      bed: { printableArea: [[0, 0], [300, 0], [300, 300], [0, 300]], printableHeight: 275, excludeArea: [[0, 0]], bedType: 'HighTempPlate' },
      printer: 'Voron 2.4 300 0.4 nozzle',
      printerModel: 'Voron 2.4 300',
      process: '0.20mm Standard @Voron',
      filaments: ['Generic PLA @System'],
      extruders: 1,
      printSequence: 'by layer',
    }
    const r = validateResult({ format: RESULT_FORMAT, ok: true, code: 'ok', message: '', exitCode: 0, warnings: [], command: 'info', info, engine })
    expect(r.info).toEqual(info as never)
    expect(r.info!.objects[0]!.size).toEqual([20, 20, 20])
    expect(r.command).toBe('info')
    expect(r.layers).toEqual([])
  })

  test("refuses the engine's own fields when they break the contract", () => {
    const info = { objects: [], bed: {}, printer: 'P', process: 'Q', filaments: ['F'] }
    const bad: [Record<string, unknown>, RegExp][] = [
      [{ exitCode: 1.5 }, /result exitCode must be a whole number/],
      [{ exitCode: '0' }, /result exitCode must be a number/],
      [{ exitCode: -2 }, /result is ok but its exitCode is -2/],
      [{ ok: false, code: 'slice-failed', exitCode: 0 }, /result is not ok but its exitCode is 0/],
      [{ objectIndex: -1 }, /result objectIndex must be at least 0/],
      [{ command: 'cut' }, /result command must be slice, layers, info, got "cut"/],
      [{ info: 'x' }, /result info must be an object/],
      [{ info: { ...info, objects: undefined } }, /result info.objects must be a list/],
      [{ info: { ...info, objects: [{ objectIndex: 0, name: 'a', instances: 1, triangles: 12, min: [0, 0], max: [1, 1, 1], size: [1, 1, 1] }] } }, /info.objects\[0\].min must be \[x, y, z\]/],
      [{ info: { ...info, objects: [{ objectIndex: 0, name: 7, instances: 1, triangles: 12, min: [0, 0, 0], max: [1, 1, 1], size: [1, 1, 1] }] } }, /info.objects\[0\].name must be text/],
      [{ info: { ...info, objects: [{ objectIndex: 0, name: 'a', instances: 1, triangles: 1.5, min: [0, 0, 0], max: [1, 1, 1], size: [1, 1, 1] }] } }, /info.objects\[0\].triangles must be a whole number/],
      [{ info: { ...info, bed: [] } }, /result info.bed must be an object/],
      [{ info: { ...info, printer: 5 } }, /result info.printer must be text/],
      [{ info: { ...info, filaments: 'F' } }, /result info.filaments must be a list/],
      [{ engine: 'sketch-orca' }, /result engine must be an object/],
    ]
    for (const [extra, message] of bad) expect(() => validateResult({ ...okResult, ...extra })).toThrow(message)
  })

  test('parseResult and readResult refuse what is not JSON', () => {
    expect(() => parseResult('{')).toThrow(/result is not JSON/)
    const path = join(scratch, 'result.json')
    writeFileSync(path, JSON.stringify(okResult))
    expect(readResult(path).printZ).toEqual([0.2, 0.4])
  })

  test("the stock command line's exit codes have names", () => {
    expect(ORCA_EXIT_CODES[-100]).toBe('slice-failed')
    expect(ORCA_EXIT_CODES[-5]).toBe('preset-invalid')
    expect(ORCA_EXIT_CODES[-17]).toBe('process-not-compatible')
    expect(ORCA_EXIT_CODES[-52]).toBe('not-printable')
    for (const name of Object.values(ORCA_EXIT_CODES)) expect(name).toMatch(/^[a-z][a-z0-9]*(-[a-z0-9]+)*$/)
  })

  test("each exit code the engine uses has the name of the engine's failure table (cli/common.cpp)", () => {
    const cpp = readFileSync(join(import.meta.dir, '..', 'cli', 'common.cpp'), 'utf8')
    const table = [...cpp.matchAll(/\{\s*"([a-z][a-z0-9-]*)"\s*,\s*(-?\d+)\s*\}/g)].map((m) => [m[1]!, Number(m[2])] as const)
    expect(table.length).toBeGreaterThan(10)
    const first = new Map<number, string>()
    for (const [name, code] of table) if (code !== 0 && !first.has(code)) first.set(code, name)
    for (const [code, name] of first) expect(`${code} ${ORCA_EXIT_CODES[code]}`).toBe(`${code} ${name}`)
  })
})

describe('progress', () => {
  test('one JSON object per line; blank lines and CRLF are fine', () => {
    const text = '{"stage":"slicing","percent":10,"message":"Slicing"}\r\n\r\n{"stage":"gcode","percent":100}\n'
    expect(parseProgress(text)).toEqual([
      { stage: 'slicing', percent: 10, message: 'Slicing' },
      { stage: 'gcode', percent: 100, message: '' },
    ])
    expect(parseProgress('')).toEqual([])
  })

  test('refuses a line that is not a progress line', () => {
    expect(() => parseProgress('{"stage":"x","percent":10}\nnot json')).toThrow(/progress line 2 is not JSON/)
    expect(() => parseProgress('{"stage":"x","percent":101}')).toThrow(/percent from 0 to 100/)
    expect(() => parseProgress('{"percent":5}')).toThrow(/needs stage/)
  })
})
