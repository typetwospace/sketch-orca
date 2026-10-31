import { afterAll, describe, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { scanGcode } from '../scripts/lib/gcode.ts'
import { RESULT_FORMAT, validateJob, type JobResult } from '../scripts/lib/job.ts'
import type { OrcaLock } from '../scripts/lib/lock.ts'
import { writeStamp, workPaths } from '../scripts/lib/paths.ts'
import { cube, meshBounds } from '../scripts/lib/models.ts'
import type { FlatPreset, PresetSet } from '../scripts/lib/profiles.ts'
import {
  CAPITAL_KEY,
  CONFIG_DEF_EXPECT,
  CUBE,
  PAUSE_LAYER_INDEX,
  POCKET,
  REFUSALS,
  REFUSE,
  SMOKE_CASES,
  SMOKE_PRESETS,
  SMOKE_VERSION,
  VALUE_RENAME,
  Z_OFFSETS,
  bedOutline,
  checkAdaptiveLayers,
  checkBambu3mf,
  checkConfigDef,
  checkConfigValue,
  checkExitCode,
  checkExtrusionHeights,
  checkFeatureLengths,
  checkObsoleteKeys,
  legacyKeysFromSource,
  flatSquare,
  loneTriangleObj,
  missingRelationshipTargets,
  toolChanges,
  checkFixedLayers,
  checkGcodeLayers,
  checkInfo,
  checkLayerList,
  checkPause,
  checkPocketCover,
  checkRefusal,
  checkVersion,
  engineEnv,
  engineLibraryDirs,
  exitByte,
  expectedFixedLayers,
  firstCoveringLayer,
  fixedLayerProfile,
  forgetSmokePass,
  layerLimits,
  layersFromProfile,
  parseConfigDef,
  pauseCommands,
  samePath,
  selectedCases,
  smokeJob,
  smokePassedFor,
  smokeTimeoutMs,
} from '../scripts/lib/smoke.ts'
import { writeZip } from './support/zip-writer.ts'

const scratch = mkdtempSync(join(tmpdir(), 'sketch-orca-smoke-'))
afterAll(() => rmSync(scratch, { recursive: true, force: true }))

const LOCK: OrcaLock = {
  format: 'sketch-orca-lock/1',
  repo: 'OrcaSlicer/OrcaSlicer',
  tag: 'v2.4.2',
  commit: '8500fcdccaa10b5099ac20d252af3a7c560046f1',
  version: '2.4.2',
  publishedAt: '2026-07-07T03:33:49Z',
  license: 'AGPL-3.0',
}

/** An ok result for one object whose layers end at `tops`. */
function result(tops: number[], extra: Partial<JobResult> = {}): JobResult {
  const ranges = tops.map((t, i): [number, number] => [i === 0 ? 0 : tops[i - 1]!, t])
  return { format: RESULT_FORMAT, ok: true, code: 'ok', message: '', warnings: [], layers: [{ objectIndex: 0, ranges }], printZ: tops, outputs: { gcode: '', bambu3mf: '' }, ...extra }
}

interface TestLayer {
  z: number
  /** Extruding segments [x0, y0, x1, y1]; each starts with a travel. */
  moves: [number, number, number, number][]
  /** Where the pause tag goes: before the layer's moves (as Orca writes it) or after its first move. */
  pause?: 'before' | 'after-first-move'
  pauseGcode?: string
}

/**
 * Plain-style G-code as Orca writes it for a non-Bambu printer, relative extrusion. The nozzle moves
 * to each layer's Z plus `zOffset` (the tags keep print Z); `config` is written as its config block.
 */
function gcode(layers: TestLayer[], style: 'plain' | 'bbl' = 'plain', zOffset = 0, config?: Record<string, string>): string {
  const out = ['; test G-code', 'G90', 'M83']
  let below = 0
  for (const l of layers) {
    if (style === 'plain') out.push(';LAYER_CHANGE', `;Z:${l.z}`, `;HEIGHT:${+(l.z - below).toFixed(6)}`)
    else out.push('; CHANGE_LAYER', `; Z_HEIGHT: ${l.z}`, `; LAYER_HEIGHT: ${+(l.z - below).toFixed(6)}`)
    out.push(`G1 Z${+(l.z + zOffset).toFixed(3)} F600`)
    const pause = () => out.push(style === 'plain' ? ';PAUSE_PRINT' : '; PAUSE_PRINTING', ...(l.pauseGcode ?? 'PAUSE').split('\n'))
    if (l.pause === 'before') pause()
    l.moves.forEach(([x0, y0, x1, y1], i) => {
      out.push(`G1 X${x0} Y${y0} F12000`, `G1 X${x1} Y${y1} E0.05`)
      if (i === 0 && l.pause === 'after-first-move') pause()
    })
    below = l.z
  }
  if (config) out.push('; CONFIG_BLOCK_START', ...Object.entries(config).map(([k, v]) => `; ${k} = ${v}`), '; CONFIG_BLOCK_END')
  return out.join('\n') + '\n'
}

/** A square perimeter from (a, a) to (b, b). */
function square(a: number, b: number): [number, number, number, number][] {
  return [
    [a, a, b, a],
    [b, a, b, b],
    [b, b, a, b],
    [a, b, a, a],
  ]
}

describe('expected layers (Orca Slicing.cpp, ported)', () => {
  test('the fixed-height profile: the first layer, then the layer height to the top', () => {
    expect(fixedLayerProfile(20, 0.25, 0.2)).toEqual([0, 0.25, 0.25, 0.25, 0.25, 0.2, 20, 0.2])
    expect(fixedLayerProfile(10, 0.2, 0.2)).toEqual([0, 0.2, 10, 0.2])
  })

  test('the 20 mm cube at 0.2 mm with a 0.25 mm first layer has exactly 100 layers, 0.25 to 20.05', () => {
    for (const minLayer of [0.07, 0.08, 0.1]) {
      const z = expectedFixedLayers(CUBE.size, CUBE.firstLayer, CUBE.layerHeight, minLayer)
      expect(z).toHaveLength(CUBE.layers)
      expect(z[0]).toBe(CUBE.firstLayer)
      expect(z[z.length - 1]!).toBeCloseTo(CUBE.lastTop, 9)
      for (let i = 1; i < z.length; i++) expect(z[i]! - z[i - 1]!).toBeCloseTo(0.2, 9)
    }
    expect(expectedFixedLayers(CUBE.size, CUBE.firstLayer, CUBE.layerHeight, 0.07)[PAUSE_LAYER_INDEX]!).toBeCloseTo(5.05, 9)
  })

  test('the pocket box: 50 layers, and the first one covering the pocket (z 3 to 6) ends at 6.2', () => {
    const z = expectedFixedLayers(POCKET.size[2], POCKET.firstLayer, POCKET.layerHeight, 0.07)
    expect(z).toHaveLength(50)
    const ranges = z.map((t, i): [number, number] => [i === 0 ? 0 : z[i - 1]!, t])
    const cover = firstCoveringLayer(ranges, POCKET.max[2])
    expect(cover).toBe(POCKET.coverIndex)
    expect(ranges[cover!]![1]).toBeCloseTo(POCKET.coverZ, 9)
    // The layer below is sliced at 5.9, inside the pocket.
    expect((ranges[cover! - 1]![0] + ranges[cover! - 1]![1]) / 2).toBeLessThan(POCKET.max[2])
    expect(firstCoveringLayer(ranges, 20)).toBeUndefined()
  })

  test('a varying profile is followed between its points', () => {
    const z = layersFromProfile(10, 0.2, 0.05, [0, 0.2, 10, 0.1])
    const h = z.slice(1).map((t, i) => t - z[i]!)
    for (let i = 1; i < h.length; i++) expect(h[i]!).toBeLessThan(h[i - 1]! + 1e-12)
    expect(Math.max(...h)).toBeLessThanOrEqual(0.2)
    expect(Math.min(...h)).toBeGreaterThanOrEqual(0.1)
    expect(z[z.length - 1]!).toBeGreaterThan(9.9)
  })

  test("the printer's layer limits, as Orca applies them", () => {
    expect(layerLimits({ min_layer_height: ['0.08'], max_layer_height: ['0.32'], nozzle_diameter: ['0.4'] }, 0.2)).toEqual({ min: 0.08, max: 0.32 })
    expect(layerLimits({ min_layer_height: ['0'], max_layer_height: ['0'], nozzle_diameter: ['0.4'] }, 0.2)).toEqual({ min: 0.07, max: 0.30000000000000004 })
    expect(layerLimits({ min_layer_height: ['0.005'], max_layer_height: ['0.3'], nozzle_diameter: ['0.4'] }, 0.2).min).toBe(0.01)
    expect(layerLimits({ min_layer_height: ['0.08'], max_layer_height: ['0.32'] }, 0.4)).toEqual({ min: 0.08, max: 0.4 })
    expect(layerLimits({ min_layer_height: ['0.08'], max_layer_height: ['0.32'] }, 0.05)).toEqual({ min: 0.05, max: 0.32 })
  })
})

describe('checking results', () => {
  const cube = expectedFixedLayers(CUBE.size, CUBE.firstLayer, CUBE.layerHeight, 0.07)

  test('the expected layers pass; a missing or moved layer does not', () => {
    expect(checkFixedLayers(result(cube), cube)).toEqual([])
    expect(checkFixedLayers(result(cube.slice(0, -1)), cube)).toEqual(['expected 100 layers, got 99'])
    const moved = [...cube]
    moved[10] = moved[10]! + 0.01
    expect(checkFixedLayers(result(moved), cube)).toEqual([`layer 11 has print Z ${moved[10]}, expected ${cube[10]}`])
    const rounded = cube.map((z) => Number(z.toFixed(5)))
    expect(checkFixedLayers(result(rounded), cube)).toEqual([])
  })

  test('the layer list must be one object, starting at 0, layers touching, tops equal to print Z', () => {
    expect(checkLayerList(result([0.2, 0.4]))).toEqual([])
    expect(checkLayerList({ ...result([0.2]), ok: false, code: 'slice-failed', message: 'empty' })).toEqual(['the engine failed: slice-failed: empty'])
    expect(checkLayerList({ ...result([0.2]), layers: [] })).toEqual(['expected the layers of 1 object, got 0'])
    expect(checkLayerList({ ...result([0.2, 0.4]), layers: [{ objectIndex: 0, ranges: [[0, 0.2], [0.25, 0.4]] }] })).toEqual(['layer 2 starts at 0.25, but the layer below ends at 0.2'])
    expect(checkLayerList({ ...result([0.2, 0.4]), layers: [{ objectIndex: 0, ranges: [[0.1, 0.2], [0.2, 0.4]] }] })).toEqual(['the first layer starts at 0.1, not 0'])
    expect(checkLayerList({ ...result([0.2, 0.4]), printZ: [0.2, 0.45] })).toEqual(['layer 2 ends at 0.4 but its print Z is 0.45'])
    expect(checkLayerList({ ...result([0.2, 0.4]), printZ: [0.2] })).toEqual(['2 layer ranges but 1 print Z values'])
  })

  test('adaptive layers must vary within the limits, from the set first layer to the top', () => {
    const tops = [0.2]
    let h = 0.08
    while (tops[tops.length - 1]! < 20) {
      tops.push(+(tops[tops.length - 1]! + h).toFixed(6))
      h = Math.min(0.32, h + 0.02)
    }
    const o = { firstLayer: 0.2, minLayer: 0.08, maxLayer: 0.32, objectHeight: 20 }
    expect(checkAdaptiveLayers(result(tops), o)).toEqual([])
    const even = Array.from({ length: 100 }, (_, i) => +(0.2 * (i + 1)).toFixed(6))
    expect(checkAdaptiveLayers(result(even), o)).toEqual(['the heights do not vary: 1 distinct, spread 0.0000 mm'])
    const tooThick = [...tops.slice(0, 5), +(tops[4]! + 0.5).toFixed(6)]
    expect(checkAdaptiveLayers(result(tooThick), { ...o, objectHeight: tooThick[tooThick.length - 1]! })).toContain(`layer 6 is 0.5000 mm, outside the printer's 0.08..0.32`)
    expect(checkAdaptiveLayers(result(tops), { ...o, firstLayer: 0.25 })[0]).toMatch(/the first layer is 0.2 mm, expected 0.25/)
    expect(checkAdaptiveLayers(result(tops.slice(0, 40)), o).pop()).toMatch(/not near the top 20/)
    expect(checkAdaptiveLayers(result([0.2, 0.3]), o)).toEqual(['only 2 layers'])
  })

  test("the G-code's layer changes match the result", () => {
    const scan = scanGcode(gcode([{ z: 0.2, moves: [] }, { z: 0.4, moves: [] }]))
    expect(checkGcodeLayers(scan, [0.2, 0.4])).toEqual([])
    expect(checkGcodeLayers(scan, [0.2, 0.4, 0.6])).toEqual(['the G-code has 2 layer changes, the result 3 layers'])
    expect(checkGcodeLayers(scan, [0.2, 0.45])).toEqual(['G-code layer 2 is at Z 0.4, the result says 0.45'])
  })

  test("every extrusion is at its layer's print Z plus the printer's z_offset, the last in the top layer", () => {
    const layers = (n: number) => Array.from({ length: n }, (_, i) => ({ z: +(0.25 + 0.2 * i).toFixed(6), moves: square(10, 20) }))
    for (const offset of Z_OFFSETS) {
      const scan = scanGcode(gcode(layers(3), 'plain', offset))
      expect(checkExtrusionHeights(scan, [0.25, 0.45, 0.65], offset)).toEqual([])
      // The layer tags keep print Z.
      expect(checkGcodeLayers(scan, [0.25, 0.45, 0.65])).toEqual([])
      // Taking z_offset off again (or counting it twice) puts every extrusion off its layer.
      const wrong = checkExtrusionHeights(scan, [0.25, 0.45, 0.65], 0)
      expect(wrong[0]).toBe(`line 9 extrudes at Z ${+(0.25 + offset).toFixed(3)}, but its layer (1) is at print Z 0.25, plus z_offset 0`)
      expect(wrong).toHaveLength(4)
      expect(wrong[3]).toBe('9 more extrusions are off their layer\'s Z')
    }
    // Without an offset, a G-code that moves the nozzle 0.3 mm up is off.
    expect(checkExtrusionHeights(scanGcode(gcode(layers(2), 'plain', 0.3)), [0.25, 0.45], 0)).toHaveLength(4)
    // Z as Orca writes it, to 3 decimals, is close enough.
    expect(checkExtrusionHeights(scanGcode(gcode([{ z: 0.2501, moves: square(10, 20) }], 'plain', 0.3)), [0.2501], 0.3)).toEqual([])
    // The top layer prints nothing: the G-code stops below the top of the print.
    const short = [...layers(2), { z: 0.65, moves: [] }]
    expect(checkExtrusionHeights(scanGcode(gcode(short, 'plain', 0.3)), [0.25, 0.45, 0.65], 0.3)).toEqual(['the last extrusion is in layer 2, not in the top layer (3)'])
    // More layers in the G-code than in the result.
    expect(checkExtrusionHeights(scanGcode(gcode(layers(3), 'plain', 0.3)), [0.25, 0.45], 0.3)).toEqual(['line 33 extrudes in layer 3, but the result has 2 layers'])
    expect(checkExtrusionHeights(scanGcode(gcode([{ z: 0.25, moves: [] }])), [0.25], 0)).toEqual(['the G-code extrudes nothing after its first layer change'])
  })

  test("the z offsets reach past the engine's top-of-print slack: z_offset counted twice refuses the raised one, z_offset taken off the lowered one", () => {
    // cli/run.cpp check_gcode_reaches_top: the last extrusion, at print Z as Orca's G-code processor
    // stores it, must reach the last print Z less half the last layer and 0.01 mm.
    const cpp = readFileSync(join(import.meta.dir, '..', 'cli', 'run.cpp'), 'utf8')
    expect(cpp).toMatch(/want\s*=\s*zs\.back\(\)\s*-\s*0\.5\s*\*\s*last_height\s*-\s*0\.01\s*;/)
    const slack = 0.5 * CUBE.layerHeight + 0.01
    // The cube's last extrusion is at its last print Z whatever the offset; `term` is what a check
    // adds to that print Z for the offset: nothing (the fix), the offset (the old double count), or
    // the offset taken off (a sign slip).
    const refused = (term: (offset: number) => number) => Z_OFFSETS.filter((o) => CUBE.lastTop < CUBE.lastTop + term(o) - slack)
    expect(refused(() => 0)).toEqual([])
    expect(refused((o) => o)).toEqual(Z_OFFSETS.filter((o) => o > 0))
    expect(refused((o) => -o)).toEqual(Z_OFFSETS.filter((o) => o < 0))
  })

  test("the G-code's config block gives a setting's value, as text or as a number", () => {
    const scan = scanGcode(gcode([{ z: 0.2, moves: [] }], 'plain', 0, { z_offset: '-0.1', required_nozzle_HRC: '5', z_offset_extra: '9' }))
    expect(checkConfigValue(scan, 'z_offset', -0.1)).toEqual([])
    expect(checkConfigValue(scan, 'z_offset', 0.3)).toEqual(['the G-code\'s config block gives z_offset = "-0.1", expected "0.3"'])
    expect(checkConfigValue(scan, CAPITAL_KEY.key, CAPITAL_KEY.value)).toEqual([])
    expect(checkConfigValue(scan, CAPITAL_KEY.key, '3')).toEqual(['the G-code\'s config block gives required_nozzle_HRC = "5", expected "3"'])
    expect(checkConfigValue(scan, 'layer_height', 0.2)).toEqual(["the G-code's config block has no layer_height"])
    const blank = scanGcode(gcode([{ z: 0.2, moves: [] }], 'plain', 0, { z_offset: '' }))
    expect(checkConfigValue(blank, 'z_offset', 0)).toEqual(['the G-code\'s config block gives z_offset = "", expected "0"'])
  })
})

describe('pauses', () => {
  const layers = (pauseAt: number, how: 'before' | 'after-first-move' = 'before', pauseGcode?: string): TestLayer[] =>
    [0.25, 0.45, 0.65, 0.85].map((z) => ({ z, moves: square(90, 110), ...(Math.abs(z - pauseAt) < 1e-9 ? { pause: how, pauseGcode } : {}) }))

  test("the machine's pause G-code as commands", () => {
    expect(pauseCommands('M400 U1')).toEqual(['M400 U1'])
    expect(pauseCommands('; pause\nM601 ; park\n\n  M117 Paused  \n')).toEqual(['M601', 'M117 Paused'])
    expect(pauseCommands('')).toEqual([])
  })

  test('one pause, in its layer, before the first extrusion, running the machine pause G-code', () => {
    expect(checkPause(scanGcode(gcode(layers(0.65))), 0.65, 'PAUSE')).toEqual([])
    expect(checkPause(scanGcode(gcode(layers(0.65, 'before', 'M601\nM117 Paused'))), 0.65, 'M601\nM117 Paused')).toEqual([])
  })

  test('a pause in another layer, after an extrusion, twice or missing is reported', () => {
    expect(checkPause(scanGcode(gcode(layers(0.45))), 0.65, 'PAUSE')).toEqual(['the pause is in the layer at Z 0.45, expected 0.65'])
    expect(checkPause(scanGcode(gcode(layers(0.65, 'after-first-move'))), 0.65, 'PAUSE')[0]).toMatch(/comes after the layer's first extrusion/)
    const twice = layers(0.65).map((l) => (l.z === 0.25 ? { ...l, pause: 'before' as const } : l))
    expect(checkPause(scanGcode(gcode(twice)), 0.65, 'PAUSE')).toEqual(['expected 1 pause in the G-code, found 2'])
    expect(checkPause(scanGcode(gcode(layers(99))), 0.65, 'PAUSE')).toEqual(['expected 1 pause in the G-code, found 0'])
    expect(checkPause(scanGcode(';PAUSE_PRINT\nPAUSE\n' + gcode(layers(99))), 0.65, 'PAUSE')).toEqual(['the pause comes before the first layer'])
  })

  test('the commands after the pause tag must be the machine pause G-code', () => {
    expect(checkPause(scanGcode(gcode(layers(0.65, 'before', 'M600'))), 0.65, 'PAUSE')).toEqual([`after the pause tag: ["M600"], expected the machine's pause G-code ["PAUSE"]`])
    expect(checkPause(scanGcode(gcode(layers(0.65))), 0.65, '')).toEqual(['the machine preset has no pause G-code'])
    // A pause G-code with placeholders cannot be compared as text; the rest still is.
    expect(checkPause(scanGcode(gcode(layers(0.65, 'before', 'M117 layer 3'))), 0.65, 'M117 layer {layer_num}')).toEqual([])
  })

  test('Bambu tags', () => {
    expect(checkPause(scanGcode(gcode(layers(0.45, 'before', 'M400 U1'), 'bbl')), 0.45, 'M400 U1')).toEqual([])
  })

  test('the pocket is open below the pause and bridged after it', () => {
    // The box spans 90..110 on the bed; the pocket 95..105 (half 5); walls stay outside it.
    const wall: [number, number, number, number][] = [...square(90, 110), ...square(91, 109), [92, 92, 92, 108], [108, 92, 108, 108]]
    const bridge: [number, number, number, number][] = [...square(90, 110), [90.5, 96, 109.5, 96], [90.5, 100, 109.5, 100], [90.5, 104, 109.5, 104]]
    const ok = gcode([
      { z: 5.8, moves: wall },
      { z: 6.0, moves: wall },
      { z: 6.2, moves: bridge, pause: 'before' },
      { z: 6.4, moves: bridge },
    ])
    expect(checkPocketCover(scanGcode(ok), [5, 5], 0.6)).toEqual([])
    const closedEarly = gcode([
      { z: 6.0, moves: bridge },
      { z: 6.2, moves: bridge, pause: 'before' },
    ])
    expect(checkPocketCover(scanGcode(closedEarly), [5, 5], 0.6)).toEqual(['the layer at Z 6, below the pause, already extrudes over the pocket'])
    const notCovered = gcode([
      { z: 6.0, moves: wall },
      { z: 6.2, moves: wall, pause: 'before' },
    ])
    expect(checkPocketCover(scanGcode(notCovered), [5, 5], 0.6)).toEqual(['the layer at Z 6.2, after the pause, does not cover the pocket'])
    expect(checkPocketCover(scanGcode(gcode([{ z: 0.2, moves: wall, pause: 'before' }])), [5, 5], 0.6)).toEqual(['the pause is not after a first layer'])
  })
})

describe('engine output', () => {
  const version = JSON.stringify({ engine: 'sketch-orca', engineVersion: '0.1.0', orcaVersion: '2.4.2', orcaCommit: LOCK.commit })

  test('version names the locked release', () => {
    expect(checkVersion(`${version}\n`, LOCK)).toEqual([])
    expect(checkVersion(`loading resources\n${version}\n`, LOCK)).toEqual([])
    expect(checkVersion(version.replace('2.4.2', '2.4.1'), LOCK)).toEqual(['orcaVersion is "2.4.1", the lock says 2.4.2'])
    expect(checkVersion(version.replace(LOCK.commit, 'abc'), LOCK)).toEqual([`orcaCommit is "abc", the lock says ${LOCK.commit}`])
    expect(checkVersion(JSON.stringify({ engine: 'orca', orcaVersion: '2.4.2', orcaCommit: LOCK.commit }), LOCK)).toEqual(['engine is "orca", expected "sketch-orca"', 'engineVersion is missing'])
    expect(checkVersion('OrcaSlicer 2.4.2', LOCK)[0]).toMatch(/did not print JSON/)
    expect(checkVersion('42', LOCK)).toEqual(['version did not print a JSON object'])
  })

  /** config-def output holding every expected option, in Orca's type names, plus `filler` more. */
  function defs(filler = 300, change: (key: string, d: Record<string, unknown>) => Record<string, unknown> | undefined = (_k, d) => d) {
    const typeName: Record<string, string> = { float: 'coFloat', floats: 'coFloats', int: 'coInt', percent: 'coPercent', bool: 'coBool', enum: 'coEnum', string: 'coString' }
    const options: Record<string, unknown>[] = []
    for (const [key, type, values] of CONFIG_DEF_EXPECT) {
      const d = change(key, { key, type: typeName[type], label: key, ...(type === 'enum' ? { enumValues: [...values, 'other'] } : {}) })
      if (d) options.push(d)
    }
    for (let i = 0; i < filler; i++) options.push({ key: `option_${i}`, type: 'coFloat' })
    return { format: 'sketch-orca-config-def/1', options }
  }

  test('config-def: every expected option with its type and enum values', () => {
    expect(checkConfigDef(parseConfigDef(defs()))).toEqual([])
    const parsed = parseConfigDef(defs())
    expect(parsed.get('sparse_infill_density')).toEqual({ key: 'sparse_infill_density', type: 'percent', enumValues: [] })
    expect(parsed.get('seam_position')?.enumValues).toEqual(['nearest', 'aligned', 'back', 'random', 'other'])
  })

  test('config-def: a map of options and other spellings of the enum values are read too', () => {
    const map = {
      settings: {
        layer_height: { type: 'float' },
        support_type: { type: 'coEnum', enum: { values: ['normal(auto)', 'tree(auto)'], labels: ['Normal', 'Tree'] } },
        brim_type: { type: 'enum', enum_values: ['no_brim', 'outer_only'] },
        seam_position: { type: 'coEnum', enum: [{ value: 'nearest', label: 'Nearest' }, { value: 'back', label: 'Back' }] },
        wall_generator: { type: 'coEnum', enumValues: [{ label: 'no value' }] },
      },
    }
    const parsed = parseConfigDef(map)
    expect(parsed.get('layer_height')?.type).toBe('float')
    expect(parsed.get('support_type')?.enumValues).toEqual(['normal(auto)', 'tree(auto)'])
    expect(parsed.get('brim_type')?.enumValues).toEqual(['no_brim', 'outer_only'])
    expect(parsed.get('seam_position')?.enumValues).toEqual(['nearest', 'back'])
    expect(parsed.get('wall_generator')?.enumValues).toEqual([])
    expect(() => parseConfigDef(null)).toThrow(/not a JSON object/)
    expect(() => parseConfigDef({ options: [] })).toThrow(/lists no options/)
  })

  test('config-def: a missing option, a wrong type, missing enum values and a short list are reported', () => {
    expect(checkConfigDef(parseConfigDef(defs(300, (k, d) => (k === 'wall_loops' ? undefined : d))))).toEqual(['config-def has no wall_loops'])
    expect(checkConfigDef(parseConfigDef(defs(300, (k, d) => (k === 'enable_support' ? { ...d, type: 'coInt' } : d))))).toEqual(['enable_support has type "int", expected "bool"'])
    expect(checkConfigDef(parseConfigDef(defs(300, (k, d) => (k === 'sparse_infill_pattern' ? { ...d, enumValues: ['grid'] } : d))))).toEqual(['sparse_infill_pattern lacks the enum values gyroid, honeycomb (has 1)'])
    expect(checkConfigDef(parseConfigDef(defs(10)))).toEqual([`config-def lists only ${CONFIG_DEF_EXPECT.length + 10} options; Orca has far more`])
  })

  // The shape of Orca's handle_legacy (PrintConfig.cpp), cut down: renames, a value fix on a key it
  // keeps, a key it drops unless the value says otherwise, and its set of ignored keys.
  const PRINT_CONFIG_CPP = [
    'void PrintConfigDef::init_fff_params() {}',
    'void PrintConfigDef::handle_legacy(t_config_option_key &opt_key, std::string &value)',
    '{',
    '    if (opt_key == "wiping_volume") {',
    '        opt_key = "prime_volume";',
    '    } else if (opt_key == "wall_direction" && value == "auto") {',
    '        value = "ccw";',
    '    } else if (opt_key ==',
    '               "prime_tower_rib_wall") {',
    '        if (value == "1") opt_key = "wipe_tower_wall_type"; else opt_key = "";',
    '    } else if (opt_key == "wiping_volume") {',
    '    }',
    '    static std::set<std::string> ignore = {',
    '        "acceleration", "silent_mode",',
    '        "max_print_speed",',
    '    };',
    '    if (ignore.find(opt_key) != ignore.end()) { opt_key = ""; return; }',
    '    std::set<std::string> other = { "not_legacy" };',
    '}',
    'void PrintConfigDef::handle_legacy_composite(DynamicPrintConfig &config) { if (opt_key == "after") {} }',
  ].join('\r\n')

  test("legacyKeysFromSource: the keys handle_legacy compares with and the ones it ignores, as the engine's build reads them", () => {
    expect(legacyKeysFromSource(PRINT_CONFIG_CPP)).toEqual({
      compared: ['wiping_volume', 'wall_direction', 'prime_tower_rib_wall'],
      ignored: ['acceleration', 'silent_mode', 'max_print_speed'],
    })
    expect(() => legacyKeysFromSource('void PrintConfigDef::handle_legacy_composite() {}')).toThrow(/no PrintConfigDef::handle_legacy/)
    expect(() => legacyKeysFromSource(PRINT_CONFIG_CPP.replace('ignore = {', 'ignored_keys = {'))).toThrow(/no set of ignored keys/)
    expect(() => legacyKeysFromSource(PRINT_CONFIG_CPP.replace(/\};/g, '}'))).toThrow(/does not end with \};/)
    expect(() => legacyKeysFromSource(PRINT_CONFIG_CPP.replace(/opt_key ==/g, 'opt_key !='))).toThrow(/no legacy keys found in handle_legacy \(0 compared, 3 ignored\)/)
  })

  test("config-def's obsolete keys: sorted, none an option, renamed to options, and handle_legacy's own", () => {
    const options = parseConfigDef({ options: ['prime_volume', 'wall_direction', 'wipe_tower_wall_type', 'layer_height'].map((key) => ({ key, type: 'coFloat' })) })
    const legacy = legacyKeysFromSource(PRINT_CONFIG_CPP)
    const obsolete = ['acceleration', 'max_print_speed', 'prime_tower_rib_wall', 'silent_mode', 'wiping_volume']
    const renamed = { prime_tower_rib_wall: 'wipe_tower_wall_type', wiping_volume: 'prime_volume' }
    // Orca still defines silent_mode, and handle_legacy ignores it.
    const stillDefined = ['silent_mode']
    expect(checkObsoleteKeys({ obsolete, renamed, stillDefined }, options, legacy)).toEqual([])
    expect(checkObsoleteKeys({ obsolete, stillDefined }, options)).toEqual([])
    expect(checkObsoleteKeys({ obsolete, stillDefined: [] }, options, legacy)).toEqual([])
    expect(checkObsoleteKeys({}, options)).toEqual(['config-def has no list of obsolete keys'])
    expect(checkObsoleteKeys({ obsolete: [] }, options)).toEqual(['config-def lists no obsolete keys'])
    expect(checkObsoleteKeys({ obsolete: ['acceleration', 3] }, options)).toEqual(["config-def's obsolete keys are not all keys"])
    expect(checkObsoleteKeys({ obsolete: ['silent_mode', 'acceleration'], stillDefined }, options)).toEqual(["config-def's obsolete keys are not sorted, each once: silent_mode comes before acceleration"])
    expect(checkObsoleteKeys({ obsolete: ['acceleration', 'acceleration'], stillDefined: [] }, options)).toEqual(["config-def's obsolete keys are not sorted, each once: acceleration comes before acceleration"])
    expect(checkObsoleteKeys({ obsolete: ['acceleration', 'layer_height'], stillDefined: [] }, options)).toEqual(['config-def lists obsolete keys as options too: layer_height'])
    expect(checkObsoleteKeys({ obsolete, renamed: ['prime_volume'], stillDefined }, options)).toEqual(["config-def's renamed is not an object"])
    expect(checkObsoleteKeys({ obsolete, renamed: { wall_direction: 'prime_volume', wiping_volume: 'no_such_option' }, stillDefined }, options)).toEqual([
      'config-def renames wall_direction, which it does not list as obsolete',
      'config-def renames wiping_volume to "no_such_option", which is not an option',
    ])
    // Against Orca's source: an ignored key missing, a renamed key missing, a key handle_legacy never names.
    expect(checkObsoleteKeys({ obsolete: obsolete.filter((k) => k !== 'silent_mode'), stillDefined: [] }, options, legacy)).toEqual(['handle_legacy ignores keys config-def does not list as obsolete: silent_mode'])
    expect(checkObsoleteKeys({ obsolete: obsolete.filter((k) => k !== 'wiping_volume'), stillDefined }, options, legacy)).toEqual([
      'handle_legacy reads keys that config-def lists neither as options nor as obsolete: wiping_volume',
    ])
    expect(checkObsoleteKeys({ obsolete: [...obsolete, 'z_lift'].sort(), stillDefined }, options, legacy)).toEqual(['config-def lists obsolete keys handle_legacy does not name: z_lift'])
  })

  test("config-def's stillDefined: the obsolete keys Orca still defines, each one handle_legacy ignores, so a live setting misread as obsolete is caught", () => {
    const legacy = legacyKeysFromSource(PRINT_CONFIG_CPP)
    const obsolete = ['acceleration', 'max_print_speed', 'prime_tower_rib_wall', 'silent_mode', 'wiping_volume']
    const renamed = { prime_tower_rib_wall: 'wipe_tower_wall_type', wiping_volume: 'prime_volume' }
    // wall_direction is a live setting whose value handle_legacy fixes ("auto" becomes "ccw"). Were
    // every probe value of config-def to drop or rename it, config-def would leave it out of the
    // options and list it as obsolete: every other check passes that (the options lack it, it is
    // obsolete, handle_legacy names it). Orca still defines it, though, and does not ignore it.
    const misread = parseConfigDef({ options: ['prime_volume', 'wipe_tower_wall_type', 'layer_height'].map((key) => ({ key, type: 'coFloat' })) })
    const withLive = [...obsolete, 'wall_direction'].sort()
    expect(checkObsoleteKeys({ obsolete: withLive, renamed, stillDefined: ['silent_mode', 'wall_direction'] }, misread, legacy)).toEqual([
      'config-def lists as obsolete keys Orca still defines that handle_legacy does not ignore (live settings its probe values misread, most likely): wall_direction',
    ])
    // Without the list, config-def cannot be checked for that.
    expect(checkObsoleteKeys({ obsolete: withLive, renamed }, misread, legacy)).toEqual(['config-def does not say which obsolete keys Orca still defines (stillDefined)'])
    expect(checkObsoleteKeys({ obsolete, renamed, stillDefined: 'silent_mode' }, misread)).toEqual(["config-def's stillDefined is not a list of keys"])
    expect(checkObsoleteKeys({ obsolete, renamed, stillDefined: ['silent_mode', ''] }, misread)).toEqual(["config-def's stillDefined is not a list of keys"])
    expect(checkObsoleteKeys({ obsolete, renamed, stillDefined: ['silent_mode', 'acceleration'] }, misread)).toEqual([
      "config-def's stillDefined keys are not sorted, each once: silent_mode comes before acceleration",
    ])
    expect(checkObsoleteKeys({ obsolete, renamed, stillDefined: ['layer_height', 'silent_mode'] }, misread)).toEqual(['config-def says Orca still defines layer_height, which it does not list as obsolete'])
    // Without Orca's source only the list's shape is checked: handle_legacy's ignored keys are not at hand.
    expect(checkObsoleteKeys({ obsolete: withLive, renamed, stillDefined: ['silent_mode', 'wall_direction'] }, misread)).toEqual([])
  })

  test('a Bambu .gcode.3mf: plate_1.gcode with Bambu tags for every layer, and its md5', () => {
    const text = gcode([{ z: 0.2, moves: square(100, 120) }, { z: 0.4, moves: square(100, 120) }], 'bbl')
    const md5 = createHash('md5').update(text).digest('hex')
    const zip = (entries: { name: string; data: string }[]) => writeZip([{ name: '[Content_Types].xml', data: '<Types/>' }, ...entries])
    expect(checkBambu3mf(zip([{ name: 'Metadata/plate_1.gcode', data: text }, { name: 'Metadata/plate_1.gcode.md5', data: md5.toUpperCase() }]), [0.2, 0.4])).toEqual([])
    expect(checkBambu3mf(zip([{ name: 'Metadata/plate_1.gcode', data: text }]), [0.2, 0.4])).toEqual([])
    expect(checkBambu3mf(zip([{ name: 'Metadata/plate_1.gcode', data: text }, { name: 'Metadata/plate_1.gcode.md5', data: '0'.repeat(32) }]), [0.2, 0.4])).toEqual([
      `Metadata/plate_1.gcode.md5 says ${'0'.repeat(32)}, the G-code hashes to ${md5}`,
    ])
    expect(checkBambu3mf(zip([{ name: 'Metadata/plate_2.gcode', data: text }]), [0.2, 0.4])).toEqual(['no Metadata/plate_1.gcode in the archive (it holds [Content_Types].xml, Metadata/plate_2.gcode)'])
    const plain = gcode([{ z: 0.2, moves: [] }, { z: 0.4, moves: [] }])
    expect(checkBambu3mf(zip([{ name: 'Metadata/plate_1.gcode', data: plain }]), [0.2, 0.4])).toEqual(['plate_1.gcode does not use Bambu layer tags (style plain)'])
    expect(checkBambu3mf(zip([{ name: 'Metadata/plate_1.gcode', data: text }]), [0.2, 0.4, 0.6])).toEqual(['the G-code has 2 layer changes, the result 3 layers'])
  })
})

describe('jobs and the engine environment', () => {
  const dir = resolve(scratch, 'case')
  const presets = { machine: join(dir, 'machine.json'), process: join(dir, 'process.json'), filaments: [join(dir, 'filament_1.json')] }

  test('smokeJob writes into the case folder', () => {
    const j = smokeJob({ command: 'slice', dir, resources: resolve(scratch, 'resources'), presets, overrides: { layer_height: '0.2' } })
    expect(j.model.path).toBe(join(dir, 'model.stl'))
    expect(j.workDir).toBe(join(dir, 'work'))
    expect(j.output).toEqual({ gcode: join(dir, 'out.gcode'), bambu3mf: null, stats: true })
    expect(j.overrides).toEqual({ layer_height: '0.2' })
    expect(j.arrange).toBe(true)
    const b = smokeJob({ command: 'slice', dir, resources: resolve(scratch, 'resources'), presets, bambu3mf: true, customGcodes: [{ printZ: 5.05, type: 'pause' }] })
    expect(b.output.bambu3mf).toBe(join(dir, 'out.gcode.3mf'))
    expect(b.customGcodes).toEqual([{ printZ: 5.05, type: 'pause' }])
    const l = smokeJob({ command: 'layers', dir, resources: resolve(scratch, 'resources'), presets, objects: [{ index: 0, overrides: {}, layerHeights: { mode: 'adaptive', quality: 0.5 }, heightRanges: [] }] })
    expect(l.command).toBe('layers')
    expect(l.objects[0]!.layerHeights).toEqual({ mode: 'adaptive', quality: 0.5 })
  })

  test("the engine's library folders come first on its library path", () => {
    const base = { Path: 'C:\\Windows\\System32', OTHER: 'x' }
    expect(engineEnv(base, ['C:\\e', 'C:\\d'], 'win32')).toEqual({ Path: 'C:\\e;C:\\d;C:\\Windows\\System32', OTHER: 'x' })
    expect(base.Path).toBe('C:\\Windows\\System32')
    expect(engineEnv({ PATH: '/usr/bin' }, ['/e/lib'], 'linux')).toEqual({ PATH: '/usr/bin', LD_LIBRARY_PATH: '/e/lib' })
    expect(engineEnv({ DYLD_LIBRARY_PATH: '/x' }, ['/e/lib'], 'darwin')).toEqual({ DYLD_LIBRARY_PATH: '/e/lib:/x' })
    expect(engineEnv({}, ['C:\\e'], 'win32')).toEqual({ Path: 'C:\\e' })
  })

  test('library folders before staging: the build, the dependency prefix, GMP and MPFR', () => {
    const w = workPaths(resolve(scratch, 'home'))
    const exe = join(w.build, 'sketch-orca', 'sketch-orca.exe')
    expect(engineLibraryDirs(w, exe, 'win32', 'x64')).toEqual([
      dirname(exe),
      join(w.depsPrefix, 'bin'),
      join(w.depsPrefix, 'bin', 'occt'),
      join(w.src, 'deps', 'GMP', 'gmp', 'lib', 'win-x64'),
      join(w.src, 'deps', 'MPFR', 'mpfr', 'lib', 'win-x64'),
    ])
    expect(engineLibraryDirs(w, exe, 'win32', 'arm64')[3]).toBe(join(w.src, 'deps', 'GMP', 'gmp', 'lib', 'win-arm64'))
    expect(engineLibraryDirs(w, exe, 'linux')).toEqual([dirname(exe), join(w.depsPrefix, 'lib')])
  })

  test('output paths compare as files: separators, and on Windows letter case, do not matter', () => {
    expect(samePath('C:\\work\\Out.gcode', 'c:/work/out.gcode', 'win32')).toBe(true)
    expect(samePath('C:\\work\\out.gcode', 'C:\\work\\out2.gcode', 'win32')).toBe(false)
    expect(samePath('', '', 'win32')).toBe(true)
    expect(samePath('', 'C:\\work\\out.gcode', 'win32')).toBe(false)
    if (process.platform !== 'win32') {
      expect(samePath('/work/out.gcode', '/work/./out.gcode', 'linux')).toBe(true)
      expect(samePath('/work/Out.gcode', '/work/out.gcode', 'linux')).toBe(false)
    }
  })

  test('the cases, in order', () => {
    expect(SMOKE_CASES.map((c) => c.id)).toEqual([
      'version',
      'cube-fixed',
      'sphere-adaptive',
      'pause-at-layer',
      'layers-only',
      'config-def',
      'bambu-3mf',
      'pocket-pause',
      'info',
      'filaments',
      'z-offset',
      'override-keys',
      'crash-result',
      'refusals',
    ])
    expect(new Set(SMOKE_CASES.map((c) => c.id)).size).toBe(SMOKE_CASES.length)
    // Version 3 added filaments, crash-result, the feature lengths, the relationship targets and five
    // refusals; version 4 z-offset, override-keys, config-def's obsolete keys and two refusals;
    // version 5 the lowered nozzle past the top-of-print slack, config-def's stillDefined, the old
    // name Orca renames for its value (override-keys) and the refusal of the value it drops.
    expect(SMOKE_VERSION).toBe(5)
    // A raised and a lowered nozzle; the capitalised key is one of Orca's own.
    expect(Z_OFFSETS.some((o) => o > 0) && Z_OFFSETS.some((o) => o < 0)).toBe(true)
    expect(CAPITAL_KEY.key).not.toBe(CAPITAL_KEY.key.toLowerCase())
    const plainJob = smokeJob({ command: 'slice', dir: scratch, resources: scratch, presets: { machine: join(scratch, 'm.json'), process: join(scratch, 'p.json'), filaments: [join(scratch, 'f.json')] } })
    expect(() => validateJob({ ...plainJob, overrides: { [CAPITAL_KEY.key]: CAPITAL_KEY.value } })).not.toThrow()
    // The old name passes the job format with either value; the engine renames or drops it.
    expect(() => validateJob({ ...plainJob, overrides: { [VALUE_RENAME.key]: VALUE_RENAME.renamed } })).not.toThrow()
    expect(() => validateJob({ ...plainJob, overrides: { [VALUE_RENAME.key]: VALUE_RENAME.dropped } })).not.toThrow()
    expect(VALUE_RENAME.renamed).not.toBe(VALUE_RENAME.dropped)
    expect(SMOKE_CASES.find((c) => c.id === 'override-keys')!.title).toContain(`${VALUE_RENAME.key} ${VALUE_RENAME.renamed}, read as ${VALUE_RENAME.to} = ${VALUE_RENAME.as}`)
    expect(SMOKE_PRESETS.generic2.filaments).toEqual(['Generic PLA @System', 'Generic PLA @System'])
    expect(SMOKE_PRESETS.generic2.machine).toBe(SMOKE_PRESETS.generic.machine)
  })

  test('toolChanges lists the T commands without their comments', () => {
    expect(toolChanges(scanGcode('G1 X1\nT0 ; first\n;T1 in a comment\nT1\nM104 T1 S200\n  T12  \n'))).toEqual(['T0', 'T1', 'T12'])
  })

  test('missingRelationshipTargets: absolute and relative targets must be in the archive; external ones are left out', () => {
    const names = ['[Content_Types].xml', '_rels/.rels', '3D/3dmodel.model', '3D/_rels/3dmodel.model.rels', '3D/Objects/object_1.model', 'Metadata/plate_1.gcode', 'Metadata/plate_1.png']
    const rels = new Map([
      ['_rels/.rels', '<Relationships><Relationship Target="/3D/3dmodel.model" Id="rel-1" Type="t"/><Relationship Target="/Metadata/plate_1.png" Id="rel-2" Type="t"/><Relationship Target="/Metadata/plate_1_small.png" Id="rel-3" Type="t"/></Relationships>'],
      ['3D/_rels/3dmodel.model.rels', '<Relationships><Relationship Target="Objects/object_1.model" Id="r1" Type="t"/><Relationship Target="../Metadata/top_1.png" Id="r2" Type="t"/><Relationship TargetMode="External" Target="https://example.invalid/x" Id="r3" Type="t"/></Relationships>'],
    ])
    expect(missingRelationshipTargets(names, rels)).toEqual([
      '_rels/.rels names /Metadata/plate_1_small.png, which is not in the archive',
      '3D/_rels/3dmodel.model.rels names ../Metadata/top_1.png, which is not in the archive',
    ])
    expect(missingRelationshipTargets(names, new Map([['_rels/.rels', '<Relationships><Relationship Target="/3D/3dmodel.model" Id="a" Type="t"/></Relationships>']]))).toEqual([])
  })

  test('checkFeatureLengths: stats and the G-code agree within 3% on every feature of 50 mm or more; Travel and features stats does not list are left out', () => {
    const gcode = new Map([['Outer wall', 1000], ['Inner wall', 2000], ['Custom', 120], ['Bridge', 10]])
    expect(checkFeatureLengths({ 'Outer wall': { mm: 1010 }, 'Inner wall': { mm: 1960 }, Bridge: { mm: 30 }, Travel: { mm: 5000 } }, gcode)).toEqual([])
    // The three-times-too-long lengths of the old stats, and a feature the G-code does not print.
    expect(checkFeatureLengths({ 'Outer wall': { mm: 3000 }, 'Inner wall': { mm: 2000 }, 'Sparse infill': { mm: 400 } }, gcode)).toEqual([
      'stats gives "Outer wall" 3000.0 mm, the G-code prints 1000.0 mm of it',
      'stats gives "Sparse infill" 400.0 mm, the G-code prints 0.0 mm of it',
    ])
  })

  test('flatSquare and loneTriangleObj: a model with no height, and an OBJ with a single triangle', () => {
    const flat = meshBounds(flatSquare())
    expect(flat.max[2] - flat.min[2]).toBe(0)
    expect(flat.max[0] - flat.min[0]).toBe(20)
    const obj = loneTriangleObj().trim().split('\n')
    expect(obj.filter((l) => l.startsWith('v '))).toEqual(['v 0 0 0', 'v 20 0 0', 'v 0 0 20'])
    expect(obj.filter((l) => l.startsWith('f '))).toEqual(['f 1 2 3'])
  })

  test('smokeTimeoutMs: SKETCH_ORCA_SMOKE_TIMEOUT_MS, else 15 minutes', () => {
    expect(smokeTimeoutMs({})).toBe(900_000)
    expect(smokeTimeoutMs({ SKETCH_ORCA_SMOKE_TIMEOUT_MS: '5000' })).toBe(5000)
    expect(smokeTimeoutMs({ SKETCH_ORCA_SMOKE_TIMEOUT_MS: 'soon' })).toBe(900_000)
    expect(smokeTimeoutMs({ SKETCH_ORCA_SMOKE_TIMEOUT_MS: '-1' })).toBe(900_000)
  })

  test('--case picks cases by id, in their own order', () => {
    expect(selectedCases([])).toBe(SMOKE_CASES)
    expect(selectedCases(['--case', 'pocket-pause,version']).map((c) => c.id)).toEqual(['version', 'pocket-pause'])
    expect(selectedCases(['--case=config-def']).map((c) => c.id)).toEqual(['config-def'])
    expect(() => selectedCases(['--case', 'cube,version'])).toThrow(/unknown smoke case cube; the cases are version, cube-fixed/)
  })

  test('smokePassedFor: only for the engine build the stamp and the full run\'s report name, at this case version', () => {
    const w = workPaths(resolve(scratch, 'stamp-home'))
    const exe = join(w.build, 'sketch-orca', 'sketch-orca.exe')
    mkdirSync(dirname(exe), { recursive: true })
    writeFileSync(exe, 'engine build 1')
    expect(smokePassedFor(w, exe)).toBe(false)
    const sha = createHash('sha256').update('engine build 1').digest('hex')
    const report = (passed: boolean, cases = SMOKE_CASES.map((c) => c.id), exeSha256 = sha) => {
      mkdirSync(w.smoke, { recursive: true })
      const r = { format: 'sketch-orca-smoke/1', smokeVersion: SMOKE_VERSION, exeSha256, orcaCommit: 'c', passed, cases: cases.map((id, i) => ({ id, title: id, ok: passed || i > 0, problems: [], warnings: [] })) }
      writeFileSync(join(w.smoke, 'report.json'), JSON.stringify(r))
    }
    writeStamp(w.state, 'smoke', 'inputs', { exeSha256: sha, smokeVersion: SMOKE_VERSION })
    // The stamp alone is not enough: the full run's report must say so too.
    expect(smokePassedFor(w, exe)).toBe(false)
    report(true)
    expect(smokePassedFor(w, exe)).toBe(true)
    // A later full run of the same build that failed (the stamp of the earlier pass still there).
    report(false)
    expect(smokePassedFor(w, exe)).toBe(false)
    // A report of a subset, or of another build.
    report(true, SMOKE_CASES.slice(1).map((c) => c.id))
    expect(smokePassedFor(w, exe)).toBe(false)
    report(true, undefined, 'f'.repeat(64))
    expect(smokePassedFor(w, exe)).toBe(false)
    report(true)
    expect(smokePassedFor(w, exe)).toBe(true)
    forgetSmokePass(w)
    expect(smokePassedFor(w, exe)).toBe(false)
    writeStamp(w.state, 'smoke', 'inputs', { exeSha256: sha, smokeVersion: SMOKE_VERSION - 1 })
    expect(smokePassedFor(w, exe)).toBe(false)
    writeStamp(w.state, 'smoke', 'inputs', { exeSha256: sha, smokeVersion: SMOKE_VERSION })
    expect(smokePassedFor(w, exe)).toBe(true)
    writeFileSync(exe, 'engine build 2')
    expect(smokePassedFor(w, exe)).toBe(false)
    rmSync(exe)
    expect(smokePassedFor(w, exe)).toBe(false)
  })
})

describe('exit codes, refusals and info', () => {
  const failed = (extra: Partial<JobResult> = {}): JobResult => ({
    format: RESULT_FORMAT,
    ok: false,
    code: 'file-not-found',
    message: 'the model C:\\work\\missing.stl does not exist',
    exitCode: -3,
    warnings: [],
    layers: [],
    printZ: [],
    outputs: { gcode: '', bambu3mf: '' },
    ...extra,
  })

  test('exitByte: the low byte, as Bun (8 bits) and Node on Windows (32 bits) report a negative exit code', () => {
    expect(exitByte(0)).toBe(0)
    expect(exitByte(-3)).toBe(253)
    expect(exitByte(253)).toBe(253)
    expect(exitByte(4294967293)).toBe(253)
    expect(exitByte(-21)).toBe(235)
    expect(exitByte(-100)).toBe(156)
  })

  test("checkExitCode: the engine exits with its result's exitCode, compared by the low byte", () => {
    expect(checkExitCode(0, result([0.2], { exitCode: 0 }))).toEqual([])
    expect(checkExitCode(253, failed())).toEqual([])
    expect(checkExitCode(4294967293, failed())).toEqual([])
    expect(checkExitCode(-3, failed())).toEqual([])
    expect(checkExitCode(254, failed())).toEqual(["the engine exited with 254, but its result's exitCode is -3"])
    expect(checkExitCode(1, result([0.2], { exitCode: 0 }))).toEqual(["the engine exited with 1, but its result's exitCode is 0"])
    expect(checkExitCode(0, result([0.2]))).toEqual(['the result has no exitCode'])
    expect(checkExitCode(null, failed())).toEqual(["the engine was stopped before it exited; its result's exitCode is -3"])
  })

  test('checkRefusal: the named error, exit code and object, a message and no outputs', () => {
    const want = { code: 'file-not-found', exitCode: -3 }
    expect(checkRefusal(failed(), 253, want)).toEqual([])
    expect(checkRefusal(failed({ code: 'arrange-failed', exitCode: -21, objectIndex: 0 }), 235, { code: 'arrange-failed', exitCode: -21, objectIndex: 0 })).toEqual([])
    expect(checkRefusal(result([0.2], { exitCode: 0 }), 0, want)).toEqual(['the engine did the job; expected file-not-found (-3)'])
    expect(checkRefusal(failed({ code: 'model-unreadable', exitCode: -6 }), 250, want)).toEqual([
      'the engine answered model-unreadable (the model C:\\work\\missing.stl does not exist), expected file-not-found',
      'exitCode is -6, expected -3',
    ])
    expect(checkRefusal(failed(), 254, want)).toEqual(["the engine exited with 254, but its result's exitCode is -3"])
    expect(checkRefusal(failed({ exitCode: undefined }), 253, want)).toEqual(['exitCode is missing, expected -3'])
    expect(checkRefusal(failed({ objectIndex: 0 }), 253, want)).toEqual(['objectIndex is 0, expected none'])
    expect(checkRefusal(failed(), 253, { ...want, objectIndex: 0 })).toEqual(['objectIndex is missing, expected 0'])
    expect(checkRefusal(failed({ message: ' ' }), 253, want)).toEqual(['the message is empty'])
    expect(checkRefusal(failed(), 253, { ...want, mentions: 'missing.stl' })).toEqual([])
    expect(checkRefusal(failed(), 253, { ...want, mentions: 'other.stl' })).toEqual(['the message does not name other.stl: "the model C:\\\\work\\\\missing.stl does not exist"'])
    expect(checkRefusal(failed({ outputs: { gcode: 'C:\\work\\out.gcode', bambu3mf: '' } }), 253, want)).toEqual([
      'the failed job names outputs {"gcode":"C:\\\\work\\\\out.gcode","bambu3mf":""}',
    ])
  })

  test("the refused jobs: each with what it gets wrong, and the exit code of the engine's failure table (cli/common.cpp)", () => {
    const cpp = readFileSync(join(import.meta.dir, '..', 'cli', 'common.cpp'), 'utf8')
    const table = new Map([...cpp.matchAll(/\{\s*"([a-z][a-z0-9-]*)"\s*,\s*(-?\d+)\s*\}/g)].map((m) => [m[1]!, Number(m[2])] as const))
    expect(REFUSALS.map((r) => r.id)).toEqual([
      'missing-model',
      'unknown-setting',
      'unknown-capitalised-setting',
      'preset-file-key',
      'renamed-key-dropped-value',
      'pause-below-first-layer',
      'too-big-for-the-bed',
      'two-at-one-print-z',
      'flat-model',
      'lone-triangle',
      'profile-not-at-zero',
      'layers-above-the-nozzle',
      'gcode-folder-unmakeable',
    ])
    for (const r of REFUSALS) expect(`${r.code} ${table.get(r.code)}`).toBe(`${r.code} ${r.exitCode}`)
    const dir = resolve(scratch, 'refuse')
    const res = resolve(scratch, 'resources')
    const presets = { machine: join(dir, 'machine.json'), process: join(dir, 'process.json'), filaments: [join(dir, 'filament_1.json')] }
    const made = Object.fromEntries(REFUSALS.map((r) => [r.id, r.make(join(dir, r.id), res, presets)]))
    for (const r of REFUSALS) {
      const { job } = made[r.id]!
      expect(job.command).toBe(['profile-not-at-zero', 'layers-above-the-nozzle'].includes(r.id) ? 'layers' : 'slice')
      expect(job.output.gcode).toBe(r.id === 'gcode-folder-unmakeable' ? join(dir, r.id, 'blocker', 'out.gcode') : join(dir, r.id, 'out.gcode'))
      expect(job.overrides.layer_height).toBe(String(CUBE.layerHeight))
      // The jobs written unchecked are the ones job.ts refuses itself: the engine must refuse them too.
      if (r.unchecked) expect(() => validateJob(job)).toThrow()
      else expect(() => validateJob(job)).not.toThrow()
    }
    // A failed G-code export keeps no layers (contract: a failed job has no layers, printZ or outputs).
    expect(REFUSALS.find((r) => r.id === 'gcode-folder-unmakeable')!.absent).toEqual(['layers', 'printZ', 'outputs'])
    expect(existsSync(join(dir, 'gcode-folder-unmakeable', 'blocker'))).toBe(true)
    expect(made['flat-model']!.model).toEqual(flatSquare())
    // An OBJ: Orca's STL loader would drop the lone facet and fail the whole file instead.
    expect(made['lone-triangle']!.model).toBeUndefined()
    expect(made['lone-triangle']!.job.model.path).toBe(join(dir, 'lone-triangle', 'model.obj'))
    expect(readFileSync(join(dir, 'lone-triangle', 'model.obj'), 'utf8')).toBe(loneTriangleObj())
    // The profile starts at the first layer, so the nozzle check is what refuses it.
    expect(made['layers-above-the-nozzle']!.job.objects[0]!.layerHeights).toEqual({ mode: 'profile', profile: [0, CUBE.firstLayer, 5, 0.5, CUBE.size, 0.5] })
    expect(made['missing-model']!.model).toBeUndefined()
    expect(made['missing-model']!.job.model.path).toBe(join(dir, 'missing-model', 'missing.stl'))
    expect(made['unknown-setting']!.job.overrides[REFUSE.unknownSetting]).toBe('1')
    // The message names the key, and says it is not a setting (not that Orca dropped it: it never had it).
    expect(REFUSALS.find((r) => r.id === 'unknown-setting')!.mentions).toBe(`${REFUSE.unknownSetting} is not a setting`)
    // A key of Orca's shape with capital letters passes the job format, and the engine says Orca has
    // no such setting; a preset-file key in another letter case is refused by the job format itself.
    expect(made['unknown-capitalised-setting']!.job.overrides[REFUSE.capitalisedSetting]).toBe(String(CUBE.layerHeight))
    expect(REFUSE.capitalisedSetting.toLowerCase()).toBe('layer_height')
    expect(REFUSALS.find((r) => r.id === 'unknown-capitalised-setting')).toMatchObject({ code: 'setting-invalid', mentions: 'Layer_Height is not a setting' })
    expect(made['preset-file-key']!.job.overrides[REFUSE.presetFileKey]).toBeDefined()
    expect(() => validateJob(made['preset-file-key']!.job)).toThrow(/"Inherits" belongs to the preset files/)
    expect(REFUSALS.find((r) => r.id === 'preset-file-key')).toMatchObject({ code: 'job-invalid', unchecked: true, mentions: '"Inherits" belongs to the preset files' })
    // An old name with the value Orca drops it for: the engine says it is not a setting, as for a
    // key Orca never had (handle_legacy clears both alike).
    expect(made['renamed-key-dropped-value']!.job.overrides[VALUE_RENAME.key]).toBe(VALUE_RENAME.dropped)
    expect(REFUSALS.find((r) => r.id === 'renamed-key-dropped-value')).toMatchObject({ code: 'setting-invalid', exitCode: -2, mentions: 'top_one_wall_type is not a setting' })
    expect(made['pause-below-first-layer']!.job.customGcodes).toEqual([{ printZ: REFUSE.belowFirstLayer, type: 'pause' }])
    // Below half the first layer: no layer to run the pause before (run.cpp check_custom_gcodes).
    expect(REFUSE.belowFirstLayer).toBeLessThanOrEqual(0.5 * CUBE.firstLayer)
    expect(made['too-big-for-the-bed']!.model!.triangles).toEqual(cube(REFUSE.tooBig).triangles)
    expect(REFUSALS.find((r) => r.id === 'too-big-for-the-bed')!.objectIndex).toBe(0)
  })

  test('bedOutline reads printable_area', () => {
    expect(bedOutline({ printable_area: ['0x0', '300x0', '300x300', '0x300'] })).toEqual([[0, 0], [300, 0], [300, 300], [0, 300]])
    expect(bedOutline({ printable_area: '-5x-5;205.5x-5;205.5x205;-5x205' })).toEqual([[-5, -5], [205.5, -5], [205.5, 205], [-5, 205]])
    expect(bedOutline({})).toEqual([])
    expect(() => bedOutline({ printable_area: ['0,0'] })).toThrow(/printable_area has the point "0,0"/)
  })

  test('checkInfo: only info, the object as made, the bed and the preset names', () => {
    const preset = (type: FlatPreset['type'], name: string, extra: Record<string, unknown> = {}): FlatPreset => ({ type, name, from: 'system', ...extra })
    const set: PresetSet = {
      machine: preset('machine', 'Voron 2.4 300 0.4 nozzle', { printable_area: ['0x0', '300x0', '300x300', '0x300'], printable_height: '275' }),
      process: preset('process', '0.20mm Standard @Voron'),
      filaments: [preset('filament', 'Generic PLA @System')],
    }
    const model = cube(20)
    const object = { objectIndex: 0, name: 'model.stl', instances: 1, triangles: 12, min: [0, 0, 0] as [number, number, number], max: [20, 20, 20] as [number, number, number], size: [20, 20, 20] as [number, number, number] }
    const info = {
      objects: [object],
      bed: { printableArea: [[0, 0], [300, 0], [300, 300], [0, 300]], printableHeight: 275, excludeArea: [[0, 0]], bedType: 'HighTempPlate' },
      printer: 'Voron 2.4 300 0.4 nozzle',
      process: '0.20mm Standard @Voron',
      filaments: ['Generic PLA @System'],
    }
    const ok = (extra: Partial<JobResult> = {}): JobResult => ({ ...result([]), layers: [], exitCode: 0, command: 'info', info, ...extra })
    const o = { model, fileName: 'model.stl', set }
    expect(checkInfo(ok(), o)).toEqual([])
    expect(checkInfo(ok({ command: 'slice' }), o)).toEqual(['command is "slice", expected "info"'])
    expect(checkInfo(ok({ printZ: [0.2], layers: [{ objectIndex: 0, ranges: [[0, 0.2]] }] }), o)).toEqual(['info returned 1 layer lists and 1 print Z values; it slices nothing'])
    expect(checkInfo(ok({ info: undefined }), o)).toEqual(['the result has no info'])
    expect(checkInfo(ok({ info: { ...info, objects: [] } }), o)).toEqual(['info lists 0 objects, expected 1'])
    const one = (x: Partial<typeof object>) => ok({ info: { ...info, objects: [{ ...object, ...x }] } })
    expect(checkInfo(one({ name: 'C:\\work\\model.stl' }), o)).toEqual(['the object is named "C:\\\\work\\\\model.stl", expected the file\'s name "model.stl"'])
    expect(checkInfo(one({ triangles: 24, instances: 2 }), o)).toEqual(['the object has 2 instances, expected 1', 'the object has 24 triangles, expected 12'])
    expect(checkInfo(one({ size: [20, 20, 10], max: [20, 20, 10] }), o)).toEqual(['the object\'s size is 20 x 20 x 10, expected 20 x 20 x 20'])
    expect(checkInfo(one({ max: [20, 20, 30] }), o)).toEqual(['min 0, 0, 0 and max 20, 20, 30 do not span the size 20 x 20 x 20'])
    expect(checkInfo(ok({ info: { ...info, printer: 'X1C', filaments: [] } }), o)).toEqual([
      'printer is "X1C", expected "Voron 2.4 300 0.4 nozzle"',
      'filaments are [], expected ["Generic PLA @System"]',
    ])
    expect(checkInfo(ok({ info: { ...info, bed: { printableArea: [[0, 0], [256, 0], [256, 256], [0, 256]], printableHeight: 250 } } }), o)).toEqual([
      "the bed is [[0,0],[256,0],[256,256],[0,256]], but the machine preset's printable_area is [[0,0],[300,0],[300,300],[0,300]]",
      "the bed's printable height is 250, but the machine preset's printable_height is 275",
    ])
  })
})
