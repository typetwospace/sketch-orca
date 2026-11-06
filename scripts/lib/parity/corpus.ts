// The parity corpus (parity/corpus.json, parity/CORPUS.md): the jobs the Rust translation is held to
// the C++ engine on, and the families of Orca's work each case exercises. Four kinds of case:
//   smoke    each of the smoke run's cases (smoke.ts SMOKE_CASES), run as the smoke run runs it;
//   job      a job written from the protocol's job schema (protocol/schema/job.schema.json): each of
//            Orca's 28 test meshes (<work>/src/tests/data) as an info, a layers and a slice job, and
//            the Sketch-shaped jobs (layer profiles, pauses, colour changes, height ranges, custom
//            G-code, a single-filament prime tower through smooth timelapse), and a slice for each
//            family the presets leave off (each sparse infill pattern, both wall generators,
//            supports, tree supports, raft, ironing, fuzzy skin, Z contouring);
//   example  protocol/examples/invalid/job-*.json, its example paths moved into the case's folder;
//   refusal  each of the smoke run's refused jobs (smoke.ts REFUSALS), one case each.
// The meshes stay in the work folder: `parity corpus` converts the 21 OBJ files to binary STL as
// Sketch writes model.stl (objToBinaryStl, CORPUS.md) and copies the 6 STL files and the 3MF as they
// are into <work>/parity/corpus/meshes/<mesh>/, and writes parity/corpus.json, which names each
// mesh's source and SHA-256. A case runs as a smoke case does (same.ts runEngineCases), so every
// output of every engine run is recorded the same way. No Sketch code is in here.

import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { sha256Bytes } from '../files.ts'
import type { CustomGcode, Job, JobCommand, JobObject, Overrides } from '../job.ts'
import { cube, encodeBinaryStl, writeStl } from '../models.ts'
import type { PresetFiles, PresetSet } from '../profiles.ts'
import { StepError } from '../run.ts'
import { PLAIN_OVERRIDES, REFUSALS, SMOKE_CASES, SMOKE_PRESETS, SMOKE_VERSION, type SmokeCase } from '../smoke.ts'

export const CORPUS_FORMAT = 'sketch-orca-parity-corpus/1'

/** Orca's test meshes, below the engine's checkout (<work>/src). */
export const ORCA_TEST_DATA = 'tests/data'

export type MeshKind = 'obj' | 'stl' | '3mf'

export interface CorpusMesh {
  id: string
  /** Below <work>/src/tests/data, with forward slashes. */
  source: string
  kind: MeshKind
  /** The model's file name in a case's folder: model.stl for a converted OBJ, else the source's own name. */
  file: string
}

const OBJ_NAMES = [
  '20mm_cube',
  '2x20x10',
  'A',
  'A_upsidedown',
  'V',
  'V_standing',
  'bridge',
  'cube_with_concave_hole',
  'cube_with_concave_hole_enlarged',
  'cube_with_concave_hole_enlarged_standing',
  'cube_with_hole',
  'extruder_idler',
  'extruder_idler_quads',
  'frog_legs',
  'ipadstand',
  'overhang',
  'pyramid',
  'simplification',
  'sloping_hole',
  'small_dorito',
  'two_hollow_squares',
] as const

/** The 28 meshes: 21 OBJ files (converted), 6 STL files and one 3MF (as they are). */
export const CORPUS_MESHES: CorpusMesh[] = [
  ...OBJ_NAMES.map((n): CorpusMesh => ({ id: n.toLowerCase().replace(/_/g, '-'), source: `${n}.obj`, kind: 'obj', file: 'model.stl' })),
  { id: 'prusa-stl', source: 'test_3mf/Prusa.stl', kind: 'stl', file: 'Prusa.stl' },
  { id: '20mmbox-cr', source: 'test_stl/ASCII/20mmbox-CR.stl', kind: 'stl', file: '20mmbox-CR.stl' },
  { id: '20mmbox-crlf', source: 'test_stl/ASCII/20mmbox-CRLF.stl', kind: 'stl', file: '20mmbox-CRLF.stl' },
  { id: '20mmbox-lf', source: 'test_stl/ASCII/20mmbox-LF.stl', kind: 'stl', file: '20mmbox-LF.stl' },
  { id: '20mmbox-nonstandard', source: 'test_stl/ASCII/20mmbox-nonstandard.stl', kind: 'stl', file: '20mmbox-nonstandard.stl' },
  { id: '20mmbox-unicode', source: 'test_stl/Geräte/20mmbox-čřšřěá.stl', kind: 'stl', file: '20mmbox-čřšřěá.stl' },
  { id: 'buchse-3mf', source: 'test_3mf/Geräte/Büchse.3mf', kind: '3mf', file: 'Büchse.3mf' },
]

// ---------------------------------------------------------------- OBJ to binary STL

/** The header Sketch gives model.stl: these bytes, then zero bytes to 80. */
export const SKETCH_STL_HEADER = 'Sketch'

/**
 * The triangles of an OBJ file: `v x y z` vertices and `f` faces, whose corners are a vertex number
 * (1-based, or negative from the last vertex read) before any `/`. A face of n corners is the fan
 * (1, k, k+1) for k = 2..n-1, as Orca's OBJ reader splits a quad. Other lines are left out.
 */
export function objTriangles(text: string): [number, number, number][][] {
  const v: [number, number, number][] = []
  const out: [number, number, number][][] = []
  for (const raw of text.split(/\r\n|\r|\n/)) {
    const line = raw.trim()
    if (line.startsWith('v ') || line.startsWith('v\t')) {
      const p = line.split(/\s+/).slice(1, 4).map(Number)
      if (p.length !== 3 || !p.every(Number.isFinite)) throw new Error(`an OBJ vertex is not three numbers: ${line}`)
      v.push(p as [number, number, number])
    } else if (line.startsWith('f ') || line.startsWith('f\t')) {
      const idx = line
        .split(/\s+/)
        .slice(1)
        .map((c) => {
          const n = Number(c.split('/')[0])
          if (!Number.isInteger(n) || n === 0) throw new Error(`an OBJ face corner is not a vertex number: ${line}`)
          const i = n < 0 ? v.length + n : n - 1
          if (i < 0 || i >= v.length) throw new Error(`an OBJ face names vertex ${n} of ${v.length}: ${line}`)
          return v[i]!
        })
      if (idx.length < 3) throw new Error(`an OBJ face has ${idx.length} corners: ${line}`)
      for (let k = 1; k + 1 < idx.length; k++) out.push([idx[0]!, idx[k]!, idx[k + 1]!])
    }
  }
  return out
}

/**
 * A binary STL as Sketch writes model.stl (CORPUS.md): an 80-byte header of SKETCH_STL_HEADER and
 * zero bytes, the triangle count (u32), then per triangle the unit normal of (b - a) x (c - a),
 * computed in double precision from the corners as given (zero for a degenerate triangle), the
 * three corners, all as little-endian float32, and a u16 attribute of 0.
 */
export function sketchBinaryStl(triangles: [number, number, number][][]): Buffer {
  const buf = Buffer.alloc(84 + 50 * triangles.length)
  buf.write(SKETCH_STL_HEADER, 0, 'latin1')
  buf.writeUInt32LE(triangles.length, 80)
  let o = 84
  for (const [a, b, c] of triangles) {
    const ux = b![0] - a![0]
    const uy = b![1] - a![1]
    const uz = b![2] - a![2]
    const vx = c![0] - a![0]
    const vy = c![1] - a![1]
    const vz = c![2] - a![2]
    let nx = uy * vz - uz * vy
    let ny = uz * vx - ux * vz
    let nz = ux * vy - uy * vx
    const length = Math.hypot(nx, ny, nz)
    if (length > 0) {
      nx /= length
      ny /= length
      nz /= length
    }
    for (const x of [nx, ny, nz, ...a!, ...b!, ...c!]) {
      buf.writeFloatLE(x, o)
      o += 4
    }
    buf.writeUInt16LE(0, o)
    o += 2
  }
  return buf
}

export function objToBinaryStl(text: string): Buffer {
  return sketchBinaryStl(objTriangles(text))
}

// ---------------------------------------------------------------- families

/** Orca's InfillPattern (PrintConfig.hpp), each as its key in the settings, or a name of ours for the two with none. */
export const INFILL_PATTERNS = [
  'monotonic',
  'monotonicline',
  'rectilinear',
  'alignedrectilinear',
  'zigzag',
  'crosszag',
  'lockedzag',
  'line',
  'grid',
  'triangles',
  'tri-hexagon',
  'cubic',
  'adaptivecubic',
  'quartercubic',
  'supportcubic',
  'lightning',
  'honeycomb',
  '3dhoneycomb',
  'lateral-honeycomb',
  'lateral-lattice',
  'crosshatch',
  'tpmsd',
  'tpmsfk',
  'gyroid',
  'concentric',
  'hilbertcurve',
  'archimedeanchords',
  'octagramspiral',
  'supportbase',
  'concentricinternal',
] as const

export interface Family {
  name: string
  /** The Orca step (Print.hpp PrintStep / PrintObjectStep) or front-end command it belongs to. */
  step: string
  what: string
}

/** Every family parity.json lists, in its order. */
export const FAMILIES: Family[] = [
  { name: 'version', step: 'front end', what: 'sketch-orca version' },
  { name: 'config-def', step: 'front end', what: 'sketch-orca config-def' },
  { name: 'intake', step: 'front end', what: 'reading and checking a job, and the refusals' },
  { name: 'model-load', step: 'load', what: "reading the model (STL, OBJ, 3MF) and Orca's mesh repairs" },
  { name: 'placement', step: 'arrange', what: 'arranging the objects on the plate' },
  { name: 'info', step: 'info', what: 'the info command' },
  { name: 'slice-mesh', step: 'posSlice', what: 'layer heights and slicing the mesh into layers' },
  { name: 'perimeters-classic', step: 'posPerimeters', what: 'walls, classic generator' },
  { name: 'perimeters-arachne', step: 'posPerimeters', what: 'walls, Arachne generator' },
  ...INFILL_PATTERNS.map((p): Family => ({ name: `infill-${p}`, step: 'posInfill', what: `the ${p} infill pattern` })),
  { name: 'supports', step: 'posSupportMaterial', what: 'normal supports' },
  { name: 'tree-supports', step: 'posSupportMaterial', what: 'tree supports' },
  { name: 'raft', step: 'posSupportMaterial', what: 'rafts' },
  { name: 'ironing', step: 'posIroning', what: 'ironing' },
  { name: 'fuzzy-skin', step: 'posPerimeters', what: 'fuzzy skin' },
  { name: 'wipe-tower', step: 'psWipeTower', what: 'the prime tower and tool ordering' },
  { name: 'gcode-export', step: 'psGCodeExport', what: 'writing the G-code' },
  { name: 'gcode-processor', step: 'psGCodeExport', what: "the G-code processor's statistics and times" },
  { name: 'step-estimate-curled-extrusions', step: 'posEstimateCurledExtrusions', what: 'curled extrusion estimates' },
  { name: 'step-prepare-infill', step: 'posPrepareInfill', what: 'top, bottom and internal surfaces' },
  { name: 'step-contouring', step: 'posContouring', what: 'Z contouring' },
  { name: 'step-simplify-path', step: 'posSimplifyPath', what: 'path simplification' },
  { name: 'step-simplify-support-path', step: 'posSimplifySupportPath', what: 'support path simplification' },
  { name: 'step-detect-overhangs-for-lift', step: 'posDetectOverhangsForLift', what: 'overhangs for z hop' },
  { name: 'step-simplify-wall', step: 'posSimplifyWall', what: 'wall simplification' },
  { name: 'step-simplify-infill', step: 'posSimplifyInfill', what: 'infill simplification' },
  { name: 'step-skirt-brim', step: 'psSkirtBrim', what: 'skirt and brim' },
  { name: 'step-conflict-check', step: 'psConflictCheck', what: 'the conflict check between objects' },
]

/** A setting as a job sees it: the job's override, else the preset's value, else Orca's default. */
export type SettingLookup = (key: string) => string | undefined

/** The setting lookup for a preset set, the job's overrides and Orca's defaults (configdef.json of the probe). */
export function settingLookup(set: PresetSet | undefined, overrides: Overrides, defaults: Map<string, string>): SettingLookup {
  return (key) => {
    if (key in overrides) return overrides[key]
    for (const p of set ? [set.process, set.machine, ...set.filaments] : []) {
      const v = (p as Record<string, unknown>)[key]
      if (v !== undefined) return Array.isArray(v) ? String(v[0]) : String(v)
    }
    return defaults.get(key)
  }
}

/** Orca's defaults by key, from the probe's configdef.json (each option's `default`). */
export function configDefaults(configdef: unknown): Map<string, string> {
  const out = new Map<string, string>()
  const options = (configdef as { options?: { optKey: string; default?: unknown }[] }).options ?? []
  for (const o of options) if (o.default !== undefined && o.default !== null) out.set(o.optKey, Array.isArray(o.default) ? String(o.default[0]) : String(o.default))
  return out
}

/** Every step of a slice that runs whatever the settings (Print.cpp process). */
const SLICE_STEPS = ['step-estimate-curled-extrusions', 'step-prepare-infill', 'step-simplify-path', 'step-detect-overhangs-for-lift', 'step-simplify-wall', 'step-simplify-infill', 'step-skirt-brim', 'step-conflict-check']

const PATTERN_KEYS = new Map<string, string>(INFILL_PATTERNS.map((p) => [p, `infill-${p}`]))

/**
 * The families a job exercises, as its command and settings name them: the patterns it fills with,
 * the wall generator, supports, raft, ironing, fuzzy skin and the prime tower when they are on.
 */
export function familiesFor(command: JobCommand, get: SettingLookup, filaments: number): string[] {
  const out = new Set<string>(['model-load', 'placement'])
  if (command === 'info') out.add('info')
  if (command === 'layers' || command === 'slice') out.add('slice-mesh')
  if (command === 'slice') {
    out.add(get('wall_generator') === 'classic' ? 'perimeters-classic' : 'perimeters-arachne')
    const density = parseFloat(get('sparse_infill_density') ?? '0')
    const patterns = [get('top_surface_pattern'), get('bottom_surface_pattern'), get('internal_solid_infill_pattern'), ...(density > 0 ? [get('sparse_infill_pattern')] : [])]
    for (const p of patterns) if (p && PATTERN_KEYS.has(p)) out.add(PATTERN_KEYS.get(p)!)
    if (get('enable_support') === '1') {
      out.add((get('support_type') ?? '').startsWith('tree') ? 'tree-supports' : 'supports')
      out.add('step-simplify-support-path')
      out.add('infill-supportbase')
    }
    if (Number(get('raft_layers') ?? '0') > 0) out.add('raft')
    if ((get('ironing_type') ?? 'no ironing') !== 'no ironing') out.add('ironing')
    if (!['none', 'disabled_fuzzy', undefined].includes(get('fuzzy_skin'))) out.add('fuzzy-skin')
    if (get('enable_prime_tower') === '1' && (filaments > 1 || get('timelapse_type') === '1')) out.add('wipe-tower')
    if (get('zaa_enabled') === '1') out.add('step-contouring')
    // Fill.cpp turns narrow internal solid infill into this pattern on any slice that has some.
    out.add('infill-concentricinternal')
    for (const s of SLICE_STEPS) out.add(s)
    out.add('gcode-export')
    out.add('gcode-processor')
  }
  const order = new Map(FAMILIES.map((f, i) => [f.name, i]))
  return [...out].sort((a, b) => order.get(a)! - order.get(b)!)
}

// ---------------------------------------------------------------- cases

export type PresetKey = keyof typeof SMOKE_PRESETS

/** A job as the corpus holds it: paths written relative to the case's folder, presets by set. */
export interface JobTemplate {
  command: JobCommand
  presets: PresetKey
  /** The model's file name in the case's folder. */
  model: string
  overrides: Overrides
  objects: JobObject[]
  customGcodes: CustomGcode[]
  arrange: boolean
  orient: boolean
  /** Whether the job writes G-code (out.gcode) and a .gcode.3mf (out.gcode.3mf). */
  gcode: boolean
  bambu3mf: boolean
  stats: boolean
}

export type CaseKind = 'smoke' | 'job' | 'example' | 'refusal'

export interface CorpusCase {
  id: string
  kind: CaseKind
  title: string
  /** The families it exercises; filled in by `parity corpus` for jobs (familiesFor). */
  families: string[]
  /** smoke: the smoke case's id. */
  smoke?: string
  /** job: the mesh (CORPUS_MESHES id) and the job. */
  mesh?: string
  job?: JobTemplate
  /** example: the file below protocol/examples/invalid. */
  example?: string
  /** refusal: the smoke refusal's id. */
  refusal?: string
}

/** The families of each smoke case, by hand: what its jobs run (smoke.ts). */
export const SMOKE_FAMILIES: Record<string, string[]> = {
  version: ['version'],
  'config-def': ['config-def'],
  refusals: ['intake'],
  'file-collisions': ['intake'],
  'crash-result': ['intake'],
  info: ['model-load', 'placement', 'info'],
  'layers-only': ['model-load', 'placement', 'slice-mesh'],
  'sphere-adaptive': ['model-load', 'placement', 'slice-mesh'],
}

/** Smoke cases that run a prime tower (two filaments, or the tower case). */
export const SMOKE_TOWER_CASES = ['filaments', 'flush-volumes', 'prime-tower']

function plainJob(command: JobCommand, model: string, extra: Partial<JobTemplate> = {}): JobTemplate {
  return {
    command,
    presets: 'generic',
    model,
    overrides: {},
    objects: [],
    customGcodes: [],
    arrange: true,
    orient: false,
    gcode: command === 'slice',
    bambu3mf: false,
    stats: true,
    ...extra,
  }
}

/** The fixed layers the Sketch-shaped slice jobs print at: 0.2 mm from a 0.2 mm first layer. */
const SKETCH_LAYERS: Overrides = { ...PLAIN_OVERRIDES, layer_height: '0.2', initial_layer_print_height: '0.2' }

/**
 * The mesh the Sketch-shaped jobs use: the 20 mm cube, flat on the bed and flat on top, so every
 * layer to the top prints (a profile must end at the top; the front end fails a slice whose G-code
 * stops below it, as a pointed top does).
 */
export const SKETCH_MESH = '20mm-cube'

/** The Sketch-shaped jobs, written from the protocol's job schema. */
export function sketchJobs(): CorpusCase[] {
  const m = CORPUS_MESHES.find((x) => x.id === SKETCH_MESH)!
  const obj = (o: Partial<JobObject>): JobObject => ({ index: 0, overrides: {}, layerHeights: { mode: 'fixed' }, heightRanges: [], ...o })
  const cases: [string, string, JobTemplate][] = [
    ['sketch-layers-fixed', 'layers at fixed heights, no profile', plainJob('layers', m.file, { overrides: SKETCH_LAYERS })],
    [
      'sketch-layers-profile',
      'layers from a boundary profile',
      plainJob('layers', m.file, { overrides: SKETCH_LAYERS, objects: [obj({ layerHeights: { mode: 'profile', profile: [0, 0.2, 5, 0.12, 12, 0.24, 20, 0.2] } })] }),
    ],
    [
      'sketch-slice-pauses',
      'a slice with two pauses',
      plainJob('slice', m.file, {
        overrides: SKETCH_LAYERS,
        customGcodes: [
          { printZ: 2, type: 'pause', note: 'corpus: first pause' },
          { printZ: 4, type: 'pause', note: 'corpus: second pause' },
        ],
      }),
    ],
    ['sketch-slice-inserts', 'a slice with a colour change inserted', plainJob('slice', m.file, { overrides: SKETCH_LAYERS, customGcodes: [{ printZ: 3, type: 'color', color: '#C03030' }] })],
    [
      'sketch-slice-height-ranges',
      'a slice with a height range of its own settings',
      plainJob('slice', m.file, { overrides: SKETCH_LAYERS, objects: [obj({ heightRanges: [{ from: 2, to: 6, overrides: { sparse_infill_density: '40%', wall_loops: '4' } }] })] }),
    ],
    ['sketch-slice-custom-gcode', 'a slice with custom G-code at a layer', plainJob('slice', m.file, { overrides: SKETCH_LAYERS, customGcodes: [{ printZ: 5, type: 'custom', gcode: 'M117 corpus custom' }] })],
    [
      'sketch-slice-timelapse-tower',
      'one filament, a prime tower through smooth timelapse, written as a .gcode.3mf',
      plainJob('slice', m.file, { presets: 'bambu', overrides: { ...SKETCH_LAYERS, enable_prime_tower: '1', timelapse_type: '1' }, bambu3mf: true }),
    ],
  ]
  return cases.map(([id, title, job]) => ({ id, kind: 'job', title, families: [], mesh: m.id, job }))
}

/** The sparse infill patterns Orca's settings take (sparse_infill_pattern's values). */
export const SPARSE_PATTERNS = INFILL_PATTERNS.filter((p) => !['monotonic', 'monotonicline', 'supportbase', 'concentricinternal'].includes(p))

/** The meshes the family jobs slice: a cube for fills and surfaces, the overhang for supports. */
export const FAMILY_MESHES = { fill: '20mm-cube', overhang: 'overhang' } as const

/**
 * A slice for each family no other case reaches as the presets stand: each sparse infill pattern
 * on the 20 mm cube, both wall generators, and supports, tree supports, a raft, ironing, fuzzy skin
 * and Z contouring, each switched on by its setting.
 */
export function familyJobs(): CorpusCase[] {
  const mesh = (id: string) => CORPUS_MESHES.find((x) => x.id === id)!
  const fill = mesh(FAMILY_MESHES.fill)
  const overhang = mesh(FAMILY_MESHES.overhang)
  const job = (id: string, title: string, m: CorpusMesh, overrides: Overrides): CorpusCase => ({ id, kind: 'job', title, families: [], mesh: m.id, job: plainJob('slice', m.file, { overrides }) })
  return [
    ...SPARSE_PATTERNS.map((p) => job(`family-infill-${p}`, `the 20 mm cube with ${p} sparse infill`, fill, { sparse_infill_pattern: p, sparse_infill_density: '25%' })),
    job('family-perimeters-classic', 'the 20 mm cube with the classic wall generator', fill, { wall_generator: 'classic' }),
    job('family-perimeters-arachne', 'the 20 mm cube with the Arachne wall generator', fill, { wall_generator: 'arachne' }),
    job('family-supports', 'the overhang with normal supports', overhang, { enable_support: '1', support_type: 'normal(auto)' }),
    job('family-tree-supports', 'the overhang with tree supports', overhang, { enable_support: '1', support_type: 'tree(auto)' }),
    job('family-raft', 'the overhang on a raft of 2 layers', overhang, { raft_layers: '2' }),
    job('family-ironing', 'the 20 mm cube with its top ironed', fill, { ironing_type: 'top' }),
    job('family-fuzzy-skin', 'the 20 mm cube with fuzzy skin on its outer walls', fill, { fuzzy_skin: 'external' }),
    job('family-contouring', 'the 20 mm cube with Z contouring (zaa_enabled)', fill, { zaa_enabled: '1' }),
  ]
}

/** protocol/examples/invalid/job-*.json, sorted. */
export function exampleJobs(protocolDir: string, names: string[]): CorpusCase[] {
  return names
    .filter((n) => /^job-.*\.json$/.test(n))
    .sort()
    .map((n) => {
      if (!existsSync(join(protocolDir, 'examples', 'invalid', n))) throw new StepError(`no ${n} in protocol/examples/invalid`)
      return { id: `example-${n.slice(4, -5)}`, kind: 'example', title: `protocol/examples/invalid/${n}`, families: ['intake'], example: n }
    })
}

/** Every case, in the corpus's order (families of jobs are filled in by withFamilies). */
export function corpusCases(protocolDir: string, exampleNames: string[]): CorpusCase[] {
  const smoke = SMOKE_CASES.map((c): CorpusCase => ({ id: `smoke-${c.id}`, kind: 'smoke', title: c.title, families: [], smoke: c.id }))
  const meshes = CORPUS_MESHES.flatMap((m) =>
    (['info', 'layers', 'slice'] as const).map((command): CorpusCase => ({ id: `mesh-${m.id}-${command}`, kind: 'job', title: `${m.source}, ${command}`, families: [], mesh: m.id, job: plainJob(command, m.file) })),
  )
  const refusals = REFUSALS.map((r): CorpusCase => ({ id: `refusal-${r.id}`, kind: 'refusal', title: `the smoke refusal ${r.id} (${r.code})`, families: ['intake'], refusal: r.id }))
  return [...smoke, ...meshes, ...sketchJobs(), ...familyJobs(), ...exampleJobs(protocolDir, exampleNames), ...refusals]
}

/** Fills in each case's families: jobs from their settings, smoke cases from SMOKE_FAMILIES or as a generic slice. */
export function withFamilies(cases: CorpusCase[], sets: Record<PresetKey, PresetSet>, defaults: Map<string, string>): CorpusCase[] {
  return cases.map((c) => {
    if (c.kind === 'job') {
      const j = c.job!
      return { ...c, families: familiesFor(j.command, settingLookup(sets[j.presets], j.overrides, defaults), sets[j.presets].filaments.length) }
    }
    if (c.kind === 'smoke') {
      const fixed = SMOKE_FAMILIES[c.smoke!]
      if (fixed) return { ...c, families: fixed }
      const f = familiesFor('slice', settingLookup(sets.generic, PLAIN_OVERRIDES, defaults), 1)
      return { ...c, families: SMOKE_TOWER_CASES.includes(c.smoke!) ? [...new Set([...f, 'wipe-tower'])].sort((a, b) => FAMILIES.findIndex((x) => x.name === a) - FAMILIES.findIndex((x) => x.name === b)) : f }
    }
    return c
  })
}

// ---------------------------------------------------------------- the corpus file

export interface CorpusMeshEntry extends CorpusMesh {
  sourceSha256: string
  /** The file a case gets (the converted STL for an OBJ). */
  sha256: string
  size: number
  /** Triangles of a converted OBJ. */
  triangles?: number
}

export interface CorpusDocument {
  format: typeof CORPUS_FORMAT
  smokeVersion: number
  orcaCommit: string
  /** How an OBJ becomes model.stl (CORPUS.md). */
  stl: string
  meshes: CorpusMeshEntry[]
  families: Family[]
  cases: CorpusCase[]
}

export const STL_DESCRIPTION = `binary STL as Sketch writes model.stl: 80-byte header "${SKETCH_STL_HEADER}" padded with zero bytes, u32 triangle count, per triangle the unit normal of (b-a)x(c-a) computed in double from the corners (zero when degenerate), corners a, b, c, all little-endian float32, then u16 0; an OBJ face of n corners is the fan (1,k,k+1)`

/** The mesh folder of the corpus: <corpus>/meshes/<mesh>/<file>. */
export function meshPath(corpusDir: string, m: CorpusMesh): string {
  return join(corpusDir, 'meshes', m.id, m.file)
}

/** Converts or copies the 28 meshes into `corpusDir` (fresh) and returns their entries. */
export function buildMeshes(testData: string, corpusDir: string): CorpusMeshEntry[] {
  rmSync(join(corpusDir, 'meshes'), { recursive: true, force: true })
  return CORPUS_MESHES.map((m) => {
    const src = join(testData, ...m.source.split('/'))
    if (!existsSync(src)) throw new StepError(`no ${m.source} in ${testData}; the engine's checkout (<work>/src) is at another commit`)
    const bytes = readFileSync(src)
    const to = meshPath(corpusDir, m)
    mkdirSync(join(corpusDir, 'meshes', m.id), { recursive: true })
    if (m.kind === 'obj') {
      const tris = objTriangles(bytes.toString('utf8'))
      const stl = sketchBinaryStl(tris)
      writeFileSync(to, stl)
      return { ...m, sourceSha256: sha256Bytes(bytes), sha256: sha256Bytes(stl), size: stl.length, triangles: tris.length }
    }
    copyFileSync(src, to)
    return { ...m, sourceSha256: sha256Bytes(bytes), sha256: sha256Bytes(bytes), size: bytes.length }
  })
}

export function corpusDocument(o: { orcaCommit: string; meshes: CorpusMeshEntry[]; cases: CorpusCase[] }): CorpusDocument {
  return { format: CORPUS_FORMAT, smokeVersion: SMOKE_VERSION, orcaCommit: o.orcaCommit, stl: STL_DESCRIPTION, meshes: o.meshes, families: FAMILIES, cases: o.cases }
}

/** parity/corpus.json in the repository. */
export function corpusFile(engineDir: string): string {
  return join(engineDir, 'parity', 'corpus.json')
}

export function readCorpus(path: string): CorpusDocument {
  if (!existsSync(path)) throw new StepError(`no corpus at ${path}; run parity corpus`)
  const doc = JSON.parse(readFileSync(path, 'utf8')) as CorpusDocument
  if (doc.format !== CORPUS_FORMAT) throw new StepError(`${path} is ${JSON.stringify(doc.format)}, not ${CORPUS_FORMAT}`)
  return doc
}

/** Why the meshes in `corpusDir` are not the ones the corpus names (missing or other bytes). */
export function corpusMeshProblems(doc: CorpusDocument, corpusDir: string): string[] {
  const out: string[] = []
  for (const m of doc.meshes) {
    const p = meshPath(corpusDir, m)
    if (!existsSync(p)) out.push(`${m.id}: no ${p}`)
    else if (sha256Bytes(readFileSync(p)) !== m.sha256) out.push(`${m.id}: ${p} is not the file corpus.json names`)
  }
  return out
}

// ---------------------------------------------------------------- running a case

/** A job template made concrete in `dir`, with the engine's resources and the preset files written there. */
export function instantiateJob(t: JobTemplate, dir: string, resources: string, presets: PresetFiles): Job {
  return {
    format: 'sketch-orca-job/1',
    command: t.command,
    resources,
    workDir: join(dir, 'work'),
    model: { path: join(dir, t.model) },
    presets: { machine: presets.machine, process: presets.process, filaments: presets.filaments },
    overrides: t.overrides,
    objects: t.objects,
    customGcodes: t.customGcodes,
    arrange: t.arrange,
    orient: t.orient,
    output: { gcode: t.gcode ? join(dir, 'out.gcode') : '', bambu3mf: t.bambu3mf ? join(dir, 'out.gcode.3mf') : null, stats: t.stats },
  }
}

/** The example paths of protocol/examples/invalid, and what each becomes in a case's folder. */
export const EXAMPLE_RESOURCES = '/opt/sketch-orca/resources'
export const EXAMPLE_FOLDER = '/jobs/3/'

/**
 * An invalid example moved into `dir`: its resources become the engine's, its presets the files of
 * the generic set, and every other path under /jobs/3/ the same name in `dir`. A path that is not
 * under /jobs/3/ (a relative one, which is what the example is about) stays as it is.
 */
export function rewriteExample(example: Record<string, unknown>, dir: string, resources: string, presets: PresetFiles): Record<string, unknown> {
  const move = (p: unknown): unknown => (typeof p === 'string' && p.startsWith(EXAMPLE_FOLDER) ? join(dir, ...p.slice(EXAMPLE_FOLDER.length).split('/')) : p)
  const out: Record<string, unknown> = { ...example }
  if (out.resources === EXAMPLE_RESOURCES) out.resources = resources
  out.workDir = move(out.workDir)
  if (out.model && typeof out.model === 'object') out.model = { ...(out.model as Record<string, unknown>), path: move((out.model as Record<string, unknown>).path) }
  const p = out.presets as { machine?: unknown; process?: unknown; filaments?: unknown[] } | undefined
  if (p && typeof p === 'object') {
    out.presets = {
      ...p,
      machine: typeof p.machine === 'string' && p.machine.startsWith(EXAMPLE_FOLDER) ? presets.machine : p.machine,
      process: typeof p.process === 'string' && p.process.startsWith(EXAMPLE_FOLDER) ? presets.process : p.process,
      filaments: Array.isArray(p.filaments) ? p.filaments.map((f, i) => (typeof f === 'string' && f.startsWith(EXAMPLE_FOLDER) ? (presets.filaments[i] ?? presets.filaments[0]!) : f)) : p.filaments,
    }
  }
  if (out.output && typeof out.output === 'object') {
    const o = out.output as Record<string, unknown>
    out.output = { ...o, gcode: move(o.gcode), bambu3mf: move(o.bambu3mf) }
  }
  return out
}

type CaseEnv = Parameters<SmokeCase['run']>[0]

/** Writes the job as it is (no checks: the engine makes its own) and runs it; a result is all a corpus case asks for. */
async function runCorpusJob(env: CaseEnv, job: Record<string, unknown>): Promise<{ problems: string[]; warnings: string[] }> {
  const jobPath = join(env.dir, 'job.json')
  const resultPath = join(env.dir, 'result.json')
  const progressPath = join(env.dir, 'progress.ndjson')
  const workDir = job.workDir
  if (typeof workDir === 'string' && workDir.startsWith(env.dir)) mkdirSync(workDir, { recursive: true })
  writeFileSync(jobPath, JSON.stringify(job, null, 2) + '\n')
  const r = await env.exec(['run', '--job', jobPath, '--out', resultPath, '--progress', progressPath])
  if (r.timedOut) return { problems: ['the engine did not finish in time'], warnings: [] }
  if (!existsSync(resultPath)) return { problems: [`the engine exited with ${r.code} and wrote no result`], warnings: [] }
  return { problems: [], warnings: [] }
}

/** The smoke cases the corpus names, run under the corpus case's id. */
export function corpusSmokeCases(doc: CorpusDocument, o: { corpusDir: string; protocolDir: string }): SmokeCase[] {
  const meshes = new Map(doc.meshes.map((m) => [m.id, m]))
  return doc.cases.map((c): SmokeCase => {
    switch (c.kind) {
      case 'smoke': {
        const s = SMOKE_CASES.find((x) => x.id === c.smoke)
        if (!s) throw new StepError(`corpus case ${c.id} names the smoke case ${c.smoke}, which smoke.ts does not have`)
        return { id: c.id, title: s.title, run: (env) => s.run(env) }
      }
      case 'job': {
        const m = meshes.get(c.mesh!)
        if (!m) throw new StepError(`corpus case ${c.id} names the mesh ${c.mesh}, which corpus.json does not list`)
        return {
          id: c.id,
          title: c.title,
          run: async (env) => {
            copyFileSync(meshPath(o.corpusDir, m), join(env.dir, c.job!.model))
            return runCorpusJob(env, instantiateJob(c.job!, env.dir, env.resources, env.files[c.job!.presets]) as unknown as Record<string, unknown>)
          },
        }
      }
      case 'example':
        return {
          id: c.id,
          title: c.title,
          run: async (env) => {
            const example = JSON.parse(readFileSync(join(o.protocolDir, 'examples', 'invalid', c.example!), 'utf8')) as Record<string, unknown>
            const job = rewriteExample(example, env.dir, env.resources, env.files.generic)
            const model = (job.model as { path?: unknown } | undefined)?.path
            if (typeof model === 'string' && model.startsWith(env.dir)) writeFileSync(model, encodeBinaryStl(cube(20), basename(model)))
            return runCorpusJob(env, job)
          },
        }
      case 'refusal': {
        const r = REFUSALS.find((x) => x.id === c.refusal)
        if (!r) throw new StepError(`corpus case ${c.id} names the refusal ${c.refusal}, which smoke.ts does not have`)
        return {
          id: c.id,
          title: c.title,
          run: async (env) => {
            const { job, model } = r.make(env.dir, env.resources, env.files.generic)
            if (model) writeStl(job.model.path, model, r.id)
            return runCorpusJob(env, job as unknown as Record<string, unknown>)
          },
        }
      }
    }
  })
}
