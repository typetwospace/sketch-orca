// `smoke`: slices models made in code with the engine just built and checks each result exactly.
//
// Cases (one folder each under <work>/smoke, with the model, job, result, G-code and engine log):
//   version          `sketch-orca version` names the locked Orca release and commit
//   cube-fixed       20 mm cube, 0.2 mm layers, 0.25 mm first layer: exactly 100 layers, first top 0.25, last 20.05;
//                    the length stats gives each feature is the length the G-code prints of it
//   sphere-adaptive  adaptive layer height on a sphere: varying heights, all within the printer's min and max
//   pause-at-layer   a pause at print Z 5.05 appears once, in that layer before its first extrusion, as the machine's pause G-code
//   layers-only      `layers` returns the 100 ranges, names no outputs and writes no G-code
//   config-def       the settings catalogue lists layer_height, wall_loops, sparse_infill_density, enable_support... with types and enum values,
//                    and the obsolete keys (those Orca's handle_legacy drops or renames, none of them an option), saying
//                    which of them Orca still defines; with Orca's source at hand, they are the ones handle_legacy
//                    names, and each one Orca still defines is one it ignores (not a live setting misread)
//   bambu-3mf        an X1 Carbon job writes a .gcode.3mf holding Metadata/plate_1.gcode, and every part its
//                    relationships name is in the archive
//   pocket-pause     a box with a closed pocket (z 3 to 6): the first layer covering it is the one at 6.2, and a pause there
//                    comes after the last open layer and before the bridge over the pocket
//   info             `info` returns the cube (name, instances, triangles, size), the bed and the preset names, and slices nothing
//   filaments        two filaments with no filament_colour override: a change to filament 2 at 5.05 (T1, with the prime
//                    tower); a change on the first layer and support on filament 2 where none is needed (a prime
//                    tower planned but not made, patch 0004) slice instead of crashing
//   z-offset         the cube with the printer's z_offset at 0.3 and at -0.2 (an override): the expected layers, layer
//                    tags at print Z, every extrusion at its layer's print Z plus the offset, the last in the top layer;
//                    both offsets are beyond the slack of the engine's top-of-print check, so a wrong sign fails one
//   override-keys    overrides whose key has capital letters (required_nozzle_HRC, a filament setting) or is an old
//                    name Orca renames for its value (top_one_wall_type topmost, read as only_one_wall_top 1) are
//                    applied: the cube slices and the G-code's config block gives their values
//   preset-metadata  preset files that carry the lists the engine fills in itself (inherits_group...), shorter or
//                    longer than its own, and a process with a filament setting as an empty list: the cube slices,
//                    with the filament's filament_type and none of the files' lists
//   flush-volumes    the flushing volumes between filaments: a nozzle_volume past what an int holds, in the machine
//                    preset or as an override, is refused (config-invalid) instead of being converted; an override
//                    that makes filament 2 a support filament gives the matrix Orca's support volumes (0,230,700,0)
//   prime-tower      where the prime tower goes (wipe_tower_x/y): an object that sets only raft_layers keeps the
//                    print's support on filament 2, so its tower is placed as for a filament change; on the printer's
//                    bed moved by -150, -150 (0,0 at its centre, as a delta's) the tower moves with the bed; on the
//                    bed cut to a diamond, and with its back half excluded, the tower prints on the diamond and off
//                    the excluded half; a place the job sets off the diamond is config-invalid, and a tower wider
//                    than the bed arrange-failed
//   imported-settings a 3MF project (a .gcode.3mf the engine wrote) whose object has wall_loops 6 slices, as a model
//                    the job does not list, with the plate's wall_loops 2: its inner walls are the plain cube's
//   file-collisions  a --progress on the job, the model or a preset, and a --out on the job, the model, a preset or
//                    output.gcode, are refused before anything is written over them: invalid-params (exit -2), and
//                    when the result itself would replace an input, nothing written at all; also both at once, for
//                    a job that is not valid, a wrong command line, another name for the file (a hard link, a
//                    path through ..) and the result's temporary file; the job, the model and presets keep their bytes
//   crash-result     an engine that crashes in Orca's code (SKETCH_ORCA_TEST_CRASH) still writes its result:
//                    internal-error, exit code 255, no outputs
//   refusals         a missing model, an unknown setting (also one with capital letters), a key of the preset files as
//                    an override, an old name with the value Orca drops it for (top_one_wall_type none), a pause
//                    below the first layer, two custom G-codes at one print Z, a cube bigger than the bed, a flat
//                    model, a lone triangle (OBJ), layer profiles that do not start at 0 or go above the nozzle, a
//                    G-code folder that cannot be made and a work folder from the root of the current drive
//                    (\sketch-smoke-work, absolute on no system) get their named errors, exit codes and object, and
//                    write no G-code
// Every run's exit code must be the one its result gives (exitCode). The engine is run hidden, with
// a timeout; on Windows its libraries are checked first, so a missing DLL fails the case instead of
// raising a system dialog. `stage` runs some of these cases again on the staged copy (runCases).
//
// Carries OrcaSlicer code: fixedLayerProfile, layersFromProfile and layerLimits are ported to
// TypeScript from OrcaSlicer's src/libslic3r/Slicing.cpp (layer_height_profile_from_ranges with its
// lh_append, generate_object_layers, and min/max_layer_height_from_nozzle with SlicingParameters;
// OrcaSlicer v2.4.2, commit 8500fcd), 2026-10-30, to predict the layers the engine must make.
// Those portions are copyright the OrcaSlicer contributors (NOTICE).

import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { createWriteStream, existsSync, linkSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, resolve, sep } from 'node:path'
import type { Context } from '../orca.ts'
import type { Platform } from './env.ts'
import { hashTree } from './files.ts'
import { commandsAfter, configValue, extrusions, featureLengths, firstExtrusionLine, layerAtLine, layerBounds, layerHitsRect, scanGcode, segmentHitsRect, type Extrusion, type GcodeScan, type Rect } from './gcode.ts'
import { isEngineAbsolute, parseProgress, readResult, validateJob, writeJob, type CustomGcode, type Job, type JobObject, type JobResult, type Overrides } from './job.ts'
import type { OrcaLock } from './lock.ts'
import { boxTriangles, boxWithPocket, cube, meshBounds, uvSphere, writeStl, type Mesh } from './models.ts'
import { checkPresetSet, loadProfileTree, presetSet, serializePreset, settingList, settingNumber, writePresetSet, type PresetFiles, type PresetSet } from './profiles.ts'
import { readStamp, sha256File, sha256Text, writeStamp, type WorkPaths } from './paths.ts'
import { resolveRuntime, windowsRuntimeIo, type RuntimeIo } from './pe.ts'
import { Log, StepError } from './run.ts'
import { currentPlatform } from './toolchain.ts'
import { extractZipEntry, readZipEntries } from './zip.ts'

/** Bump when the cases or their expectations change: a passed stamp of an older version does not count. */
export const SMOKE_VERSION = 7
/** Folders of Orca's resources/ that libslic3r reads (resources_dir()): presets, nozzle tables, flush volumes. */
export const ENGINE_RESOURCES = ['profiles', 'info', 'flush'] as const

/**
 * Presets the cases slice with: a generic Klipper printer (plain G-code), the same printer with two
 * filaments (its prime tower is Orca's Prusa-style one), and a Bambu X1 Carbon (.gcode.3mf).
 */
export const SMOKE_PRESETS = {
  generic: { vendor: 'Voron', machine: 'Voron 2.4 300 0.4 nozzle', process: '0.20mm Standard @Voron', filaments: ['Generic PLA @System'] },
  generic2: { vendor: 'Voron', machine: 'Voron 2.4 300 0.4 nozzle', process: '0.20mm Standard @Voron', filaments: ['Generic PLA @System', 'Generic PLA @System'] },
  bambu: { vendor: 'BBL', machine: 'Bambu Lab X1 Carbon 0.4 nozzle', process: '0.20mm Standard @BBL X1C', filaments: ['Bambu PLA Basic @BBL X1C'] },
} as const

/** The lists the engine fills itself in its merge of the presets (cli/presets.cpp, merge_metadata_key); preset-metadata gives them in the preset files. */
export const PRESET_METADATA = ['inherits_group', 'different_settings_to_system', 'compatible_machine_expression_group', 'compatible_process_expression_group', 'print_compatible_printers'] as const
export const PRESET_METADATA_MARK = 'sketch-smoke-preset-list'

/** Settings that keep the generic cases' G-code plain: no brim, skirt, supports, arcs or z alignment. */
export const PLAIN_OVERRIDES: Overrides = {
  brim_type: 'no_brim',
  skirt_loops: '0',
  enable_support: '0',
  enable_arc_fitting: '0',
  precise_z_height: '0',
}

export const CUBE = { size: 20, layerHeight: 0.2, firstLayer: 0.25, layers: 100, lastTop: 20.05 } as const
export const PAUSE_LAYER_INDEX = 24
export const POCKET = { size: [20, 20, 10] as [number, number, number], min: [5, 5, 3] as [number, number, number], max: [15, 15, 6] as [number, number, number], layerHeight: 0.2, firstLayer: 0.2, coverIndex: 30, coverZ: 6.2 } as const
export const SPHERE = { radius: 10, quality: 0.5 } as const
/**
 * The printer z_offset values z-offset slices the cube with (an override of the machine setting):
 * raised and lowered, each by more than the slack of the engine's top-of-print check (half the last
 * layer plus 0.01 mm, 0.11 mm on the cube), so a check that counts z_offset twice refuses the raised
 * one and a check that takes it off refuses the lowered one.
 */
export const Z_OFFSETS = [0.3, -0.2] as const
/** A setting of Orca's whose key has capital letters (a filament setting that changes no toolpath), and the value override-keys gives it. */
export const CAPITAL_KEY = { key: 'required_nozzle_HRC', value: '5' } as const
/**
 * An obsolete key Orca renames for some values only (PrintConfig.cpp handle_legacy): top_one_wall_type
 * is read as only_one_wall_top = 1 unless it is "none", which Orca drops. override-keys gives it a
 * value Orca renames, refusals the value Orca drops.
 */
export const VALUE_RENAME = { key: 'top_one_wall_type', renamed: 'topmost', dropped: 'none', to: 'only_one_wall_top', as: '1' } as const

/** Comparison tolerance for heights read back from the engine (far below any layer height). */
export const Z_TOLERANCE = 1e-4
/** Comparison tolerance for heights read from the G-code, which writes Z with 3 decimals. */
export const GCODE_Z_TOLERANCE = 1e-3

// ---------------------------------------------------------------- expectations

const ORCA_EPSILON = 1e-4

/**
 * Orca's layer height profile for a fixed layer height without height ranges
 * (Slicing.cpp layer_height_profile_from_ranges, first object layer fixed).
 */
export function fixedLayerProfile(objectHeight: number, firstLayer: number, layerHeight: number): number[] {
  const p: number[] = []
  const approx = (a: number, b: number) => Math.abs(a - b) < ORCA_EPSILON
  const append = (z: number, h: number) => {
    if (p.length > 0 && approx(p[p.length - 1]!, h)) {
      if (approx(p[p.length - 2]!, z)) return
      if (p.length >= 4 && approx(p[p.length - 3]!, h)) {
        p[p.length - 2] = z
        return
      }
    }
    p.push(z, h)
  }
  const lastZ = () => (p.length === 0 ? 0 : p[p.length - 2]!)
  const hi = Math.min(firstLayer, objectHeight)
  if (0 + ORCA_EPSILON < hi) {
    append(0, firstLayer)
    append(hi, firstLayer)
  }
  if (lastZ() < objectHeight) {
    const z = lastZ()
    append(z, layerHeight)
    append(objectHeight, layerHeight)
  }
  return p
}

/** The print Z of every layer Orca makes from a layer height profile (Slicing.cpp generate_object_layers, no shrinkage, no z alignment). */
export function layersFromProfile(objectHeight: number, firstLayer: number, minLayerHeight: number, profile: number[]): number[] {
  const out: number[] = [firstLayer]
  let printZ = firstLayer
  let idx = 0
  let sliceZ = printZ + 0.5 * minLayerHeight
  while (sliceZ < objectHeight) {
    let height = minLayerHeight
    if (idx < profile.length) {
      let next = idx + 2
      for (;;) {
        if (next >= profile.length || sliceZ < profile[next]!) break
        idx = next
        next += 2
      }
      const z1 = profile[idx]!
      const h1 = profile[idx + 1]!
      height = h1
      if (next < profile.length) {
        const z2 = profile[next]!
        const h2 = profile[next + 1]!
        const t = (sliceZ - z1) / (z2 - z1)
        height = (1 - t) * h1 + t * h2
      }
    }
    sliceZ = printZ + 0.5 * height
    if (sliceZ >= objectHeight) break
    printZ += height
    sliceZ = printZ + 0.5 * minLayerHeight
    out.push(printZ)
  }
  return out
}

/**
 * The layer heights Orca slices within (Slicing.cpp min/max_layer_height_from_nozzle and
 * SlicingParameters): the printer's min_layer_height (0 means 0.07, never below 0.01) and
 * max_layer_height (0 means three quarters of the nozzle), widened to hold the layer height.
 */
export function layerLimits(machine: Record<string, unknown>, layerHeight: number): { min: number; max: number } {
  const cfgMin = settingNumber(machine, 'min_layer_height') ?? 0
  const min = cfgMin === 0 ? 0.07 : Math.max(0.01, cfgMin)
  const cfgMax = settingNumber(machine, 'max_layer_height') ?? 0
  const max = Math.max(min, cfgMax === 0 ? 0.75 * (settingNumber(machine, 'nozzle_diameter') ?? 0.4) : cfgMax)
  return { min: Math.min(min, layerHeight), max: Math.max(max, layerHeight) }
}

/** The print Z of every layer of an object sliced at a fixed layer height. */
export function expectedFixedLayers(objectHeight: number, firstLayer: number, layerHeight: number, minLayerHeight: number): number[] {
  return layersFromProfile(objectHeight, firstLayer, Math.min(minLayerHeight, layerHeight), fixedLayerProfile(objectHeight, firstLayer, layerHeight))
}

/** The first layer whose slicing plane (its middle) is above `ceilingZ`: the layer that closes a pocket ending there. */
export function firstCoveringLayer(ranges: [number, number][], ceilingZ: number): number | undefined {
  const i = ranges.findIndex(([lo, hi]) => (lo + hi) / 2 > ceilingZ + 1e-9)
  return i < 0 ? undefined : i
}

const near = (a: number, b: number) => Math.abs(a - b) <= Z_TOLERANCE

/** The result is ok, with one object's layer ranges matching printZ and touching end to end. */
export function checkLayerList(result: JobResult): string[] {
  const p: string[] = []
  if (!result.ok) return [`the engine failed: ${result.code}: ${result.message}`]
  if (result.layers.length !== 1) return [`expected the layers of 1 object, got ${result.layers.length}`]
  const ranges = result.layers[0]!.ranges
  if (ranges.length !== result.printZ.length) p.push(`${ranges.length} layer ranges but ${result.printZ.length} print Z values`)
  ranges.forEach(([lo, hi], i) => {
    if (i === 0 && !near(lo, 0)) p.push(`the first layer starts at ${lo}, not 0`)
    if (i > 0 && !near(lo, ranges[i - 1]![1])) p.push(`layer ${i + 1} starts at ${lo}, but the layer below ends at ${ranges[i - 1]![1]}`)
    if (result.printZ[i] !== undefined && !near(hi, result.printZ[i]!)) p.push(`layer ${i + 1} ends at ${hi} but its print Z is ${result.printZ[i]}`)
  })
  return p
}

/** Exactly the expected layers: same count, each print Z within Z_TOLERANCE. */
export function checkFixedLayers(result: JobResult, expected: number[]): string[] {
  const p = checkLayerList(result)
  if (p.length > 0) return p
  if (result.printZ.length !== expected.length) p.push(`expected ${expected.length} layers, got ${result.printZ.length}`)
  const n = Math.min(result.printZ.length, expected.length)
  for (let i = 0; i < n; i++) {
    if (!near(result.printZ[i]!, expected[i]!)) {
      p.push(`layer ${i + 1} has print Z ${result.printZ[i]}, expected ${expected[i]}`)
      if (p.length >= 5) break
    }
  }
  return p
}

/** Adaptive layers: first layer as set, every other within [min, max], at least three distinct heights spanning 0.05 mm, reaching the top. */
export function checkAdaptiveLayers(result: JobResult, o: { firstLayer: number; minLayer: number; maxLayer: number; objectHeight: number }): string[] {
  const p = checkLayerList(result)
  if (p.length > 0) return p
  const ranges = result.layers[0]!.ranges
  if (ranges.length < 3) return [`only ${ranges.length} layers`]
  const first = ranges[0]![1] - ranges[0]![0]
  if (!near(first, o.firstLayer)) p.push(`the first layer is ${first} mm, expected ${o.firstLayer}`)
  const heights = ranges.slice(1).map(([lo, hi]) => hi - lo)
  heights.forEach((h, i) => {
    if (h < o.minLayer - Z_TOLERANCE || h > o.maxLayer + Z_TOLERANCE) p.push(`layer ${i + 2} is ${h.toFixed(4)} mm, outside the printer's ${o.minLayer}..${o.maxLayer}`)
  })
  const distinct = new Set(heights.map((h) => h.toFixed(3)))
  const spread = Math.max(...heights) - Math.min(...heights)
  if (distinct.size < 3 || spread < 0.05) p.push(`the heights do not vary: ${distinct.size} distinct, spread ${spread.toFixed(4)} mm`)
  const top = ranges[ranges.length - 1]![1]
  if (top < o.objectHeight - o.maxLayer - Z_TOLERANCE || top > o.objectHeight + o.maxLayer + Z_TOLERANCE) p.push(`the last layer ends at ${top}, not near the top ${o.objectHeight}`)
  return p
}

/** The G-code's layer changes match the result's print Z values. */
export function checkGcodeLayers(scan: GcodeScan, printZ: number[]): string[] {
  const p: string[] = []
  if (scan.layers.length !== printZ.length) p.push(`the G-code has ${scan.layers.length} layer changes, the result ${printZ.length} layers`)
  const n = Math.min(scan.layers.length, printZ.length)
  for (let i = 0; i < n; i++) {
    if (Math.abs(scan.layers[i]!.z - printZ[i]!) > 1e-3) {
      p.push(`G-code layer ${i + 1} is at Z ${scan.layers[i]!.z}, the result says ${printZ[i]}`)
      break
    }
  }
  return p
}

/**
 * Every extrusion of a layer is at that layer's print Z plus the printer's z_offset, and the last one
 * is in the top layer. The nozzle moves to print Z plus z_offset (GCode::change_layer), while the
 * result and the layer tags give print Z.
 */
export function checkExtrusionHeights(scan: GcodeScan, printZ: number[], zOffset: number): string[] {
  const moves = extrusions(scan).filter((m) => m.layer >= 0)
  if (moves.length === 0) return ['the G-code extrudes nothing after its first layer change']
  const p: string[] = []
  const shown = 3
  let off = 0
  for (const m of moves) {
    const z = printZ[m.layer]
    if (z === undefined) {
      p.push(`line ${m.line + 1} extrudes in layer ${m.layer + 1}, but the result has ${printZ.length} layers`)
      return p
    }
    if (!(Math.abs(m.z - (z + zOffset)) <= GCODE_Z_TOLERANCE)) {
      off++
      if (off <= shown) p.push(`line ${m.line + 1} extrudes at Z ${m.z}, but its layer (${m.layer + 1}) is at print Z ${z}, plus z_offset ${zOffset}`)
    }
  }
  if (off > shown) p.push(`${off - shown} more extrusions are off their layer's Z`)
  const last = moves[moves.length - 1]!
  if (last.layer !== printZ.length - 1) p.push(`the last extrusion is in layer ${last.layer + 1}, not in the top layer (${printZ.length})`)
  return p
}

/** The G-code's config block gives `key` the value `want`: as text, or as a number within 1e-9. */
export function checkConfigValue(scan: GcodeScan, key: string, want: string | number): string[] {
  const got = configValue(scan.lines, key)
  if (got === undefined) return [`the G-code's config block has no ${key}`]
  const same = typeof want === 'number' ? got.trim() !== '' && Math.abs(Number(got) - want) <= 1e-9 : got === want
  return same ? [] : [`the G-code's config block gives ${key} = ${JSON.stringify(got)}, expected ${JSON.stringify(String(want))}`]
}

/** The machine's pause G-code as the commands it emits (comments and blank lines dropped). */
export function pauseCommands(pauseGcode: string): string[] {
  return pauseGcode
    .split(/\r?\n/)
    .map((l) => l.replace(/;.*$/, '').trim())
    .filter((l) => l !== '')
}

/** One pause, inside the layer at `printZ`, before that layer's first extrusion, followed by the machine's pause G-code. */
export function checkPause(scan: GcodeScan, printZ: number, pauseGcode: string): string[] {
  const p: string[] = []
  if (scan.pauses.length !== 1) return [`expected 1 pause in the G-code, found ${scan.pauses.length}`]
  const at = scan.pauses[0]!
  const layer = layerAtLine(scan, at)
  if (!layer) return ['the pause comes before the first layer']
  if (Math.abs(layer.z - printZ) > 1e-3) p.push(`the pause is in the layer at Z ${layer.z}, expected ${printZ}`)
  const first = firstExtrusionLine(extrusions(scan), layer.index)
  if (first === undefined) p.push(`the layer at Z ${layer.z} has no extrusion`)
  else if (first < at) p.push(`the pause (line ${at + 1}) comes after the layer's first extrusion (line ${first + 1})`)
  const want = pauseCommands(pauseGcode)
  if (want.length === 0) {
    p.push('the machine preset has no pause G-code')
  } else if (!/[[{]/.test(pauseGcode)) {
    const got = commandsAfter(scan.lines, at, want.length)
    if (got.join('\n') !== want.join('\n')) p.push(`after the pause tag: ${JSON.stringify(got)}, expected the machine's pause G-code ${JSON.stringify(want)}`)
  }
  return p
}

/**
 * The pocket closes where planned: the layer before the pause extrudes nowhere inside the pocket
 * (shrunk by `margin`), the pause layer does (the bridge over it). The pocket's place on the bed
 * comes from the open layer's outline, the box being centred on it.
 */
export function checkPocketCover(scan: GcodeScan, pocketHalf: [number, number], margin: number): string[] {
  if (scan.pauses.length !== 1) return [`expected 1 pause, found ${scan.pauses.length}`]
  const pauseLayer = layerAtLine(scan, scan.pauses[0]!)
  if (!pauseLayer || pauseLayer.index === 0) return ['the pause is not after a first layer']
  const moves = extrusions(scan)
  const open = pauseLayer.index - 1
  const outline = layerBounds(moves, open)
  if (!outline) return [`the layer below the pause (Z ${scan.layers[open]!.z}) has no extrusion`]
  const cx = (outline.minX + outline.maxX) / 2
  const cy = (outline.minY + outline.maxY) / 2
  const rect: Rect = { minX: cx - pocketHalf[0] + margin, maxX: cx + pocketHalf[0] - margin, minY: cy - pocketHalf[1] + margin, maxY: cy + pocketHalf[1] - margin }
  const p: string[] = []
  if (layerHitsRect(moves, open, rect)) p.push(`the layer at Z ${scan.layers[open]!.z}, below the pause, already extrudes over the pocket`)
  if (!layerHitsRect(moves, pauseLayer.index, rect)) p.push(`the layer at Z ${pauseLayer.z}, after the pause, does not cover the pocket`)
  return p
}

/** `sketch-orca version` output names the locked release. */
export function checkVersion(stdout: string, lock: OrcaLock): string[] {
  let v: unknown
  const text = stdout.trim()
  try {
    v = JSON.parse(text)
  } catch {
    const last = text.split(/\r?\n/).pop() ?? ''
    try {
      v = JSON.parse(last)
    } catch {
      return [`version did not print JSON: ${JSON.stringify(text.slice(0, 200))}`]
    }
  }
  if (typeof v !== 'object' || v === null) return ['version did not print a JSON object']
  const o = v as Record<string, unknown>
  const p: string[] = []
  if (o.engine !== 'sketch-orca') p.push(`engine is ${JSON.stringify(o.engine)}, expected "sketch-orca"`)
  if (typeof o.engineVersion !== 'string' || o.engineVersion === '') p.push('engineVersion is missing')
  if (o.orcaVersion !== lock.version) p.push(`orcaVersion is ${JSON.stringify(o.orcaVersion)}, the lock says ${lock.version}`)
  if (o.orcaCommit !== lock.commit) p.push(`orcaCommit is ${JSON.stringify(o.orcaCommit)}, the lock says ${lock.commit}`)
  return p
}

export interface OptionDef {
  key: string
  /** Orca's type without the "co" prefix, first letter lower case: float, floats, int, percent, floatOrPercent, bool, enum, string... */
  type: string
  enumValues: string[]
}

function normalizeType(t: unknown): string {
  if (typeof t !== 'string') return ''
  const s = /^co[A-Z]/.test(t) ? t.slice(2) : t
  return s.charAt(0).toLowerCase() + s.slice(1)
}

/** An enum's values: a list of strings, or of { value, label } objects. */
function enumOf(d: Record<string, unknown>): string[] {
  const valueOf = (x: unknown): string | undefined =>
    typeof x === 'string' ? x : typeof x === 'object' && x !== null && typeof (x as Record<string, unknown>).value === 'string' ? ((x as Record<string, unknown>).value as string) : undefined
  const pick = (v: unknown): string[] | undefined => {
    if (!Array.isArray(v)) return undefined
    const values = v.map(valueOf)
    return values.every((x) => x !== undefined) ? (values as string[]) : undefined
  }
  const e = d.enum
  return (
    pick(d.enumValues) ??
    pick(d.enum_values) ??
    pick(e) ??
    (typeof e === 'object' && e !== null ? pick((e as Record<string, unknown>).values) : undefined) ??
    pick(d.values) ??
    []
  )
}

/** Reads `config-def` output: options as a list of { key, type, ... } or an object keyed by option. */
export function parseConfigDef(value: unknown): Map<string, OptionDef> {
  const out = new Map<string, OptionDef>()
  if (typeof value !== 'object' || value === null) throw new Error('config-def output is not a JSON object')
  const root = value as Record<string, unknown>
  const options = root.options ?? root.settings ?? root
  const add = (key: unknown, d: unknown) => {
    if (typeof key !== 'string' || typeof d !== 'object' || d === null) return
    const r = d as Record<string, unknown>
    out.set(key, { key, type: normalizeType(r.type), enumValues: enumOf(r) })
  }
  if (Array.isArray(options)) {
    for (const d of options) if (typeof d === 'object' && d !== null) add((d as Record<string, unknown>).key ?? (d as Record<string, unknown>).name ?? (d as Record<string, unknown>).id, d)
  } else if (typeof options === 'object' && options !== null) {
    for (const [k, d] of Object.entries(options as Record<string, unknown>)) add(k, d)
  }
  if (out.size === 0) throw new Error('config-def output lists no options')
  return out
}

/** What config-def must hold: [key, type, enum values that must be among its values]. */
export const CONFIG_DEF_EXPECT: [string, string, string[]][] = [
  ['layer_height', 'float', []],
  ['initial_layer_print_height', 'float', []],
  ['wall_loops', 'int', []],
  ['sparse_infill_density', 'percent', []],
  ['enable_support', 'bool', []],
  ['sparse_infill_pattern', 'enum', ['grid', 'gyroid', 'honeycomb']],
  ['support_type', 'enum', ['normal(auto)', 'tree(auto)']],
  ['seam_position', 'enum', ['nearest', 'aligned', 'back', 'random']],
  ['brim_type', 'enum', ['no_brim']],
  ['machine_pause_gcode', 'string', []],
  ['nozzle_diameter', 'floats', []],
  ['min_layer_height', 'floats', []],
]

export function checkConfigDef(defs: Map<string, OptionDef>): string[] {
  const p: string[] = []
  for (const [key, type, values] of CONFIG_DEF_EXPECT) {
    const d = defs.get(key)
    if (!d) {
      p.push(`config-def has no ${key}`)
      continue
    }
    if (d.type !== type) p.push(`${key} has type "${d.type}", expected "${type}"`)
    const missing = values.filter((v) => !d.enumValues.includes(v))
    if (missing.length > 0) p.push(`${key} lacks the enum values ${missing.join(', ')} (has ${d.enumValues.length})`)
  }
  if (defs.size < 300) p.push(`config-def lists only ${defs.size} options; Orca has far more`)
  return p
}

/** The keys Orca's PrintConfigDef::handle_legacy names: those it compares a setting's key with, and its set of ignored keys. */
export interface LegacyKeys {
  compared: string[]
  ignored: string[]
}

/**
 * The keys handle_legacy names in Orca's src/libslic3r/PrintConfig.cpp, read as cli/CMakeLists.txt
 * reads them into the engine (opt_key == "..." in its body, and its `ignore = { ... }` set), each once.
 */
export function legacyKeysFromSource(printConfigCpp: string): LegacyKeys {
  const from = printConfigCpp.indexOf('void PrintConfigDef::handle_legacy(')
  const to = printConfigCpp.indexOf('void PrintConfigDef::handle_legacy_composite(')
  if (from < 0 || to <= from) throw new Error('PrintConfig.cpp has no PrintConfigDef::handle_legacy followed by handle_legacy_composite')
  const body = printConfigCpp.slice(from, to)
  const compared = [...new Set([...body.matchAll(/opt_key[ \t\r\n]*==[ \t\r\n]*"([A-Za-z][A-Za-z0-9_]*)"/g)].map((m) => m[1]!))]
  const at = body.indexOf('ignore = {')
  if (at < 0) throw new Error('handle_legacy in PrintConfig.cpp has no set of ignored keys (ignore = { ... })')
  const end = body.indexOf('};', at)
  if (end < 0) throw new Error('the set of ignored keys in handle_legacy (PrintConfig.cpp) does not end with };')
  const ignored = [...new Set([...body.slice(at, end).matchAll(/"([A-Za-z][A-Za-z0-9_]*)"/g)].map((m) => m[1]!))]
  if (compared.length === 0 || ignored.length === 0) throw new Error(`no legacy keys found in handle_legacy (${compared.length} compared, ${ignored.length} ignored)`)
  return { compared, ignored }
}

/** The first place a list is not sorted with each key once: the index of the key that comes too late, else -1. */
function unsortedAt(keys: string[]): number {
  return keys.findIndex((k, i) => i > 0 && !(keys[i - 1]! < k))
}

/**
 * config-def's `obsolete` (the keys Orca drops or renames whenever it reads them) is a sorted list of
 * keys, none of them an option (config-def leaves them out, so that check guards config_def.cpp's
 * own construction), and `renamed` maps some of them to options. `stillDefined` lists, sorted, the
 * obsolete keys Orca still defines. With `legacy` (Orca's source), the list is what handle_legacy
 * says: every key it ignores, every key it reads that is no option, and only keys it names; and each
 * key Orca still defines is one handle_legacy ignores, since Orca retires a setting it still defines
 * that way. A defined key it only compares with is a live setting whose value it fixes
 * (wall_direction): listed as obsolete, it is one config-def's probe values misread.
 */
export function checkObsoleteKeys(root: unknown, defs: Map<string, OptionDef>, legacy?: LegacyKeys): string[] {
  const r = typeof root === 'object' && root !== null ? (root as Record<string, unknown>) : {}
  const list = r.obsolete
  if (!Array.isArray(list)) return ['config-def has no list of obsolete keys']
  if (list.length === 0) return ['config-def lists no obsolete keys']
  if (!list.every((k) => typeof k === 'string' && k !== '')) return ["config-def's obsolete keys are not all keys"]
  const keys = list as string[]
  const p: string[] = []
  const unsorted = unsortedAt(keys)
  if (unsorted > 0) p.push(`config-def's obsolete keys are not sorted, each once: ${keys[unsorted - 1]} comes before ${keys[unsorted]}`)
  const options = keys.filter((k) => defs.has(k))
  if (options.length > 0) p.push(`config-def lists obsolete keys as options too: ${options.join(', ')}`)
  const obsolete = new Set(keys)
  let stillDefined: string[] | undefined
  if (r.stillDefined === undefined) {
    p.push('config-def does not say which obsolete keys Orca still defines (stillDefined)')
  } else if (!Array.isArray(r.stillDefined) || !r.stillDefined.every((k) => typeof k === 'string' && k !== '')) {
    p.push("config-def's stillDefined is not a list of keys")
  } else {
    stillDefined = r.stillDefined as string[]
    const at = unsortedAt(stillDefined)
    if (at > 0) p.push(`config-def's stillDefined keys are not sorted, each once: ${stillDefined[at - 1]} comes before ${stillDefined[at]}`)
    for (const k of stillDefined) if (!obsolete.has(k)) p.push(`config-def says Orca still defines ${k}, which it does not list as obsolete`)
  }
  if (r.renamed !== undefined) {
    if (typeof r.renamed !== 'object' || r.renamed === null || Array.isArray(r.renamed)) {
      p.push("config-def's renamed is not an object")
    } else {
      for (const [from, to] of Object.entries(r.renamed as Record<string, unknown>)) {
        if (!obsolete.has(from)) p.push(`config-def renames ${from}, which it does not list as obsolete`)
        if (typeof to !== 'string' || !defs.has(to)) p.push(`config-def renames ${from} to ${JSON.stringify(to)}, which is not an option`)
      }
    }
  }
  if (legacy) {
    const named = new Set([...legacy.compared, ...legacy.ignored])
    const kept = legacy.ignored.filter((k) => !obsolete.has(k))
    if (kept.length > 0) p.push(`handle_legacy ignores keys config-def does not list as obsolete: ${kept.join(', ')}`)
    const lost = legacy.compared.filter((k) => !defs.has(k) && !obsolete.has(k))
    if (lost.length > 0) p.push(`handle_legacy reads keys that config-def lists neither as options nor as obsolete: ${lost.join(', ')}`)
    const unnamed = keys.filter((k) => !named.has(k))
    if (unnamed.length > 0) p.push(`config-def lists obsolete keys handle_legacy does not name: ${unnamed.join(', ')}`)
    const ignored = new Set(legacy.ignored)
    const live = (stillDefined ?? []).filter((k) => obsolete.has(k) && !ignored.has(k))
    if (live.length > 0) p.push(`config-def lists as obsolete keys Orca still defines that handle_legacy does not ignore (live settings its probe values misread, most likely): ${live.join(', ')}`)
  }
  return p
}

/** A Bambu .gcode.3mf holding Metadata/plate_1.gcode (with Bambu layer tags, one per layer of the result, and a matching .md5 if present). */
export function checkBambu3mf(zip: Buffer, printZ: number[]): string[] {
  const entries = readZipEntries(zip)
  const g = entries.find((e) => e.name === 'Metadata/plate_1.gcode')
  if (!g) return [`no Metadata/plate_1.gcode in the archive (it holds ${entries.map((e) => e.name).join(', ')})`]
  const gcode = extractZipEntry(zip, g)
  const p: string[] = []
  const scan = scanGcode(gcode.toString('utf8'))
  if (scan.style !== 'bbl') p.push(`plate_1.gcode does not use Bambu layer tags (style ${scan.style ?? 'none'})`)
  p.push(...checkGcodeLayers(scan, printZ))
  const md5 = entries.find((e) => e.name === 'Metadata/plate_1.gcode.md5')
  if (md5) {
    const want = extractZipEntry(zip, md5).toString('utf8').trim().toLowerCase()
    const got = createHash('md5').update(gcode).digest('hex')
    if (want !== got) p.push(`Metadata/plate_1.gcode.md5 says ${want}, the G-code hashes to ${got}`)
  }
  const rels = new Map(entries.filter((e) => e.name.endsWith('.rels')).map((e) => [e.name, extractZipEntry(zip, e).toString('utf8')]))
  p.push(...missingRelationshipTargets(entries.map((e) => e.name), rels))
  return p
}

/** The tool commands (T0, T1, ...) of the G-code, in order, comments left out. */
export function toolChanges(scan: GcodeScan): string[] {
  return scan.lines.map((l) => l.split(';')[0]!.trim()).filter((l) => /^T\d+$/.test(l))
}

/**
 * The relationships (OPC .rels parts) whose target is not in the archive: a printer that follows
 * them (a thumbnail Orca did not write) finds nothing. Absolute targets start at the archive root,
 * relative ones at the folder of the part the .rels file belongs to; external targets are left out.
 */
export function missingRelationshipTargets(names: string[], rels: Map<string, string>): string[] {
  const have = new Set(names)
  const out: string[] = []
  for (const [relsName, text] of rels) {
    // "<dir>/_rels/<part>.rels" belongs to "<dir>/<part>"; relative targets start at <dir>.
    const base = relsName.replace(/(^|\/)_rels\/[^/]*$/, '$1')
    for (const m of text.matchAll(/<Relationship\b[^>]*>/g)) {
      const tag = m[0]
      if (/TargetMode\s*=\s*"External"/i.test(tag)) continue
      const target = /\bTarget\s*=\s*"([^"]*)"/.exec(tag)?.[1]
      if (target === undefined) continue
      const parts: string[] = []
      for (const seg of (target.startsWith('/') ? target.slice(1) : base + target).split('/')) {
        if (seg === '..') parts.pop()
        else if (seg !== '.' && seg !== '') parts.push(seg)
      }
      const path = parts.join('/')
      if (!have.has(path)) out.push(`${relsName} names ${target}, which is not in the archive`)
    }
  }
  return out
}

/**
 * The length stats gives each feature against the length the G-code prints of it (featureLengths),
 * for the features with at least `minMm` either way: they may differ by `tolerance` (arcs are
 * chords in the scan). Travel is not printed, so it is left out; so are G-code features stats does
 * not list (the start G-code's purge line).
 */
export function checkFeatureLengths(features: Record<string, { mm: number }>, gcode: Map<string, number>, minMm = 50, tolerance = 0.03): string[] {
  const out: string[] = []
  for (const [role, f] of Object.entries(features)) {
    if (role === 'Travel') continue
    const printed = gcode.get(role) ?? 0
    if (f.mm < minMm && printed < minMm) continue
    if (printed === 0 || Math.abs(f.mm / printed - 1) > tolerance) out.push(`stats gives "${role}" ${f.mm.toFixed(1)} mm, the G-code prints ${printed.toFixed(1)} mm of it`)
  }
  return out
}

/** The low byte of an exit code: all every parent sees of a negative one (Bun on Windows and POSIX give 8 bits, Node on Windows all 32). */
export function exitByte(code: number): number {
  return code & 0xff
}

/** The engine exited with the code its result gives (exitCode), compared by the low byte. */
export function checkExitCode(processCode: number | null, result: JobResult): string[] {
  if (result.exitCode === undefined) return ['the result has no exitCode']
  if (processCode === null) return [`the engine was stopped before it exited; its result's exitCode is ${result.exitCode}`]
  return exitByte(processCode) === exitByte(result.exitCode) ? [] : [`the engine exited with ${processCode}, but its result's exitCode is ${result.exitCode}`]
}

/** What a refused job must get: the contract's named error (protocol/README.md), its exit code and the object it is about. */
export interface RefusalExpectation {
  code: string
  exitCode: number
  objectIndex?: number
  /** What the message must name (the setting, the file). */
  mentions?: string
}

/** A refused job: not ok, with the named error, its exit code (in the result and as the engine's own) and object, a message naming the culprit, and no outputs. */
export function checkRefusal(result: JobResult, processCode: number | null, want: RefusalExpectation): string[] {
  if (result.ok) return [`the engine did the job; expected ${want.code} (${want.exitCode})`]
  const p: string[] = []
  if (result.code !== want.code) p.push(`the engine answered ${result.code} (${result.message}), expected ${want.code}`)
  if (result.exitCode !== want.exitCode) p.push(`exitCode is ${result.exitCode ?? 'missing'}, expected ${want.exitCode}`)
  if (result.exitCode !== undefined) p.push(...checkExitCode(processCode, result))
  if (result.objectIndex !== want.objectIndex) p.push(`objectIndex is ${result.objectIndex ?? 'missing'}, expected ${want.objectIndex ?? 'none'}`)
  if (result.message.trim() === '') p.push('the message is empty')
  else if (want.mentions !== undefined && !result.message.includes(want.mentions)) p.push(`the message does not name ${want.mentions}: ${JSON.stringify(result.message)}`)
  if (result.outputs.gcode !== '' || result.outputs.bambu3mf !== '') p.push(`the failed job names outputs ${JSON.stringify(result.outputs)}`)
  return p
}

/** The bed outline of a machine preset, from printable_area ("0x0", "300x0", ...). */
export function bedOutline(machine: Record<string, unknown>): [number, number][] {
  return settingList(machine, 'printable_area').map((pt): [number, number] => {
    const m = /^\s*([-+]?\d*\.?\d+)\s*x\s*([-+]?\d*\.?\d+)\s*$/.exec(pt)
    if (!m) throw new Error(`printable_area has the point ${JSON.stringify(pt)}`)
    return [Number(m[1]), Number(m[2])]
  })
}

/**
 * `info` on a model file of one object: only info (no layers, print Z, outputs or stats), the object
 * as it was made (named after the file, one instance, its triangles and size), the bed of the machine
 * preset and the names of the presets.
 */
export function checkInfo(result: JobResult, o: { model: Mesh; fileName: string; set: PresetSet }): string[] {
  const p: string[] = []
  if (result.command !== 'info') p.push(`command is ${JSON.stringify(result.command)}, expected "info"`)
  if (result.layers.length > 0 || result.printZ.length > 0) p.push(`info returned ${result.layers.length} layer lists and ${result.printZ.length} print Z values; it slices nothing`)
  if (result.outputs.gcode !== '' || result.outputs.bambu3mf !== '') p.push(`info names outputs ${JSON.stringify(result.outputs)}`)
  if (result.stats) p.push('info returned stats')
  const info = result.info
  if (!info) return [...p, 'the result has no info']
  if (info.objects.length !== 1) return [...p, `info lists ${info.objects.length} objects, expected 1`]
  const obj = info.objects[0]!
  const b = meshBounds(o.model)
  const size = [b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]]
  if (obj.objectIndex !== 0) p.push(`the object's index is ${obj.objectIndex}, expected 0`)
  if (obj.name !== o.fileName) p.push(`the object is named ${JSON.stringify(obj.name)}, expected the file's name ${JSON.stringify(o.fileName)}`)
  if (obj.instances !== 1) p.push(`the object has ${obj.instances} instances, expected 1`)
  if (obj.triangles !== o.model.triangles.length) p.push(`the object has ${obj.triangles} triangles, expected ${o.model.triangles.length}`)
  if (!size.every((s, k) => near(obj.size[k]!, s))) p.push(`the object's size is ${obj.size.join(' x ')}, expected ${size.join(' x ')}`)
  if (![0, 1, 2].every((k) => near(obj.max[k]! - obj.min[k]!, obj.size[k]!))) p.push(`min ${obj.min.join(', ')} and max ${obj.max.join(', ')} do not span the size ${obj.size.join(' x ')}`)
  if (info.printer !== o.set.machine.name) p.push(`printer is ${JSON.stringify(info.printer)}, expected ${JSON.stringify(o.set.machine.name)}`)
  if (info.process !== o.set.process.name) p.push(`process is ${JSON.stringify(info.process)}, expected ${JSON.stringify(o.set.process.name)}`)
  const filaments = o.set.filaments.map((f) => f.name)
  if (JSON.stringify(info.filaments) !== JSON.stringify(filaments)) p.push(`filaments are ${JSON.stringify(info.filaments)}, expected ${JSON.stringify(filaments)}`)
  const want = bedOutline(o.set.machine)
  const area = info.bed.printableArea
  const sameArea =
    Array.isArray(area) &&
    area.length === want.length &&
    area.every((pt, i) => Array.isArray(pt) && pt.length === 2 && near(Number(pt[0]), want[i]![0]) && near(Number(pt[1]), want[i]![1]))
  if (!sameArea) p.push(`the bed is ${JSON.stringify(area)}, but the machine preset's printable_area is ${JSON.stringify(want)}`)
  const height = settingNumber(o.set.machine, 'printable_height')
  if (height !== undefined && !(typeof info.bed.printableHeight === 'number' && near(info.bed.printableHeight, height))) {
    p.push(`the bed's printable height is ${JSON.stringify(info.bed.printableHeight)}, but the machine preset's printable_height is ${height}`)
  }
  return p
}

// ---------------------------------------------------------------- jobs

export interface JobSpec {
  command: Job['command']
  dir: string
  resources: string
  presets: PresetFiles
  overrides?: Overrides
  objects?: JobObject[]
  customGcodes?: CustomGcode[]
  bambu3mf?: boolean
}

/** A job for the model `<dir>/model.stl`, writing into `<dir>`. */
export function smokeJob(s: JobSpec): Job {
  return validateJob({
    format: 'sketch-orca-job/1',
    command: s.command,
    resources: s.resources,
    workDir: join(s.dir, 'work'),
    model: { path: join(s.dir, 'model.stl') },
    presets: { machine: s.presets.machine, process: s.presets.process, filaments: s.presets.filaments },
    overrides: s.overrides ?? {},
    objects: s.objects ?? [],
    customGcodes: s.customGcodes ?? [],
    arrange: true,
    orient: false,
    output: { gcode: join(s.dir, 'out.gcode'), bambu3mf: s.bambu3mf ? join(s.dir, 'out.gcode.3mf') : null, stats: true },
  })
}

// ---------------------------------------------------------------- running the engine

export interface EngineRun {
  code: number | null
  timedOut: boolean
  stdout: string
}

/** Runs the engine hidden, appending its output to `logPath`; stdout is also returned. */
export function runEngine(exe: string, args: string[], o: { cwd: string; env: Record<string, string>; logPath: string; timeoutMs: number }): Promise<EngineRun> {
  return new Promise((resolve, reject) => {
    const log = createWriteStream(o.logPath, { flags: 'a' })
    log.write(`$ ${exe} ${args.join(' ')}\n`)
    const child = spawn(exe, args, { cwd: o.cwd, env: o.env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
    const chunks: Buffer[] = []
    let size = 0
    child.stdout.on('data', (d: Buffer) => {
      log.write(d)
      if (size < 16 * 1024 * 1024) {
        chunks.push(d)
        size += d.length
      }
    })
    child.stderr.on('data', (d: Buffer) => log.write(d))
    let timedOut = false
    const timer = setTimeout(() => {
      timedOut = true
      child.kill()
    }, o.timeoutMs)
    child.on('error', (err) => {
      clearTimeout(timer)
      log.end()
      reject(err)
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      log.end(() => resolve({ code, timedOut, stdout: Buffer.concat(chunks).toString('utf8') }))
    })
  })
}

/**
 * Folders the engine's libraries live in before staging: the build folder and the dependency prefix
 * (GMP and MPFR too: on Windows the dependency build makes them from source into the prefix, patch
 * 0007; never Orca's prebuilt copies).
 */
export function engineLibraryDirs(work: WorkPaths, exe: string, platform: Platform): string[] {
  if (platform !== 'win32') return [dirname(exe), join(work.depsPrefix, 'lib')]
  return [dirname(exe), join(work.depsPrefix, 'bin'), join(work.depsPrefix, 'bin', 'occt')]
}

/** The environment the engine runs in: ours, with its library folders first on PATH (LD_LIBRARY_PATH / DYLD_LIBRARY_PATH elsewhere). */
export function engineEnv(base: NodeJS.ProcessEnv, libDirs: string[], platform: Platform): Record<string, string> {
  const env: Record<string, string> = {}
  for (const [k, v] of Object.entries(base)) if (v !== undefined) env[k] = v
  const key = platform === 'win32' ? (Object.keys(env).find((k) => k.toUpperCase() === 'PATH') ?? 'Path') : platform === 'darwin' ? 'DYLD_LIBRARY_PATH' : 'LD_LIBRARY_PATH'
  const sep = platform === 'win32' ? ';' : ':'
  env[key] = [...libDirs, ...(env[key] ? [env[key]!] : [])].join(sep)
  return env
}

interface CaseEnv {
  exe: string
  dir: string
  resources: string
  /** The Orca checkout the engine was built from (<work>/src), when at hand: config-def compares the obsolete keys with it. */
  orcaSource?: string
  lock: OrcaLock
  sets: Record<keyof typeof SMOKE_PRESETS, PresetSet>
  files: Record<keyof typeof SMOKE_PRESETS, PresetFiles>
  /** Runs the engine in the case folder; `extraEnv` adds to its environment (the crash test hook). */
  exec(args: string[], extraEnv?: Record<string, string>): Promise<EngineRun>
}

interface CaseOutcome {
  problems: string[]
  warnings: string[]
}

export interface SmokeCase {
  id: string
  title: string
  run(env: CaseEnv): Promise<CaseOutcome>
}

/**
 * Writes the model (unless there is none: a job about a missing file) and the job into `env.dir`,
 * runs the engine on it and reads the result. A job expected to be done must come back ok, with
 * the engine exiting as its result says; a refused job is checked by the caller (checkRefusal).
 */
async function runJob(
  env: CaseEnv,
  job: Job,
  model: Mesh | undefined,
  name: string,
  expect: 'done' | 'refused' = 'done',
  opts: {
    /** Added to the engine's environment (the crash test hook). */
    extraEnv?: Record<string, string>
    /** Write the job as it is, without job.ts's checks: for refusals the engine must make itself. */
    unchecked?: boolean
  } = {},
): Promise<{ result?: JobResult; raw?: Record<string, unknown>; code: number | null; problems: string[]; warnings: string[] }> {
  if (model) writeStl(job.model.path, model, name)
  // A refused job's work folder may not be absolute: it is not made (in the drive's root or here).
  if (isEngineAbsolute(job.workDir)) mkdirSync(job.workDir, { recursive: true })
  for (const out of [job.output.gcode, job.output.bambu3mf]) if (out) rmSync(out, { force: true })
  const jobPath = join(env.dir, 'job.json')
  const resultPath = join(env.dir, 'result.json')
  const progressPath = join(env.dir, 'progress.ndjson')
  rmSync(resultPath, { force: true })
  rmSync(progressPath, { force: true })
  if (opts.unchecked) writeFileSync(jobPath, JSON.stringify(job, null, 2) + '\n')
  else writeJob(jobPath, job)
  const run = await env.exec(['run', '--job', jobPath, '--out', resultPath, '--progress', progressPath], opts.extraEnv)
  if (run.timedOut) return { code: run.code, problems: ['the engine did not finish in time'], warnings: [] }
  if (!existsSync(resultPath)) return { code: run.code, problems: [`the engine exited with ${run.code} and wrote no result`], warnings: [] }
  let result: JobResult
  // The result as written, for the fields readResult fills in when they are absent (outputs).
  let raw: Record<string, unknown>
  try {
    raw = JSON.parse(readFileSync(resultPath, 'utf8')) as Record<string, unknown>
    result = readResult(resultPath)
  } catch (err) {
    return { code: run.code, problems: [`bad result: ${(err as Error).message}`], warnings: [] }
  }
  const problems: string[] = []
  const warnings: string[] = []
  if (expect === 'done') {
    if (!result.ok) problems.push(`the engine failed: ${result.code}: ${result.message}`)
    problems.push(...checkExitCode(run.code, result))
  }
  if (existsSync(progressPath)) {
    try {
      if (parseProgress(readFileSync(progressPath, 'utf8')).length === 0) warnings.push('the progress file is empty')
    } catch (err) {
      problems.push(`bad progress file: ${(err as Error).message}`)
    }
  } else {
    warnings.push('no progress file was written')
  }
  return { result, raw, code: run.code, problems, warnings }
}

/** Whether two absolute paths name the same file: separators and, on Windows, letter case do not matter. */
export function samePath(a: string, b: string, platform: Platform = currentPlatform()): boolean {
  if (a === '' || b === '') return a === b
  const norm = (p: string) => (platform === 'win32' ? resolve(p.replace(/\//g, '\\')).toLowerCase() : resolve(p))
  return norm(a) === norm(b)
}

function readGcode(result: JobResult, job: Job): { scan?: GcodeScan; problems: string[] } {
  if (!samePath(result.outputs.gcode, job.output.gcode)) return { problems: [`outputs.gcode is ${JSON.stringify(result.outputs.gcode)}, expected ${job.output.gcode}`] }
  if (!existsSync(job.output.gcode)) return { problems: [`no G-code at ${job.output.gcode}`] }
  return { scan: scanGcode(readFileSync(job.output.gcode, 'utf8')), problems: [] }
}

const cubeOverrides: Overrides = { ...PLAIN_OVERRIDES, layer_height: String(CUBE.layerHeight), initial_layer_print_height: String(CUBE.firstLayer) }

function cubeExpected(set: PresetSet): number[] {
  return expectedFixedLayers(CUBE.size, CUBE.firstLayer, CUBE.layerHeight, layerLimits(set.machine, CUBE.layerHeight).min)
}

/**
 * What the refused jobs use: a setting no Orca has, one with capital letters (Orca's keys are case
 * sensitive: it has layer_height), a key of the preset files written with a capital letter, a print
 * Z below the cube's 0.25 mm first layer, a cube wider than the bed, a work folder from the root of
 * the current drive (never made: runJob makes only absolute work folders).
 */
export const REFUSE = {
  unknownSetting: 'sketch_smoke_no_such_setting',
  capitalisedSetting: 'Layer_Height',
  presetFileKey: 'Inherits',
  belowFirstLayer: 0.1,
  tooBig: 400,
  driveRooted: '\\sketch-smoke-work',
} as const

export interface Refusal extends RefusalExpectation {
  id: string
  /** The job, writing into `dir`, and its model (none: no model file is written). */
  make(dir: string, resources: string, presets: PresetFiles): { job: Job; model?: Mesh }
  /** The job breaks a rule job.ts checks too: it is written as it is, to test the engine's own check. */
  unchecked?: boolean
  /** Fields the result must not have (a failed slice keeps no layers). */
  absent?: string[]
}

/** A 20 x 20 mm square with no height, as the 12 triangles of a box: a flat model. */
export function flatSquare(size = 20): Mesh {
  return { triangles: boxTriangles([0, 0, 0], [size, size, 0]) }
}

/**
 * One triangle, standing up, as an OBJ file: a model with no inside. OBJ, because Orca's STL loader
 * drops a facet with no neighbours (the file then fails to load as a whole), while its OBJ loader
 * keeps it, and that mesh crashed the print volume check before run.cpp refused it.
 */
export function loneTriangleObj(size = 20): string {
  return `v 0 0 0\nv ${size} 0 0\nv 0 0 ${size}\nf 1 2 3\n`
}

/** Jobs the engine must refuse; each is run in `<case>/<id>`. */
export const REFUSALS: Refusal[] = [
  {
    id: 'missing-model',
    code: 'file-not-found',
    exitCode: -3,
    mentions: 'missing.stl',
    make: (dir, resources, presets) => ({ job: { ...smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }), model: { path: join(dir, 'missing.stl') } } }),
  },
  {
    id: 'unknown-setting',
    code: 'setting-invalid',
    exitCode: -2,
    mentions: `${REFUSE.unknownSetting} is not a setting`,
    make: (dir, resources, presets) => ({ job: smokeJob({ command: 'slice', dir, resources, presets, overrides: { ...cubeOverrides, [REFUSE.unknownSetting]: '1' } }), model: cube(CUBE.size) }),
  },
  {
    // The job format takes the key (it has the shape of Orca's keys); Orca has no such setting.
    id: 'unknown-capitalised-setting',
    code: 'setting-invalid',
    exitCode: -2,
    mentions: `${REFUSE.capitalisedSetting} is not a setting`,
    make: (dir, resources, presets) => ({
      job: smokeJob({ command: 'slice', dir, resources, presets, overrides: { ...cubeOverrides, [REFUSE.capitalisedSetting]: String(CUBE.layerHeight) } }),
      model: cube(CUBE.size),
    }),
  },
  {
    // Orca's loader reads inherits in any letter case, so the job format refuses it in any case.
    id: 'preset-file-key',
    code: 'job-invalid',
    exitCode: -2,
    mentions: `"${REFUSE.presetFileKey}" belongs to the preset files`,
    unchecked: true,
    make: (dir, resources, presets) => ({
      job: { ...smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }), overrides: { ...cubeOverrides, [REFUSE.presetFileKey]: 'fdm_process_common' } },
      model: cube(CUBE.size),
    }),
  },
  {
    // An old name Orca renames for other values (override-keys applies one) and drops for this one.
    id: 'renamed-key-dropped-value',
    code: 'setting-invalid',
    exitCode: -2,
    mentions: `${VALUE_RENAME.key} is not a setting`,
    make: (dir, resources, presets) => ({
      job: smokeJob({ command: 'slice', dir, resources, presets, overrides: { ...cubeOverrides, [VALUE_RENAME.key]: VALUE_RENAME.dropped } }),
      model: cube(CUBE.size),
    }),
  },
  {
    id: 'pause-below-first-layer',
    code: 'custom-gcode-invalid',
    exitCode: -2,
    make: (dir, resources, presets) => ({
      job: smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides, customGcodes: [{ printZ: REFUSE.belowFirstLayer, type: 'pause' }] }),
      model: cube(CUBE.size),
    }),
  },
  {
    id: 'too-big-for-the-bed',
    code: 'arrange-failed',
    exitCode: -21,
    objectIndex: 0,
    mentions: 'model.stl',
    make: (dir, resources, presets) => ({ job: smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }), model: cube(REFUSE.tooBig) }),
  },
  {
    id: 'two-at-one-print-z',
    code: 'custom-gcode-invalid',
    exitCode: -2,
    mentions: 'two custom G-codes at print Z',
    unchecked: true,
    make: (dir, resources, presets) => ({
      job: {
        ...smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }),
        customGcodes: [
          { printZ: 5.05, type: 'pause' },
          { printZ: 5.05, type: 'custom', gcode: 'M117 smoke' },
        ],
      },
      model: cube(CUBE.size),
    }),
  },
  {
    id: 'flat-model',
    code: 'model-unreadable',
    exitCode: -6,
    objectIndex: 0,
    mentions: 'is flat',
    make: (dir, resources, presets) => ({ job: smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }), model: flatSquare() }),
  },
  {
    id: 'lone-triangle',
    code: 'model-unreadable',
    exitCode: -6,
    objectIndex: 0,
    mentions: 'has 1 triangle(s)',
    make: (dir, resources, presets) => {
      mkdirSync(dir, { recursive: true })
      const path = join(dir, 'model.obj')
      writeFileSync(path, loneTriangleObj())
      return { job: { ...smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }), model: { path } } }
    },
  },
  {
    id: 'profile-not-at-zero',
    code: 'layer-profile-invalid',
    exitCode: -2,
    objectIndex: 0,
    mentions: 'must start at z 0',
    unchecked: true,
    make: (dir, resources, presets) => ({
      job: { ...smokeJob({ command: 'layers', dir, resources, presets, overrides: cubeOverrides }), objects: [{ index: 0, overrides: {}, layerHeights: { mode: 'profile', profile: [0.5, 0.2, CUBE.size, 0.2] }, heightRanges: [] }] },
      model: cube(CUBE.size),
    }),
  },
  {
    id: 'layers-above-the-nozzle',
    code: 'layer-profile-invalid',
    exitCode: -2,
    objectIndex: 0,
    mentions: 'nozzle_diameter',
    make: (dir, resources, presets) => ({
      job: smokeJob({
        command: 'layers',
        dir,
        resources,
        presets,
        overrides: cubeOverrides,
        // Starts at the first layer's height, as a profile must; 0.5 mm is above the 0.4 mm nozzle.
        objects: [{ index: 0, overrides: {}, layerHeights: { mode: 'profile', profile: [0, CUBE.firstLayer, 5, 0.5, CUBE.size, 0.5] }, heightRanges: [] }],
      }),
      model: cube(CUBE.size),
    }),
  },
  {
    id: 'gcode-folder-unmakeable',
    code: 'output-failed',
    exitCode: -13,
    absent: ['layers', 'printZ', 'outputs'],
    make: (dir, resources, presets) => {
      // The G-code goes into a folder whose name a file already has.
      mkdirSync(dir, { recursive: true })
      writeFileSync(join(dir, 'blocker'), 'a file, not a folder\n')
      const job = smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides })
      return { job: { ...job, output: { ...job.output, gcode: join(dir, 'blocker', 'out.gcode') } }, model: cube(CUBE.size) }
    },
  },
  {
    // A path from the root of the current drive: absolute nowhere (on Windows it lacks the drive),
    // so the engine and job.ts both refuse it.
    id: 'drive-rooted-path',
    code: 'job-invalid',
    exitCode: -2,
    mentions: 'workDir must be an absolute path',
    unchecked: true,
    make: (dir, resources, presets) => ({
      job: { ...smokeJob({ command: 'slice', dir, resources, presets, overrides: cubeOverrides }), workDir: REFUSE.driveRooted },
      model: cube(CUBE.size),
    }),
  },
]

/** A volume past the largest flushing volume Orca computes (20000 mm³), which its int() would overflow. */
export const FLUSH = { outOfRange: '1e100' } as const

/**
 * A job of the flush-volumes and prime-tower cases: the cube, sliced with a copy of one of the
 * smoke's preset sets (changed by `presets`) and these settings, refused as `refused` says or done,
 * giving `config` in the G-code's config block.
 */
export interface SettingsJob {
  id: string
  set: keyof typeof SMOKE_PRESETS
  presets?: (set: PresetSet) => void
  overrides?: Overrides
  objects?: JobObject[]
  /** The filament changes and other custom G-codes, from the expected layers of the cube. */
  customGcodes?: (expected: number[]) => CustomGcode[]
  refused?: RefusalExpectation
  config?: Record<string, string>
}

/** Runs each job in `<case>/<id>`; the G-code of each job that was done is in `scans`. */
async function runSettingsJobs(env: CaseEnv, jobs: SettingsJob[]): Promise<CaseOutcome & { scans: Map<string, GcodeScan> }> {
  const problems: string[] = []
  const warnings: string[] = []
  const scans = new Map<string, GcodeScan>()
  for (const j of jobs) {
    const dir = join(env.dir, j.id)
    mkdirSync(dir, { recursive: true })
    const set = structuredClone(env.sets[j.set])
    j.presets?.(set)
    const files = writePresetSet(join(dir, 'presets'), set)
    const customGcodes = j.customGcodes?.(cubeExpected(set))
    const job = smokeJob({ command: 'slice', dir, resources: env.resources, presets: files, overrides: { ...cubeOverrides, ...j.overrides }, objects: j.objects, customGcodes })
    const r = await runJob({ ...env, dir }, job, cube(CUBE.size), 'cube 20 mm', j.refused ? 'refused' : 'done')
    const named = (s: string) => `${j.id}: ${s}`
    problems.push(...r.problems.map(named))
    warnings.push(...r.warnings.map(named))
    if (!r.result) continue
    if (j.refused) {
      problems.push(...checkRefusal(r.result, r.code, j.refused).map(named))
      continue
    }
    if (!r.result.ok) continue
    const g = readGcode(r.result, job)
    problems.push(...g.problems.map(named))
    if (!g.scan) continue
    scans.set(j.id, g.scan)
    for (const [key, want] of Object.entries(j.config ?? {})) problems.push(...checkConfigValue(g.scan, key, want).map(named))
  }
  return { problems, warnings, scans }
}

export const FLUSH_JOBS: SettingsJob[] = [
  {
    id: 'nozzle-volume-out-of-range',
    set: 'generic',
    presets: (set) => {
      set.machine.nozzle_volume = [FLUSH.outOfRange]
    },
    refused: { code: 'config-invalid', exitCode: -18, mentions: 'nozzle_volume' },
  },
  {
    // The same volume as an override: the overrides are in the settings the volumes are computed from.
    id: 'nozzle-volume-override',
    set: 'generic',
    overrides: { nozzle_volume: FLUSH.outOfRange },
    refused: { code: 'config-invalid', exitCode: -18, mentions: 'nozzle_volume' },
  },
  {
    // Filament 2 made a support filament by an override: Orca flushes 230 mm³ into a support filament
    // and at least 700 mm³ out of one (g_flush_volume_to_support, g_min_flush_volume_from_support).
    id: 'support-filament-override',
    set: 'generic2',
    overrides: { filament_is_support: '0,1' },
    config: { filament_is_support: '0,1', flush_volumes_matrix: '0,230,700,0' },
  },
]

/** How far prime-tower's bed-shifted job moves the bed: half the 300 mm smoke printer's, so 0,0 is its centre. */
export const BED_SHIFT = [-150, -150] as const

/** A filament change to filament 2 a quarter of the way up the cube: two filaments on the plate. */
const changeToFilament2 = (expected: number[]): CustomGcode[] => [{ printZ: Number(expected[PAUSE_LAYER_INDEX]!.toFixed(6)), type: 'filament', extruder: 2 }]

/** A prime tower width past every smoke printer's bed (300 mm). */
export const TOWER_TOO_WIDE = 400

/** How far, in mm, the tower's extrusions may pass an edge of the bed or the excluded area: half a 0.4 mm line. */
export const TOWER_TOLERANCE = 0.2

export const PRIME_TOWER_JOBS: SettingsJob[] = [
  {
    // Where the tower goes for two filaments on a bed at 0,0; the jobs below are compared with it.
    id: 'tower',
    set: 'generic2',
    customGcodes: changeToFilament2,
  },
  {
    // Support on filament 2 from the print, and an object that sets only raft_layers (0, as the
    // print has it): the object keeps the print's enable_support, so filament 2 is on the plate.
    id: 'support-raft-object',
    set: 'generic2',
    overrides: { enable_support: '1', support_filament: '2' },
    objects: [{ index: 0, overrides: { raft_layers: '0' }, layerHeights: { mode: 'fixed' }, heightRanges: [] }],
  },
  {
    // The same bed moved by BED_SHIFT, as a delta's is centred on 0,0: the tower moves with it.
    id: 'bed-shifted',
    set: 'generic2',
    presets: (set) => {
      set.machine.printable_area = bedOutline(set.machine).map(([x, y]) => `${x + BED_SHIFT[0]}x${y + BED_SHIFT[1]}`)
    },
    customGcodes: changeToFilament2,
  },
  {
    // The bed cut to the diamond between its edges' midpoints: the tower's default place is partly
    // off it, so the tower moves onto it.
    id: 'bed-diamond',
    set: 'generic2',
    presets: diamondBed,
    customGcodes: changeToFilament2,
  },
  {
    // The back half of the bed excluded (bed_exclude_area), where the default place is: the tower moves off it.
    id: 'bed-excluded',
    set: 'generic2',
    presets: (set) => {
      set.machine.bed_exclude_area = rectPoints(backHalf(bedOutline(set.machine)))
    },
    customGcodes: changeToFilament2,
  },
  {
    // A place the job sets itself (wipe_tower_x/y) off the diamond is refused, not moved.
    id: 'tower-set-off-bed',
    set: 'generic2',
    presets: diamondBed,
    overrides: { wipe_tower_x: '5', wipe_tower_y: '5' },
    customGcodes: changeToFilament2,
    refused: { code: 'config-invalid', exitCode: -18, mentions: 'wipe_tower_x' },
  },
  {
    // A tower wider than the bed fits nowhere.
    id: 'tower-too-wide',
    set: 'generic2',
    overrides: { prime_tower_width: String(TOWER_TOO_WIDE) },
    customGcodes: changeToFilament2,
    refused: { code: 'arrange-failed', exitCode: -21, mentions: 'prime tower' },
  },
]

/** prime-tower's diamond: the corners are the midpoints of the bed outline's bounding box edges. */
export function diamondOutline(outline: [number, number][]): [number, number][] {
  const xs = outline.map(([x]) => x)
  const ys = outline.map(([, y]) => y)
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)]
  const cx = (x0 + x1) / 2
  const cy = (y0 + y1) / 2
  return [[cx, y0], [x1, cy], [cx, y1], [x0, cy]]
}

function diamondBed(set: PresetSet): void {
  set.machine.printable_area = diamondOutline(bedOutline(set.machine)).map(([x, y]) => `${x}x${y}`)
}

/** prime-tower's excluded area: the back half of the bed outline's bounding box. */
export function backHalf(outline: [number, number][]): Rect {
  const xs = outline.map(([x]) => x)
  const ys = outline.map(([, y]) => y)
  const [y0, y1] = [Math.min(...ys), Math.max(...ys)]
  return { minX: Math.min(...xs), minY: (y0 + y1) / 2, maxX: Math.max(...xs), maxY: y1 }
}

/** A rectangle as bed_exclude_area gives one: its four corners, XxY. */
export function rectPoints(r: Rect): string[] {
  return [`${r.minX}x${r.minY}`, `${r.maxX}x${r.minY}`, `${r.maxX}x${r.maxY}`, `${r.minX}x${r.maxY}`]
}

/** How far a point is outside a polygon, 0 when it is inside or on it. */
export function distanceOutside(x: number, y: number, poly: [number, number][]): number {
  let inside = false
  let nearest = Infinity
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i]!
    const [xj, yj] = poly[j]!
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
    const dx = xj - xi
    const dy = yj - yi
    const t = dx === 0 && dy === 0 ? 0 : Math.max(0, Math.min(1, ((x - xi) * dx + (y - yi) * dy) / (dx * dx + dy * dy)))
    nearest = Math.min(nearest, Math.hypot(x - (xi + t * dx), y - (yi + t * dy)))
  }
  return inside ? 0 : nearest
}

/** The prime tower's extrusions from the first layer up (Orca tags them "Prime tower"). */
export function towerMoves(moves: Extrusion[]): Extrusion[] {
  return moves.filter((m) => m.layer >= 0 && m.role !== undefined && /tower/i.test(m.role))
}

/** Problems with where the tower printed: off `bed` by more than `tolerance` mm, or into `excluded`. */
export function towerPlacementProblems(moves: Extrusion[], o: { bed?: [number, number][]; excluded?: Rect; tolerance: number }): string[] {
  const tower = towerMoves(moves)
  if (tower.length === 0) return ['the G-code has no prime tower extrusions']
  const p: string[] = []
  if (o.bed) {
    const off = tower.find((m) => distanceOutside(m.x0, m.y0, o.bed!) > o.tolerance || distanceOutside(m.x1, m.y1, o.bed!) > o.tolerance)
    if (off) p.push(`the prime tower prints off the printable area (G-code line ${off.line + 1}, ${off.x1}, ${off.y1})`)
  }
  if (o.excluded) {
    const t = o.tolerance
    const inner: Rect = { minX: o.excluded.minX + t, minY: o.excluded.minY + t, maxX: o.excluded.maxX - t, maxY: o.excluded.maxY - t }
    const into = tower.find((m) => segmentHitsRect(m.x0, m.y0, m.x1, m.y1, inner))
    if (into) p.push(`the prime tower prints on the excluded area (G-code line ${into.line + 1}, ${into.x1}, ${into.y1})`)
  }
  return p
}

/**
 * imported-settings: the cube sliced with wall_loops PLAIN for the plate and WALLS for its object
 * (an object override), written to a .gcode.3mf whose object carries WALLS, then that project
 * sliced as a model with the plate at PLAIN and no object listed.
 */
export const IMPORTED_WALLS = { plain: 2, object: 6, tolerance: 0.03 } as const

/** The files of a run in file-collisions: the job file, --out, --progress, the model, output.gcode and the machine and process presets. */
export type RunFile = 'job' | 'result' | 'progress' | 'model' | 'gcode' | 'machine' | 'process'

/** The files of a run the engine reads. */
export const RUN_INPUTS: readonly RunFile[] = ['job', 'model', 'machine', 'process']

/** The command of collisions' invalid jobs: the job format has no such command. */
export const INVALID_COMMAND = 'carve'

/**
 * A run of file-collisions whose command line or job names one file twice, or whose command line is
 * wrong. `out` and `progress` say which file --out and --progress get, `alias` that one of them
 * names its file another way: a hard link to it, or a path through a folder and `..`. `jobName` is
 * the job file's name (job.json): result.json.tmp is the file the result is written through.
 * `invalidJob` gives the job an unknown command, `args` adds to the command line. The run is refused
 * with `code` in the result at `resultAt`, or (no `code`) writes nothing at all, --out included;
 * the files in `intact` keep their bytes.
 */
export interface Collision {
  id: string
  out: RunFile
  progress: RunFile
  alias?: { of: 'out' | 'progress'; how: 'hard-link' | 'dot-dot' }
  jobName?: string
  invalidJob?: boolean
  args?: string[]
  code?: string
  mentions?: string
  resultAt?: RunFile
  intact: RunFile[]
}

export const COLLISIONS: Collision[] = [
  // The result would replace the model and the progress file the job: nothing is written, the
  // failure result over the model included.
  { id: 'result-on-model-progress-on-job', out: 'model', progress: 'job', intact: ['job', 'model'] },
  // A job that is not valid names its model all the same: the result is not written over it.
  { id: 'invalid-job-result-on-model', out: 'model', progress: 'progress', invalidJob: true, intact: ['job', 'model'] },
  // Nor is the progress file opened over it.
  { id: 'invalid-job-progress-on-model', out: 'result', progress: 'model', invalidJob: true, code: 'invalid-params', mentions: 'model.path', resultAt: 'result', intact: ['job', 'model'] },
  // A wrong command line still gets its result...
  { id: 'command-line-wrong', out: 'result', progress: 'progress', args: ['--colour', 'red'], code: 'invalid-params', mentions: 'unknown option --colour', resultAt: 'result', intact: ['job', 'model'] },
  // ...but not over a file its job names.
  { id: 'command-line-result-on-model', out: 'model', progress: 'progress', args: ['--colour', 'red'], intact: ['job', 'model'] },
  // The preset files are inputs as the model is.
  { id: 'result-on-process', out: 'process', progress: 'progress', intact: ['job', 'model', 'process'] },
  { id: 'progress-on-machine', out: 'result', progress: 'machine', code: 'invalid-params', mentions: 'presets.machine', resultAt: 'result', intact: ['job', 'model', 'machine'] },
  // Another name for the model: a hard link, which the progress file would empty through.
  { id: 'progress-on-model-link', out: 'result', progress: 'model', alias: { of: 'progress', how: 'hard-link' }, code: 'invalid-params', mentions: 'model.path', resultAt: 'result', intact: ['job', 'model'] },
  // Another path to the model, through a folder and ..
  { id: 'result-on-model-dot-dot', out: 'model', progress: 'progress', alias: { of: 'out', how: 'dot-dot' }, intact: ['job', 'model'] },
  // The result is written through result.json.tmp, which is the job here.
  { id: 'result-tmp-on-job', out: 'result', progress: 'progress', jobName: 'result.json.tmp', intact: ['job', 'model'] },
  // The progress file would have been opened, emptying the job, before the job was read.
  { id: 'progress-on-job', out: 'result', progress: 'job', code: 'invalid-params', mentions: 'would replace the job', resultAt: 'result', intact: ['job'] },
  // The result would replace the job: nothing is written, not even the result.
  { id: 'result-on-job', out: 'job', progress: 'progress', intact: ['job'] },
  // The progress file would empty the model before Orca read it.
  { id: 'progress-on-model', out: 'result', progress: 'model', code: 'invalid-params', mentions: 'model.path', resultAt: 'result', intact: ['job', 'model'] },
  // The result would replace the model once the run was done.
  { id: 'result-on-model', out: 'model', progress: 'progress', intact: ['job', 'model'] },
  // The result would replace the G-code it reports as done.
  { id: 'result-on-gcode', out: 'gcode', progress: 'progress', code: 'invalid-params', mentions: 'output.gcode', resultAt: 'gcode', intact: ['job', 'model'] },
]

export const SMOKE_CASES: SmokeCase[] = [
  {
    id: 'version',
    title: 'version names the locked Orca release',
    async run(env) {
      const r = await env.exec(['version'])
      if (r.timedOut) return { problems: ['version did not finish in time'], warnings: [] }
      const problems = r.code === 0 ? checkVersion(r.stdout, env.lock) : [`version exited with ${r.code}`]
      return { problems, warnings: [] }
    },
  },
  {
    id: 'cube-fixed',
    title: `20 mm cube at 0.2 mm with a 0.25 mm first layer: exactly ${CUBE.layers} layers`,
    async run(env) {
      const expected = cubeExpected(env.sets.generic)
      const problems: string[] = []
      if (expected.length !== CUBE.layers || !near(expected[0]!, CUBE.firstLayer) || !near(expected[expected.length - 1]!, CUBE.lastTop)) {
        problems.push(`the expectation itself is off: ${expected.length} layers from ${expected[0]} to ${expected[expected.length - 1]}`)
      }
      const job = smokeJob({ command: 'slice', dir: env.dir, resources: env.resources, presets: env.files.generic, overrides: cubeOverrides })
      const r = await runJob(env, job, cube(CUBE.size), 'cube 20 mm')
      problems.push(...r.problems)
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      problems.push(...checkFixedLayers(r.result, expected))
      const g = readGcode(r.result, job)
      problems.push(...g.problems)
      if (g.scan) {
        problems.push(...checkGcodeLayers(g.scan, r.result.printZ))
        if (g.scan.pauses.length > 0) problems.push(`the G-code has ${g.scan.pauses.length} pauses; none were asked for`)
        if (!r.result.stats) problems.push('the result has no stats; the job asked for them')
        else problems.push(...checkFeatureLengths(r.result.stats.features, featureLengths(extrusions(g.scan))))
      }
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'sphere-adaptive',
    title: 'adaptive layer height on a sphere varies within the printer limits',
    async run(env) {
      const layerHeight = settingNumber(env.sets.generic.process, 'layer_height') ?? 0.2
      const firstLayer = settingNumber(env.sets.generic.process, 'initial_layer_print_height') ?? layerHeight
      const { min: minLayer, max: maxLayer } = layerLimits(env.sets.generic.machine, layerHeight)
      const job = smokeJob({
        command: 'layers',
        dir: env.dir,
        resources: env.resources,
        presets: env.files.generic,
        overrides: PLAIN_OVERRIDES,
        objects: [{ index: 0, overrides: {}, layerHeights: { mode: 'adaptive', quality: SPHERE.quality }, heightRanges: [] }],
      })
      const r = await runJob(env, job, uvSphere(SPHERE.radius, 96, 48), 'sphere r10')
      const problems = [...r.problems]
      if (r.result?.ok) problems.push(...checkAdaptiveLayers(r.result, { firstLayer, minLayer, maxLayer, objectHeight: 2 * SPHERE.radius }))
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'pause-at-layer',
    title: 'a pause at a print Z appears once, before that layer, as the machine pause G-code',
    async run(env) {
      const expected = cubeExpected(env.sets.generic)
      const printZ = Number(expected[PAUSE_LAYER_INDEX]!.toFixed(6))
      const job = smokeJob({
        command: 'slice',
        dir: env.dir,
        resources: env.resources,
        presets: env.files.generic,
        overrides: cubeOverrides,
        customGcodes: [{ printZ, type: 'pause', note: 'smoke: pause' }],
      })
      const r = await runJob(env, job, cube(CUBE.size), 'cube 20 mm')
      const problems = [...r.problems]
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      problems.push(...checkFixedLayers(r.result, expected))
      const g = readGcode(r.result, job)
      problems.push(...g.problems)
      if (g.scan) problems.push(...checkPause(g.scan, printZ, String(env.sets.generic.machine.machine_pause_gcode ?? '')))
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'layers-only',
    title: '"layers" returns the layer ranges and writes no G-code',
    async run(env) {
      const expected = cubeExpected(env.sets.generic)
      const job = smokeJob({ command: 'layers', dir: env.dir, resources: env.resources, presets: env.files.generic, overrides: cubeOverrides })
      const r = await runJob(env, job, cube(CUBE.size), 'cube 20 mm')
      const problems = [...r.problems]
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      problems.push(...checkFixedLayers(r.result, expected))
      if (r.raw && 'outputs' in r.raw) problems.push(`the result names outputs (${JSON.stringify(r.raw.outputs)}); layers writes none`)
      if (existsSync(job.output.gcode)) problems.push(`G-code was written to ${job.output.gcode}`)
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'config-def',
    title: 'config-def lists the settings with types and enum values, and the obsolete keys Orca drops or renames, none of them a setting',
    async run(env) {
      const out = join(env.dir, 'config-def.json')
      rmSync(out, { force: true })
      const r = await env.exec(['config-def', '--out', out])
      if (r.timedOut) return { problems: ['config-def did not finish in time'], warnings: [] }
      if (r.code !== 0) return { problems: [`config-def exited with ${r.code}`], warnings: [] }
      if (!existsSync(out)) return { problems: [`config-def wrote nothing to ${out}`], warnings: [] }
      try {
        const root = JSON.parse(readFileSync(out, 'utf8')) as unknown
        const defs = parseConfigDef(root)
        const legacy = env.orcaSource ? legacyKeysFromSource(readFileSync(join(env.orcaSource, 'src', 'libslic3r', 'PrintConfig.cpp'), 'utf8')) : undefined
        return {
          problems: [...checkConfigDef(defs), ...checkObsoleteKeys(root, defs, legacy)],
          warnings: legacy ? [] : ["no Orca source at hand: the obsolete keys were not compared with handle_legacy's"],
        }
      } catch (err) {
        return { problems: [(err as Error).message], warnings: [] }
      }
    },
  },
  {
    id: 'bambu-3mf',
    title: 'an X1 Carbon job writes a .gcode.3mf holding Metadata/plate_1.gcode',
    async run(env) {
      const job = smokeJob({ command: 'slice', dir: env.dir, resources: env.resources, presets: env.files.bambu, bambu3mf: true })
      const r = await runJob(env, job, cube(CUBE.size), 'cube 20 mm')
      const problems = [...r.problems]
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      if (!samePath(r.result.outputs.bambu3mf, job.output.bambu3mf ?? '')) problems.push(`outputs.bambu3mf is ${JSON.stringify(r.result.outputs.bambu3mf)}, expected ${job.output.bambu3mf}`)
      if (!job.output.bambu3mf || !existsSync(job.output.bambu3mf)) return { problems: [...problems, 'no .gcode.3mf was written'], warnings: r.warnings }
      try {
        problems.push(...checkBambu3mf(readFileSync(job.output.bambu3mf), r.result.printZ))
      } catch (err) {
        problems.push(`the .gcode.3mf cannot be read: ${(err as Error).message}`)
      }
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'pocket-pause',
    title: `a box with a closed pocket: the pause goes before the first layer covering it (Z ${POCKET.coverZ})`,
    async run(env) {
      const overrides: Overrides = { ...PLAIN_OVERRIDES, layer_height: String(POCKET.layerHeight), initial_layer_print_height: String(POCKET.firstLayer) }
      const model = boxWithPocket(POCKET.size, { min: POCKET.min, max: POCKET.max })
      const layersDir = join(env.dir, 'layers')
      mkdirSync(layersDir, { recursive: true })
      const plan = await runJob({ ...env, dir: layersDir }, smokeJob({ command: 'layers', dir: layersDir, resources: env.resources, presets: env.files.generic, overrides }), model, 'box with pocket')
      if (!plan.result?.ok) return { problems: plan.problems, warnings: plan.warnings }
      const problems = [...plan.problems, ...checkLayerList(plan.result)]
      const ranges = plan.result.layers[0]?.ranges ?? []
      const cover = firstCoveringLayer(ranges, POCKET.max[2])
      if (cover !== POCKET.coverIndex || !near(ranges[cover]![1], POCKET.coverZ)) {
        problems.push(`the first layer covering the pocket is ${cover === undefined ? 'none' : `layer ${cover + 1} at Z ${ranges[cover]![1]}`}, expected layer ${POCKET.coverIndex + 1} at Z ${POCKET.coverZ}`)
        return { problems, warnings: plan.warnings }
      }
      const printZ = Number(ranges[cover]![1].toFixed(6))
      const job = smokeJob({ command: 'slice', dir: env.dir, resources: env.resources, presets: env.files.generic, overrides, customGcodes: [{ printZ, type: 'pause', note: 'Insert the magnet' }] })
      const r = await runJob(env, job, model, 'box with pocket')
      problems.push(...r.problems)
      if (!r.result?.ok) return { problems, warnings: [...plan.warnings, ...r.warnings] }
      const g = readGcode(r.result, job)
      problems.push(...g.problems)
      if (g.scan) {
        problems.push(...checkPause(g.scan, printZ, String(env.sets.generic.machine.machine_pause_gcode ?? '')))
        const half: [number, number] = [(POCKET.max[0] - POCKET.min[0]) / 2, (POCKET.max[1] - POCKET.min[1]) / 2]
        problems.push(...checkPocketCover(g.scan, half, 0.6))
      }
      return { problems, warnings: [...plan.warnings, ...r.warnings] }
    },
  },
  {
    id: 'info',
    title: '"info" returns the objects, the bed and the preset names, and slices nothing',
    async run(env) {
      const model = cube(CUBE.size)
      const job = smokeJob({ command: 'info', dir: env.dir, resources: env.resources, presets: env.files.generic })
      const r = await runJob(env, job, model, 'cube 20 mm')
      const problems = [...r.problems]
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      problems.push(...checkInfo(r.result, { model, fileName: basename(job.model.path), set: env.sets.generic }))
      if (existsSync(job.output.gcode)) problems.push(`G-code was written to ${job.output.gcode}`)
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'filaments',
    title: 'two filaments: a filament change at 5.05 prints with T1; a change on the first layer and unused support filament slice too',
    async run(env) {
      const problems: string[] = []
      const warnings: string[] = []
      const expected = cubeExpected(env.sets.generic2)
      const printZ = Number(expected[PAUSE_LAYER_INDEX]!.toFixed(6))
      const jobs: { id: string; overrides?: Overrides; customGcodes?: CustomGcode[]; check(scan: GcodeScan, result: JobResult): string[] }[] = [
        {
          // No filament_colour override: the engine sizes the colours to the filaments itself.
          id: 'change-at-5',
          customGcodes: [{ printZ, type: 'filament', extruder: 2 }],
          check: (scan, result) => {
            const p = checkFixedLayers(result, expected)
            const change = toolChanges(scan)
            if (!change.includes('T1')) p.push(`the G-code never selects T1 (tool commands: ${change.join(', ') || 'none'})`)
            return p
          },
        },
        {
          // The whole print is on filament 2 from the first layer: a prime tower is planned, none is made.
          id: 'change-on-first-layer',
          customGcodes: [{ printZ: CUBE.firstLayer, type: 'filament', extruder: 2 }],
          check: (scan) => (toolChanges(scan).includes('T1') ? [] : ['the G-code never selects T1']),
        },
        {
          // Support on filament 2 where the cube needs none: filament 2 is never used.
          id: 'support-filament-unused',
          overrides: { enable_support: '1', support_filament: '2' },
          check: () => [],
        },
      ]
      for (const j of jobs) {
        const dir = join(env.dir, j.id)
        mkdirSync(dir, { recursive: true })
        const job = smokeJob({ command: 'slice', dir, resources: env.resources, presets: env.files.generic2, overrides: { ...cubeOverrides, ...j.overrides }, customGcodes: j.customGcodes })
        const r = await runJob({ ...env, dir }, job, cube(CUBE.size), 'cube 20 mm')
        const named = (s: string) => `${j.id}: ${s}`
        problems.push(...r.problems.map(named))
        warnings.push(...r.warnings.map(named))
        if (!r.result?.ok) continue
        const g = readGcode(r.result, job)
        problems.push(...g.problems.map(named))
        if (g.scan) problems.push(...j.check(g.scan, r.result).map(named))
      }
      return { problems, warnings }
    },
  },
  {
    id: 'z-offset',
    title: `the cube with a printer z_offset of ${Z_OFFSETS.join(' and of ')} mm: its layers and layer tags at print Z, every extrusion at print Z plus the offset`,
    async run(env) {
      const problems: string[] = []
      const warnings: string[] = []
      const expected = cubeExpected(env.sets.generic)
      if (Z_OFFSETS.some((o) => o === (settingNumber(env.sets.generic.machine, 'z_offset') ?? 0))) problems.push('the expectation itself is off: the printer preset already has one of the offsets')
      for (const offset of Z_OFFSETS) {
        const id = offset < 0 ? `lowered-${-offset}` : `raised-${offset}`
        const dir = join(env.dir, id)
        mkdirSync(dir, { recursive: true })
        // z_offset is a printer setting; the job's overrides set it as a client would for a printer.
        const job = smokeJob({ command: 'slice', dir, resources: env.resources, presets: env.files.generic, overrides: { ...cubeOverrides, z_offset: String(offset) } })
        const r = await runJob({ ...env, dir }, job, cube(CUBE.size), 'cube 20 mm')
        const named = (s: string) => `${id}: ${s}`
        problems.push(...r.problems.map(named))
        warnings.push(...r.warnings.map(named))
        if (!r.result?.ok) continue
        problems.push(...checkFixedLayers(r.result, expected).map(named))
        const g = readGcode(r.result, job)
        problems.push(...g.problems.map(named))
        if (!g.scan) continue
        problems.push(...checkGcodeLayers(g.scan, r.result.printZ).map(named))
        problems.push(...checkExtrusionHeights(g.scan, r.result.printZ, offset).map(named))
        problems.push(...checkConfigValue(g.scan, 'z_offset', offset).map(named))
      }
      return { problems, warnings }
    },
  },
  {
    id: 'override-keys',
    title: `overrides with capital letters (${CAPITAL_KEY.key}, a filament setting) and with an old name Orca renames for its value (${VALUE_RENAME.key} ${VALUE_RENAME.renamed}, read as ${VALUE_RENAME.to} = ${VALUE_RENAME.as}) are applied: the cube slices and the G-code's config gives their values`,
    async run(env) {
      const expected = cubeExpected(env.sets.generic)
      const problems: string[] = []
      const own = settingNumber(env.sets.generic.filaments[0]!, CAPITAL_KEY.key)
      if (own !== undefined && own === Number(CAPITAL_KEY.value)) problems.push(`the expectation itself is off: the filament preset already has ${CAPITAL_KEY.key} = ${own}`)
      const before = settingNumber(env.sets.generic.process, VALUE_RENAME.to)
      if (before !== undefined && before === Number(VALUE_RENAME.as)) problems.push(`the expectation itself is off: the process preset already has ${VALUE_RENAME.to} = ${before}`)
      const overrides = { ...cubeOverrides, [CAPITAL_KEY.key]: CAPITAL_KEY.value, [VALUE_RENAME.key]: VALUE_RENAME.renamed }
      const job = smokeJob({ command: 'slice', dir: env.dir, resources: env.resources, presets: env.files.generic, overrides })
      const r = await runJob(env, job, cube(CUBE.size), 'cube 20 mm')
      problems.push(...r.problems)
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      problems.push(...checkFixedLayers(r.result, expected))
      const g = readGcode(r.result, job)
      problems.push(...g.problems)
      if (g.scan) problems.push(...checkConfigValue(g.scan, CAPITAL_KEY.key, CAPITAL_KEY.value), ...checkConfigValue(g.scan, VALUE_RENAME.to, VALUE_RENAME.as))
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'preset-metadata',
    title: `presets that carry the lists the engine fills in itself (${PRESET_METADATA.join(', ')}), as a project export may, and a process that gives a filament setting as an empty list, slice as without them`,
    async run(env) {
      const set = structuredClone(env.sets.generic)
      // Shorter and longer than the engine's own lists, which hold one entry per preset: it writes them by index.
      set.machine.inherits_group = []
      set.machine.different_settings_to_system = []
      set.process.inherits_group = [PRESET_METADATA_MARK]
      set.process.compatible_machine_expression_group = [PRESET_METADATA_MARK]
      set.process.print_compatible_printers = [PRESET_METADATA_MARK]
      set.filaments[0]!.inherits_group = [PRESET_METADATA_MARK, PRESET_METADATA_MARK, PRESET_METADATA_MARK, PRESET_METADATA_MARK]
      // The filament merge writes each filament's value into this list by index.
      set.process.filament_type = []
      const files = writePresetSet(join(env.dir, 'presets'), set)
      const job = smokeJob({ command: 'slice', dir: env.dir, resources: env.resources, presets: files, overrides: cubeOverrides })
      const r = await runJob(env, job, cube(CUBE.size), 'cube 20 mm')
      const problems = [...r.problems]
      if (!r.result?.ok) return { problems, warnings: r.warnings }
      problems.push(...checkFixedLayers(r.result, cubeExpected(env.sets.generic)))
      const g = readGcode(r.result, job)
      problems.push(...g.problems)
      if (!g.scan) return { problems, warnings: r.warnings }
      const type = settingList(env.sets.generic.filaments[0]!, 'filament_type')[0]
      if (type === undefined) problems.push('the expectation itself is off: the filament preset has no filament_type')
      else problems.push(...checkConfigValue(g.scan, 'filament_type', type))
      for (const key of PRESET_METADATA) {
        const got = configValue(g.scan.lines, key)
        if (got?.includes(PRESET_METADATA_MARK)) problems.push(`the G-code's config block gives ${key} = ${JSON.stringify(got)}, a preset file's list`)
      }
      return { problems, warnings: r.warnings }
    },
  },
  {
    id: 'flush-volumes',
    title: `the flushing volumes between filaments: a nozzle_volume of ${FLUSH.outOfRange} mm³, in the preset or as an override, is refused (config-invalid), not converted past an int; filament_is_support given as an override counts`,
    async run(env) {
      const { problems, warnings } = await runSettingsJobs(env, FLUSH_JOBS)
      return { problems, warnings }
    },
  },
  {
    id: 'prime-tower',
    title: `the prime tower: an object that sets only raft_layers keeps the print's support, whose filament puts a tower on the plate; on a bed moved by ${BED_SHIFT.join(', ')} the tower moves with it; it prints on a diamond bed and off an excluded half, a place set off the bed is refused, and a tower wider than the bed fits nowhere`,
    async run(env) {
      const { problems, warnings, scans } = await runSettingsJobs(env, PRIME_TOWER_JOBS)
      const at = (id: string) => {
        const scan = scans.get(id)
        return scan ? { x: configValue(scan.lines, 'wipe_tower_x'), y: configValue(scan.lines, 'wipe_tower_y') } : undefined
      }
      const tower = at('tower')
      // A process setting; Orca's default is 15, 220. The tower placed for two filaments must be elsewhere.
      const preset = { x: settingList(env.sets.generic2.process, 'wipe_tower_x')[0] ?? '15', y: settingList(env.sets.generic2.process, 'wipe_tower_y')[0] ?? '220' }
      if (!tower) problems.push('the expectation itself is off: the tower job gave no G-code')
      else if (Number(tower.x) === Number(preset.x) && Number(tower.y) === Number(preset.y))
        problems.push(`the expectation itself is off: the placed tower is at the presets' own ${preset.x}, ${preset.y}`)
      // Support on filament 2 counts 2 filaments, so the tower is placed as for a filament change.
      const raft = at('support-raft-object')
      if (tower && raft && (Number(raft.x) !== Number(tower.x) || Number(raft.y) !== Number(tower.y)))
        problems.push(`support-raft-object: the tower is at ${raft.x}, ${raft.y}, not placed as for two filaments (${tower.x}, ${tower.y})`)
      // The tower keeps its place on the bed: its corner moves by the bed's shift.
      const shifted = at('bed-shifted')
      if (tower && shifted && (Number(shifted.x) !== Number(tower.x) + BED_SHIFT[0] || Number(shifted.y) !== Number(tower.y) + BED_SHIFT[1]))
        problems.push(`bed-shifted: the tower is at ${shifted.x}, ${shifted.y}; on the bed moved by ${BED_SHIFT.join(', ')} it belongs at ${Number(tower.x) + BED_SHIFT[0]}, ${Number(tower.y) + BED_SHIFT[1]}`)
      // The tower printed on the diamond and off the excluded half, where its default place is not.
      const outline = bedOutline(env.sets.generic2.machine)
      const diamond = diamondOutline(outline)
      const excluded = backHalf(outline)
      const moves = (id: string) => {
        const scan = scans.get(id)
        return scan ? extrusions(scan) : undefined
      }
      const plain = moves('tower')
      if (plain && towerMoves(plain).length === 0) problems.push('the expectation itself is off: the tower job printed no prime tower')
      else if (plain) {
        if (towerPlacementProblems(plain, { bed: diamond, tolerance: TOWER_TOLERANCE }).length === 0) problems.push('the expectation itself is off: on the full bed the tower already prints inside the diamond')
        if (towerPlacementProblems(plain, { excluded, tolerance: TOWER_TOLERANCE }).length === 0) problems.push('the expectation itself is off: on the full bed the tower already prints off the back half')
      }
      for (const [id, o] of [['bed-diamond', { bed: diamond }], ['bed-excluded', { bed: outline, excluded }]] as const) {
        const m = moves(id)
        if (m) problems.push(...towerPlacementProblems(m, { ...o, tolerance: TOWER_TOLERANCE }).map((s) => `${id}: ${s}`))
      }
      return { problems, warnings }
    },
  },
  {
    id: 'imported-settings',
    title: `a 3MF project whose object has wall_loops ${IMPORTED_WALLS.object} slices, unlisted, with the plate's ${IMPORTED_WALLS.plain}: its inner walls are the plain cube's`,
    async run(env) {
      const problems: string[] = []
      const warnings: string[] = []
      const plate: Overrides = { ...cubeOverrides, wall_loops: String(IMPORTED_WALLS.plain) }
      const innerWall = async (id: string, setUp: (dir: string) => { job: Job; model?: Mesh }) => {
        const dir = join(env.dir, id)
        rmSync(dir, { recursive: true, force: true })
        mkdirSync(dir, { recursive: true })
        const { job, model } = setUp(dir)
        const r = await runJob({ ...env, dir }, job, model, 'cube 20 mm')
        const named = (s: string) => `${id}: ${s}`
        problems.push(...r.problems.map(named))
        warnings.push(...r.warnings.map(named))
        const mm = r.result?.ok ? r.result.stats?.features['Inner wall']?.mm : undefined
        if (r.result?.ok && mm === undefined) problems.push(named('the result has no Inner wall length'))
        return { job, mm }
      }
      // The object's own walls, written into the project.
      const exported = await innerWall('export', (dir) => ({
        job: smokeJob({
          command: 'slice',
          dir,
          resources: env.resources,
          presets: env.files.generic,
          overrides: plate,
          objects: [{ index: 0, overrides: { wall_loops: String(IMPORTED_WALLS.object) }, layerHeights: { mode: 'fixed' }, heightRanges: [] }],
          bambu3mf: true,
        }),
        model: cube(CUBE.size),
      }))
      const plain = await innerWall('plain', (dir) => ({ job: smokeJob({ command: 'slice', dir, resources: env.resources, presets: env.files.generic, overrides: plate }), model: cube(CUBE.size) }))
      const project = exported.job.output.bambu3mf
      if (!project || !existsSync(project)) return { problems: [...problems, 'export: no .gcode.3mf was written'], warnings }
      const imported = await innerWall('from-3mf', (dir) => {
        const path = join(dir, 'model.3mf')
        writeFileSync(path, readFileSync(project))
        return { job: { ...smokeJob({ command: 'slice', dir, resources: env.resources, presets: env.files.generic, overrides: plate }), model: { path } } }
      })
      if (exported.mm === undefined || plain.mm === undefined || imported.mm === undefined) return { problems, warnings }
      // The object's walls took effect where the job set them: more inner wall than the plain cube.
      if (!(exported.mm > plain.mm * 1.5)) problems.push(`the expectation itself is off: the object's ${IMPORTED_WALLS.object} walls gave ${exported.mm} mm of inner wall, the plain cube ${plain.mm} mm`)
      if (Math.abs(imported.mm - plain.mm) > plain.mm * IMPORTED_WALLS.tolerance)
        problems.push(`from-3mf: ${imported.mm} mm of inner wall, the plain cube ${plain.mm} mm: the project's wall_loops ${IMPORTED_WALLS.object} was used`)
      return { problems, warnings }
    },
  },
  {
    id: 'file-collisions',
    title: `a run whose files overlap (${COLLISIONS.map((c) => c.id).join(', ')}) is refused before it writes over one: invalid-params, or nothing written when the result itself would replace an input`,
    async run(env) {
      const problems: string[] = []
      const warnings: string[] = []
      for (const c of COLLISIONS) {
        const dir = join(env.dir, c.id)
        rmSync(dir, { recursive: true, force: true })
        mkdirSync(dir, { recursive: true })
        // The case's own copy of the presets, as some runs name them twice.
        const presets = writePresetSet(join(dir, 'presets'), env.sets.generic)
        const job = smokeJob({ command: 'slice', dir, resources: env.resources, presets, overrides: cubeOverrides })
        const files: Record<RunFile, string> = {
          job: join(dir, c.jobName ?? 'job.json'),
          result: join(dir, 'result.json'),
          progress: join(dir, 'progress.ndjson'),
          model: job.model.path,
          gcode: job.output.gcode,
          machine: presets.machine,
          process: presets.process,
        }
        writeStl(job.model.path, cube(CUBE.size), 'cube 20 mm')
        mkdirSync(job.workDir, { recursive: true })
        if (c.invalidJob) writeFileSync(files.job, JSON.stringify({ ...job, command: INVALID_COMMAND }, null, 2) + '\n')
        else writeJob(files.job, job)
        const paths = { out: files[c.out], progress: files[c.progress] }
        if (c.alias) {
          const target = paths[c.alias.of]
          if (c.alias.how === 'hard-link') {
            paths[c.alias.of] = join(dir, `link-${basename(target)}`)
            linkSync(target, paths[c.alias.of])
          } else {
            mkdirSync(join(dir, 'sub'), { recursive: true })
            paths[c.alias.of] = [dirname(target), 'sub', '..', basename(target)].join(sep)
          }
        }
        const before = new Map(c.intact.map((f) => [f, readFileSync(files[f])] as const))
        const outBefore = existsSync(paths.out) ? readFileSync(paths.out) : undefined
        const run = await env.exec(['run', '--job', files.job, '--out', paths.out, '--progress', paths.progress, ...(c.args ?? [])])
        const named = (s: string) => `${c.id}: ${s}`
        if (run.timedOut) {
          problems.push(named('the engine did not finish in time'))
          continue
        }
        for (const [f, bytes] of before) if (!existsSync(files[f]) || !readFileSync(files[f]).equals(bytes)) problems.push(named(`the ${f} file ${files[f]} was changed`))
        if (run.code === null || exitByte(run.code) !== exitByte(-2)) problems.push(named(`the engine exited with ${run.code}, expected -2`))
        if (c.code === undefined) {
          for (const f of ['result', 'progress', 'gcode'] as const) if (existsSync(files[f])) problems.push(named(`${files[f]} was written; nothing should be`))
          const outAfter = existsSync(paths.out) ? readFileSync(paths.out) : undefined
          if (outBefore === undefined ? outAfter !== undefined : outAfter === undefined || !outAfter.equals(outBefore)) problems.push(named(`--out ${paths.out} was written; nothing should be`))
          continue
        }
        if (c.out !== 'gcode' && existsSync(files.gcode)) problems.push(named(`the G-code ${files.gcode} was written`))
        const at = files[c.resultAt ?? 'result']
        let result: JobResult
        try {
          result = readResult(at)
        } catch (err) {
          problems.push(named(`no readable result at ${at}: ${(err as Error).message}`))
          continue
        }
        problems.push(...checkRefusal(result, run.code, { code: c.code, exitCode: -2, mentions: c.mentions }).map(named))
      }
      return { problems, warnings }
    },
  },
  {
    id: 'crash-result',
    title: 'a crash in Orca\'s code still writes the result: internal-error, exit code 255, no outputs',
    async run(env) {
      const problems: string[] = []
      const warnings: string[] = []
      for (const stage of ['slice', 'slice:abort']) {
        const id = stage.replace(':', '-')
        const dir = join(env.dir, id)
        mkdirSync(dir, { recursive: true })
        const job = smokeJob({ command: 'slice', dir, resources: env.resources, presets: env.files.generic, overrides: cubeOverrides })
        const r = await runJob({ ...env, dir }, job, cube(CUBE.size), 'cube 20 mm', 'refused', { extraEnv: { SKETCH_ORCA_TEST_CRASH: stage } })
        const named = (s: string) => `${id}: ${s}`
        problems.push(...r.problems.map(named))
        warnings.push(...r.warnings.map(named))
        if (!r.result) continue
        problems.push(...checkRefusal(r.result, r.code, { code: 'internal-error', exitCode: -1, mentions: 'crashed' }).map(named))
        if (r.code === null || exitByte(r.code) !== 255) problems.push(named(`the engine exited with ${r.code}, expected 255`))
        if (r.raw && 'outputs' in r.raw) problems.push(named('the result names outputs'))
      }
      return { problems, warnings }
    },
  },
  {
    id: 'refusals',
    title: 'refused jobs get their named error, exit code and object, and write no G-code',
    async run(env) {
      const problems: string[] = []
      const warnings: string[] = []
      const bed = bedOutline(env.sets.generic.machine)
      const narrowest = Math.min(Math.max(...bed.map((pt) => pt[0])) - Math.min(...bed.map((pt) => pt[0])), Math.max(...bed.map((pt) => pt[1])) - Math.min(...bed.map((pt) => pt[1])))
      if (!(REFUSE.tooBig > narrowest)) problems.push(`the expectation itself is off: a ${REFUSE.tooBig} mm cube fits on the ${narrowest} mm bed`)
      for (const want of REFUSALS) {
        const dir = join(env.dir, want.id)
        mkdirSync(dir, { recursive: true })
        const { job, model } = want.make(dir, env.resources, env.files.generic)
        const r = await runJob({ ...env, dir }, job, model, want.id, 'refused', { unchecked: want.unchecked })
        const named = (s: string) => `${want.id}: ${s}`
        problems.push(...r.problems.map(named))
        warnings.push(...r.warnings.map(named))
        if (r.result) problems.push(...checkRefusal(r.result, r.code, want).map(named))
        for (const key of want.absent ?? []) if (r.raw && key in r.raw) problems.push(named(`the result has ${key}; a failed job has none`))
        for (const out of [job.output.gcode, job.output.bambu3mf]) if (out && existsSync(out)) problems.push(named(`${out} was written`))
      }
      return { problems, warnings }
    },
  },
]

// ---------------------------------------------------------------- the step

export interface SmokeReport {
  format: 'sketch-orca-smoke/1'
  smokeVersion: number
  exeSha256: string
  /** What the run was for (smokeFingerprint). */
  fingerprint: string
  orcaCommit: string
  passed: boolean
  cases: { id: string; title: string; ok: boolean; problems: string[]; warnings: string[] }[]
}

/** The cases `--case a,b` (or `--case=a,b`) names, in the order of SMOKE_CASES; all of them without it. */
export function selectedCases(args: string[]): SmokeCase[] {
  const i = args.indexOf('--case')
  const raw = i >= 0 ? args[i + 1] : args.find((a) => a.startsWith('--case='))?.slice(7)
  if (raw === undefined) return SMOKE_CASES
  const ids = raw.split(',').map((s) => s.trim()).filter(Boolean)
  const unknown = ids.filter((id) => !SMOKE_CASES.some((c) => c.id === id))
  if (unknown.length > 0) throw new StepError(`unknown smoke case ${unknown.join(', ')}; the cases are ${SMOKE_CASES.map((c) => c.id).join(', ')}`)
  return SMOKE_CASES.filter((c) => ids.includes(c.id))
}

/** Runs the cases against the built engine. Skips when they already passed for this engine build, lock and case version. */
export type SmokePresetSets = Record<keyof typeof SMOKE_PRESETS, PresetSet>

/** The presets the cases slice with (SMOKE_PRESETS), flattened from a resources folder's profiles and checked. */
export function smokePresetSets(profilesDir: string): SmokePresetSets {
  const tree = loadProfileTree(profilesDir, { vendors: [...new Set(Object.values(SMOKE_PRESETS).map((p) => p.vendor))] })
  const sets = { generic: presetSet(tree, SMOKE_PRESETS.generic), generic2: presetSet(tree, SMOKE_PRESETS.generic2), bambu: presetSet(tree, SMOKE_PRESETS.bambu) }
  for (const s of Object.values(sets)) checkPresetSet(s)
  return sets
}

/** The time one engine run of a case may take: SKETCH_ORCA_SMOKE_TIMEOUT_MS, else 15 minutes. */
export function smokeTimeoutMs(env: NodeJS.ProcessEnv = process.env): number {
  const ms = Number(env.SKETCH_ORCA_SMOKE_TIMEOUT_MS)
  return ms > 0 ? ms : 15 * 60 * 1000
}

export type CaseReport = SmokeReport['cases'][number]

export interface CaseRunOptions {
  exe: string
  /** The engine's resources folder, which the jobs name. */
  resources: string
  /** The Orca checkout the engine was built from, when at hand (see CaseEnv). */
  orcaSource?: string
  /** The environment the engine runs in. */
  env: Record<string, string>
  /** Each case runs in `<root>/<id>`; the preset files go to `<root>/presets`. */
  root: string
  lock: OrcaLock
  sets: SmokePresetSets
  timeoutMs: number
}

/** Runs the cases one after the other with one engine, logging `<label>: pass|FAIL <id>` and each problem. */
export async function runCases(cases: SmokeCase[], o: CaseRunOptions, log: Log, label = 'smoke'): Promise<CaseReport[]> {
  mkdirSync(o.root, { recursive: true })
  const files = {
    generic: writePresetSet(join(o.root, 'presets', 'generic'), o.sets.generic),
    generic2: writePresetSet(join(o.root, 'presets', 'generic2'), o.sets.generic2),
    bambu: writePresetSet(join(o.root, 'presets', 'bambu'), o.sets.bambu),
  }
  for (const note of [...files.generic.notes, ...files.generic2.notes, ...files.bambu.notes]) log.line(`${label}: (preset note) ${note}`)
  const out: CaseReport[] = []
  for (const c of cases) {
    const dir = join(o.root, c.id)
    rmSync(dir, { recursive: true, force: true })
    mkdirSync(dir, { recursive: true })
    const logPath = join(dir, 'engine.log')
    const caseEnv: CaseEnv = {
      exe: o.exe,
      dir,
      resources: o.resources,
      orcaSource: o.orcaSource,
      lock: o.lock,
      sets: o.sets,
      files,
      exec: (args, extraEnv) => runEngine(o.exe, args, { cwd: dir, env: extraEnv ? { ...o.env, ...extraEnv } : o.env, logPath, timeoutMs: o.timeoutMs }),
    }
    let outcome: CaseOutcome
    try {
      outcome = await c.run(caseEnv)
    } catch (err) {
      outcome = { problems: [`${(err as Error).message}`], warnings: [] }
    }
    const ok = outcome.problems.length === 0
    out.push({ id: c.id, title: c.title, ok, problems: outcome.problems, warnings: outcome.warnings })
    log.line(`${label}: ${ok ? 'pass' : 'FAIL'} ${c.id}: ${c.title}`)
    for (const p of outcome.problems) log.line(`${label}:   ${p}`)
    for (const wn of outcome.warnings) log.line(`${label}:   (warning) ${wn}`)
  }
  return out
}

export async function runSmoke(ctx: Context, exe: string, log: Log): Promise<number> {
  const w = ctx.work
  const platform = currentPlatform()
  if (!existsSync(exe)) throw new StepError(`no engine at ${exe}; run build first`)
  const cases = selectedCases(ctx.args)
  const all = cases.length === SMOKE_CASES.length
  const sets = smokePresetSets(join(w.src, 'resources', 'profiles'))
  const exeSha256 = sha256File(exe)
  const fingerprint = smokeFingerprint(w, exe, ctx.lock, platform)
  const inputs = sha256Text(
    JSON.stringify({ exeSha256, fingerprint, commit: ctx.lock.commit, SMOKE_VERSION, presets: Object.values(sets).flatMap((s) => [s.machine, s.process, ...s.filaments].map(serializePreset)) }),
  )
  const reportPath = join(w.smoke, 'report.json')
  if (all && readStamp(w.state, 'smoke')?.inputs === inputs && existsSync(reportPath)) {
    try {
      const report = JSON.parse(readFileSync(reportPath, 'utf8')) as SmokeReport
      if (report.passed && report.exeSha256 === exeSha256 && report.fingerprint === fingerprint) {
        log.line(`smoke: already passed for this engine build (${exeSha256.slice(0, 12)}); skipping`)
        return 0
      }
    } catch {
      // Run again.
    }
  }
  const libDirs = engineLibraryDirs(w, exe, platform)
  if (platform === 'win32') {
    // Before running it: a library Windows cannot find would raise a system dialog instead of an
    // exit code. Here the Visual C++ runtime may come from System32 (stage ships its own).
    const io = windowsRuntimeIo()
    const missing = resolveRuntime(exe, libDirs, io).missing.filter((m) => !io.isSystem(m.name))
    if (missing.length > 0) {
      throw new StepError(`the engine cannot start: ${missing.map((m) => `${m.name} (needed by ${m.neededBy})`).join(', ')} not found in ${libDirs.join('; ')}`)
    }
  }
  const env = engineEnv(process.env, libDirs, platform)
  // A full run decides afresh: the pass of an earlier run no longer counts, whatever this one does
  // (it may fail, or stop half-way).
  if (all) forgetSmokePass(w)
  const report: SmokeReport = { format: 'sketch-orca-smoke/1', smokeVersion: SMOKE_VERSION, exeSha256, fingerprint, orcaCommit: ctx.lock.commit, passed: false, cases: [] }
  report.cases = await runCases(cases, { exe, resources: join(w.src, 'resources'), orcaSource: w.src, env, root: w.smoke, lock: ctx.lock, sets, timeoutMs: smokeTimeoutMs() }, log)
  report.passed = report.cases.every((c) => c.ok)
  writeFileSync(join(w.smoke, all ? 'report.json' : 'report-partial.json'), JSON.stringify(report, null, 2) + '\n')
  const failed = report.cases.filter((c) => !c.ok).length
  if (failed > 0) {
    // A failed case of a subset run fails this engine build too.
    forgetSmokePass(w)
    log.line(`smoke: ${failed} of ${report.cases.length} cases failed (details in ${w.smoke})`)
    return 1
  }
  if (all) writeStamp(w.state, 'smoke', inputs, { exeSha256, smokeVersion: SMOKE_VERSION, fingerprint })
  log.line(`smoke: all ${report.cases.length} cases passed${all ? '' : ' (a subset; no stamp written)'}`)
  return 0
}

/** Removes smoke's pass stamp (state/smoke.json). */
export function forgetSmokePass(work: WorkPaths): void {
  rmSync(join(work.state, 'smoke.json'), { force: true })
}

/**
 * The libraries the engine loads from its library folders (engineLibraryDirs), with their SHA-256:
 * on Windows its import closure, elsewhere the shared libraries there.
 */
export function engineRuntime(work: WorkPaths, exe: string, platform: Platform = currentPlatform(), io?: RuntimeIo): [string, string][] {
  const libDirs = engineLibraryDirs(work, exe, platform)
  return platform === 'win32'
    ? resolveRuntime(exe, libDirs, io ?? windowsRuntimeIo()).copy.map((c): [string, string] => [c.name, sha256File(c.path)])
    : libDirs.flatMap((d) => (existsSync(d) ? readdirSync(d).filter((n) => /\.so(\.\d+)*$|\.dylib$/.test(n)).sort().map((n): [string, string] => [n, sha256File(join(d, n))]) : []))
}

/**
 * What a smoke pass is for: the cases' version, the locked commit, the engine, the libraries it
 * loads (engineRuntime), and the resource folders it reads (ENGINE_RESOURCES of
 * <work>/src/resources). A changed library or resource file is another fingerprint, whose engine
 * has not passed.
 */
export function smokeFingerprint(work: WorkPaths, exe: string, lock: Pick<OrcaLock, 'commit'>, platform: Platform = currentPlatform(), io?: RuntimeIo): string {
  const runtime = engineRuntime(work, exe, platform, io)
  const resources = ENGINE_RESOURCES.map((r) => [r, hashTree(join(work.src, 'resources', r))])
  return sha256Text(JSON.stringify({ smokeVersion: SMOKE_VERSION, commit: lock.commit, exeSha256: sha256File(exe), runtime, resources }))
}

/**
 * True when a full run passed every case, at this case version, for `exe` as it is now with its
 * libraries and resources (`fingerprint`, smokeFingerprint): the pass stamp names its hash and the
 * fingerprint, and the full run's report.json says the same (every case of SMOKE_CASES, passed,
 * for this build).
 */
export function smokePassedFor(work: WorkPaths, exe: string, fingerprint: string): boolean {
  const stamp = readStamp(work.state, 'smoke')
  const detail = stamp?.detail as { exeSha256?: string; smokeVersion?: number; fingerprint?: string } | undefined
  if (!detail || detail.smokeVersion !== SMOKE_VERSION || detail.fingerprint !== fingerprint || !existsSync(exe)) return false
  const exeSha256 = sha256File(exe)
  if (detail.exeSha256 !== exeSha256) return false
  let report: SmokeReport
  try {
    report = JSON.parse(readFileSync(join(work.smoke, 'report.json'), 'utf8')) as SmokeReport
  } catch {
    return false
  }
  const ids = SMOKE_CASES.map((c) => c.id)
  return (
    report.passed === true &&
    report.exeSha256 === exeSha256 &&
    report.fingerprint === fingerprint &&
    report.smokeVersion === SMOKE_VERSION &&
    Array.isArray(report.cases) &&
    report.cases.length === ids.length &&
    report.cases.every((c, i) => c.ok === true && c.id === ids[i])
  )
}
