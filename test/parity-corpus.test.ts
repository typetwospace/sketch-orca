// The parity corpus (scripts/lib/parity/corpus.ts, parity/corpus.json): Orca's 28 meshes and the OBJ
// to binary STL conversion Sketch's model.stl format sets, the OBJ files as they are, the files the
// corpus makes (its zip writer read back, the same bytes every time), the families and which a job's
// settings (at every level), presets, model painting and orient reach, the cases and their ids,
// where a job stops and the engine faults, the families of the refusals by the steps the C++ engine
// reaches before it refuses, phase O3's two rules (domain A's cut: a job outside domain A holds
// only its markers and the front-objects families; the slice rule: no slice run holds `layers`) on
// made-up cases and on corpus.json, the process jobs, a job template made concrete in a case's
// folder (its filaments and its own process), a made mesh's companions copied into one, the invalid
// examples moved into one, and the committed corpus.json itself, with the counts parity/CORPUS.md
// states.
// Hermetic: temporary folders, the repository's own files.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  buildMeshes,
  commaPieces,
  configDefaults,
  CORPUS_FORMAT,
  CORPUS_MESHES,
  corpusCases,
  corpusMeshProblems,
  corpusSmokeCases,
  domainACut,
  DOMAIN_A_PATTERNS,
  ENGINE_FAULTS,
  EXAMPLE_FOLDER,
  EXAMPLE_RESOURCES,
  EXCLUDED_CORNER,
  EXCLUDED_MIDDLE,
  FALSE_CONDITION,
  FAMILIES,
  familiesFor,
  familiesUpTo,
  FRONT_OBJECTS_FAMILIES,
  indexedBox,
  INFILL_PATTERNS,
  instantiateJob,
  MADE_FILES,
  MADE_MESHES,
  madePaintedMeshes,
  MARKER_FAMILIES,
  modelPainting,
  modelXml,
  O3_FAMILIES,
  OBJ_FILE_LAYERS,
  OBJ_FILE_MESHES,
  objectLookups,
  objToBinaryStl,
  objTriangles,
  ORCA_MESHES,
  PAINT_ATTRIBUTES,
  paintedMeshes,
  plain3mf,
  presetFamilies,
  PROCESS_CONDITION,
  PROCESS_REFUSED,
  processJobs,
  readCorpus,
  REFUSAL_FAMILIES,
  rewriteExample,
  roundBed,
  settingFamilies,
  settingLookup,
  settingPoints,
  SKETCH_STL_HEADER,
  sketchBinaryStl,
  SMOKE_FAMILIES,
  SMOKE_LEFT_OUT,
  SMOKE_TOWER_CASES,
  SMOKE_TOWER_FAMILIES,
  SPARSE_PATTERNS,
  STOP_ORDER,
  templateFiles,
  templateSet,
  UNREADABLE_MESHES,
  withFamilies,
  writeZip,
  ZIP_DOS_DATE,
  type CorpusCase,
  type CorpusDocument,
  type JobTemplate,
  type PresetKey,
} from '../scripts/lib/parity/corpus.ts'
import { BOUNDARY_FAMILIES } from '../scripts/lib/parity/certify.ts'
import { sha256Bytes } from '../scripts/lib/files.ts'
import type { FlatPreset, PresetSet } from '../scripts/lib/profiles.ts'
import { REFUSALS, SMOKE_CASES } from '../scripts/lib/smoke.ts'
import { extractZipEntry, readZipEntries } from '../scripts/lib/zip.ts'

/** A flat preset of `type` named `name`, from the system, with `settings`. */
function flat(type: 'machine' | 'process' | 'filament', name: string, settings: Record<string, unknown> = {}): FlatPreset {
  return { type, name, from: 'system', ...settings }
}

/** Preset sets as the smoke run's are made (SMOKE_PRESETS): a printer, a process for it, and its filaments. */
function presetSets(): Record<PresetKey, PresetSet> {
  const set = (printer: string, filaments: number, machine: Record<string, unknown> = {}): PresetSet => ({
    machine: flat('machine', printer, machine),
    process: flat('process', `process for ${printer}`, { compatible_printers: [printer], compatible_printers_condition: '', enable_prime_tower: '1' }),
    filaments: Array.from({ length: filaments }, () => flat('filament', 'PLA', { compatible_printers: [] })),
  })
  return { generic: set('Voron', 1), generic2: set('Voron', 2), bambu: set('X1C', 1), h2d: set('H2D', 2, { wrapping_exclude_area: ['145x310', '256x310', '256x326', '145x326'] }) }
}

/** Orca's defaults the families read, as configdef.json gives them. */
const DEFAULTS = configDefaults({
  options: [
    { optKey: 'wall_generator', default: 'arachne' },
    { optKey: 'sparse_infill_density', default: '20%' },
    { optKey: 'sparse_infill_pattern', default: 'crosshatch' },
    { optKey: 'top_surface_pattern', default: 'monotonicline' },
    { optKey: 'bottom_surface_pattern', default: 'monotonic' },
    { optKey: 'internal_solid_infill_pattern', default: 'monotonic' },
    { optKey: 'enable_support', default: '0' },
    { optKey: 'support_type', default: 'normal(auto)' },
    { optKey: 'fuzzy_skin', default: 'disabled_fuzzy' },
    { optKey: 'ironing_type', default: 'no ironing' },
    { optKey: 'enable_wrapping_detection', default: '0' },
    { optKey: 'wrapping_exclude_area', default: '' },
    { optKey: 'enforce_support_layers', default: 0 },
    { optKey: 'print_sequence', default: 'by layer' },
    { optKey: 'fill_multiline', default: 1 },
  ],
})

/** The painted meshes among the made files, as `parity corpus` finds them (3mf-bambu and its unreadable twin). */
const PAINTED = madePaintedMeshes()

/** Every family process() runs on the generic presets with Orca's defaults, in parity.json's order: what a domain-A layers job holds but `layers`. */
const PROCESS = [
  'slice-mesh',
  'perimeters-arachne',
  'infill-monotonic',
  'infill-monotonicline',
  'infill-crosshatch',
  'infill-concentricinternal',
  'step-estimate-curled-extrusions',
  'step-prepare-infill',
  'step-simplify-path',
  'step-detect-overhangs-for-lift',
  'step-simplify-wall',
  'step-simplify-infill',
  'step-skirt-brim',
  'step-conflict-check',
  'tool-ordering',
  'support-check',
]
/** A domain-A layers job's families on the generic presets. */
const LAYERS = ['model-load', 'placement', ...PROCESS, 'layers']
/** A domain-A slice job's families on the generic presets: process()'s and the G-code's, never `layers`. */
const SLICE = ['model-load', 'placement', ...PROCESS.slice(0, 6), 'gcode-export', 'gcode-processor', ...PROCESS.slice(6)]

const repo = join(import.meta.dir, '..')
const protocolDir = join(repo, 'protocol')
const examples = readdirSync(join(protocolDir, 'examples', 'invalid'))

let tmp: string
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), 'parity-corpus-'))
})
afterEach(() => {
  rmSync(tmp, { recursive: true, force: true })
})

describe('meshes', () => {
  test("Orca's 28 meshes: 21 OBJ files as model.stl, 6 STL files and one 3MF under their own names", () => {
    expect(ORCA_MESHES.length).toBe(28)
    expect(ORCA_MESHES.filter((m) => m.kind === 'obj').length).toBe(21)
    expect(ORCA_MESHES.filter((m) => m.kind === 'stl').map((m) => m.file)).toEqual(['Prusa.stl', '20mmbox-CR.stl', '20mmbox-CRLF.stl', '20mmbox-LF.stl', '20mmbox-nonstandard.stl', '20mmbox-čřšřěá.stl'])
    expect(ORCA_MESHES.filter((m) => m.kind === '3mf').map((m) => m.file)).toEqual(['Büchse.3mf'])
    for (const m of ORCA_MESHES.filter((x) => x.kind === 'obj')) expect(m.file).toBe('model.stl')
    expect(new Set(ORCA_MESHES.map((m) => m.id)).size).toBe(28)
  })

  test('every mesh: the 28, the 21 OBJ files as they are, and the made files, each with its maker', () => {
    expect(CORPUS_MESHES).toEqual([...ORCA_MESHES, ...OBJ_FILE_MESHES, ...MADE_MESHES])
    expect(new Set(CORPUS_MESHES.map((m) => m.id)).size).toBe(CORPUS_MESHES.length)
    for (const m of CORPUS_MESHES) expect(m.id).toMatch(/^[a-z0-9][a-z0-9-]*$/)
    // The OBJ files as they are: the same sources as the converted ones, under their own names.
    expect(OBJ_FILE_MESHES.length).toBe(21)
    const converted = ORCA_MESHES.filter((m) => m.kind === 'obj')
    expect(OBJ_FILE_MESHES.map((m) => m.source)).toEqual(converted.map((m) => m.source))
    for (const m of OBJ_FILE_MESHES) {
      expect(m.kind).toBe('obj-file')
      expect(m.id).toBe(`obj-${converted.find((c) => c.source === m.source)!.id}`)
      expect(m.file).toBe(m.source.split('/').pop()!)
    }
    // Each made file has a maker that makes it and its companions, and nothing else.
    expect(Object.keys(MADE_FILES).sort()).toEqual(MADE_MESHES.map((m) => m.id).sort())
    for (const m of MADE_MESHES) {
      expect(m.kind).toBe('made')
      expect(Object.keys(MADE_FILES[m.id]!()).sort()).toEqual([m.file, ...(m.companions ?? [])].sort())
    }
    for (const id of UNREADABLE_MESHES) expect(MADE_MESHES.some((m) => m.id === id)).toBe(true)
  })

  test('the made files are the same bytes every time', () => {
    for (const m of MADE_MESHES) {
      const a = MADE_FILES[m.id]!()
      const b = MADE_FILES[m.id]!()
      for (const n of Object.keys(a)) expect(a[n]!.equals(b[n]!)).toBe(true)
    }
  })

  test('writeZip: a zip the reader reads back, deflated and stored, its fixed date, a UTF-8 name flagged', () => {
    const text = Buffer.from('<model>'.padEnd(400, 'x'))
    const zip = writeZip([
      { name: '3D/3dmodel.model', data: text },
      { name: 'stored.bin', data: Buffer.from([1, 2, 3]), store: true },
      { name: 'Büchse.txt', data: Buffer.from('ü') },
    ])
    expect(writeZip([{ name: '3D/3dmodel.model', data: text }, { name: 'stored.bin', data: Buffer.from([1, 2, 3]), store: true }, { name: 'Büchse.txt', data: Buffer.from('ü') }]).equals(zip)).toBe(true)
    const entries = readZipEntries(zip)
    expect(entries.map((e) => [e.name, e.method])).toEqual([
      ['3D/3dmodel.model', 8],
      ['stored.bin', 0],
      ['Büchse.txt', 8],
    ])
    expect(extractZipEntry(zip, entries[0]!).equals(text)).toBe(true)
    expect(extractZipEntry(zip, entries[1]!).equals(Buffer.from([1, 2, 3]))).toBe(true)
    expect(extractZipEntry(zip, entries[2]!).toString('utf8')).toBe('ü')
    expect(entries[0]!.compressedSize).toBeLessThan(text.length)
    expect(zip.readUInt16LE(12)).toBe(ZIP_DOS_DATE)
    expect(zip.readUInt16LE(6)).toBe(0)
    expect(zip.readUInt16LE(entries[2]!.localHeaderOffset + 6)).toBe(0x800)
    // The made 3MF archives that load are zips the reader reads, with a model part.
    for (const id of ['3mf-two-objects', '3mf-components-inch', '3mf-bambu']) {
      const m = MADE_MESHES.find((x) => x.id === id)!
      expect(readZipEntries(MADE_FILES[id]!()[m.file]!).map((e) => e.name)).toContain('3D/3dmodel.model')
    }
  })

  test('an OBJ face is split as a fan; negative and v/vt/vn corners name vertices', () => {
    const t = objTriangles('# quad\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1/1/1 2/2/2 3/3/3 4/4/4\nf -4 -3 -2\n')
    expect(t).toEqual([
      [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
      ],
      [
        [0, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
      ],
      [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
      ],
    ])
    expect(() => objTriangles('v 0 0 0\nf 1 2 3\n')).toThrow(/vertex 2 of 1/)
    expect(() => objTriangles('v 0 0\n')).toThrow(/three numbers/)
  })

  test("binary STL as Sketch writes model.stl: 'Sketch' and zero bytes, count, float32 normal and corners, attribute 0", () => {
    const b = sketchBinaryStl([
      [
        [0, 0, 0],
        [2, 0, 0],
        [0, 2, 0],
      ],
      [
        [0, 0, 0],
        [0, 0, 0],
        [1, 1, 1],
      ],
    ])
    expect(b.length).toBe(84 + 2 * 50)
    expect(b.subarray(0, 6).toString('latin1')).toBe(SKETCH_STL_HEADER)
    expect([...b.subarray(6, 80)].every((x) => x === 0)).toBe(true)
    expect(b.readUInt32LE(80)).toBe(2)
    const f = (i: number) => b.readFloatLE(84 + 4 * i)
    expect([f(0), f(1), f(2)]).toEqual([0, 0, 1])
    expect([f(3), f(4), f(5), f(6), f(7), f(8), f(9), f(10), f(11)]).toEqual([0, 0, 0, 2, 0, 0, 0, 2, 0])
    expect(b.readUInt16LE(84 + 48)).toBe(0)
    // A degenerate triangle has a zero normal.
    expect([b.readFloatLE(134), b.readFloatLE(138), b.readFloatLE(142)]).toEqual([0, 0, 0])
    expect(objToBinaryStl('v 0 0 0\nv 2 0 0\nv 0 2 0\nf 1 2 3\n').equals(sketchBinaryStl([[[0, 0, 0], [2, 0, 0], [0, 2, 0]]]))).toBe(true)
  })

  test('buildMeshes converts the OBJ files, copies the rest byte for byte, makes the made files, and corpusMeshProblems finds a changed file', () => {
    const data = join(tmp, 'data')
    const tetra = 'v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n'
    for (const m of ORCA_MESHES) {
      const p = join(data, ...m.source.split('/'))
      mkdirSync(join(p, '..'), { recursive: true })
      writeFileSync(p, m.kind === 'obj' ? tetra : `bytes of ${m.id}`)
    }
    const corpus = join(tmp, 'corpus')
    const meshes = buildMeshes(data, corpus)
    expect(meshes.map((m) => m.id)).toEqual(CORPUS_MESHES.map((m) => m.id))
    const obj = meshes.find((m) => m.kind === 'obj')!
    expect(obj.triangles).toBe(4)
    expect(obj.size).toBe(84 + 4 * 50)
    // The OBJ file as it is: its own bytes under its own name.
    const asIs = meshes.find((m) => m.kind === 'obj-file')!
    expect(readFileSync(join(corpus, 'meshes', asIs.id, asIs.file), 'utf8')).toBe(tetra)
    expect(asIs.sha256).toBe(asIs.sourceSha256)
    const unicode = meshes.find((m) => m.id === '20mmbox-unicode')!
    expect(readFileSync(join(corpus, 'meshes', unicode.id, unicode.file), 'utf8')).toBe('bytes of 20mmbox-unicode')
    // A made file and its companion.
    const mtl = meshes.find((m) => m.id === 'obj-mtl')!
    expect(readFileSync(join(corpus, 'meshes', mtl.id, mtl.file)).equals(MADE_FILES['obj-mtl']!()[mtl.file]!)).toBe(true)
    expect(Object.keys(mtl.companionSha256!)).toEqual(['painted.mtl'])
    expect(existsSync(join(corpus, 'meshes', mtl.id, 'painted.mtl'))).toBe(true)
    // The painting each model's file holds, noted on its entry: the painted made files alone.
    expect(meshes.filter((m) => m.painting !== undefined).map((m) => [m.id, m.painting])).toEqual([...PAINTED].map((id) => [id, modelPainting(CORPUS_MESHES.find((x) => x.id === id)!.file, MADE_FILES[id]!()[CORPUS_MESHES.find((x) => x.id === id)!.file]!)]))
    expect([...paintedMeshes(meshes)]).toEqual([...PAINTED])
    const doc = { meshes } as CorpusDocument
    expect(corpusMeshProblems(doc, corpus)).toEqual([])
    writeFileSync(join(corpus, 'meshes', unicode.id, unicode.file), 'changed')
    writeFileSync(join(corpus, 'meshes', mtl.id, 'painted.mtl'), 'changed')
    expect(corpusMeshProblems(doc, corpus)).toEqual([expect.stringContaining('20mmbox-unicode'), expect.stringContaining('painted.mtl')])
  })
})

describe('families', () => {
  test("every family once: the front end, the 30 infill patterns of Orca's InfillPattern, and the rest by Orca step", () => {
    expect(INFILL_PATTERNS.length).toBe(30)
    const names = FAMILIES.map((f) => f.name)
    expect(new Set(names).size).toBe(names.length)
    for (const n of ['version', 'config-def', 'intake', 'model-load', 'orient', 'placement', 'info', 'slice-mesh', 'perimeters-classic', 'perimeters-arachne', 'supports', 'tree-supports', 'raft', 'ironing', 'fuzzy-skin', 'wipe-tower', 'gcode-export', 'gcode-processor']) {
      expect(names).toContain(n)
    }
    expect(names.filter((n) => n.startsWith('infill-')).length).toBe(30)
    expect(SPARSE_PATTERNS.length).toBe(26)
    expect(names.length).toBe(66)
  })

  test("phase O3's families: after every family before them, so parity.json keeps its order, layers last, each with the step where it runs", () => {
    const names = FAMILIES.map((f) => f.name)
    const added = ['tool-ordering', 'support-check', 'multi-filament', 'painted-facets', 'print-by-object', 'fill-multiline', 'layers']
    expect(names.slice(names.indexOf('step-conflict-check') + 1)).toEqual(added)
    expect(names.indexOf('step-conflict-check')).toBe(58)
    const step = (n: string) => FAMILIES.find((f) => f.name === n)!.step
    expect(added.map(step)).toEqual(['psWipeTower', 'posSupportMaterial', 'psWipeTower', 'posSlice', 'psSkirtBrim', 'posInfill', 'layers'])
    // The O3 families, in parity.json's order.
    expect([...O3_FAMILIES] as string[]).toEqual(names.filter((n) => (O3_FAMILIES as readonly string[]).includes(n)))
    expect(O3_FAMILIES.length).toBe(20)
    // Domain A's patterns and the markers: every family outside domain A, none of them an O3 family.
    expect(DOMAIN_A_PATTERNS.every((p) => (INFILL_PATTERNS as readonly string[]).includes(p))).toBe(true)
    for (const p of INFILL_PATTERNS) expect([p, MARKER_FAMILIES.includes(`infill-${p}`)]).toEqual([p, !(DOMAIN_A_PATTERNS as readonly string[]).includes(p)])
    for (const m of ['supports', 'tree-supports', 'raft', 'ironing', 'fuzzy-skin', 'wipe-tower', 'step-contouring', 'step-simplify-support-path', 'multi-filament', 'painted-facets', 'print-by-object', 'fill-multiline']) expect(MARKER_FAMILIES).toContain(m)
    for (const m of MARKER_FAMILIES) expect(names).toContain(m)
    for (const f of O3_FAMILIES) expect(MARKER_FAMILIES).not.toContain(f)
    // The front-objects families are what certify's front-objects boundary passes.
    expect([...FRONT_OBJECTS_FAMILIES] as string[]).toEqual([...BOUNDARY_FAMILIES['front-objects']!])
  })

  test('familiesFor reads the command and the settings', () => {
    const at = (command: 'info' | 'layers' | 'slice', overrides: Record<string, string>, filaments: number) => familiesFor(command, settingLookup(undefined, overrides, DEFAULTS), filaments, settingPoints(undefined, overrides, DEFAULTS))
    // An info job is answered before the objects are placed (cli/run.cpp run).
    expect(at('info', {}, 1)).toEqual(['model-load', 'info'])
    // A layers job runs process() and answers its layers; a slice job runs process() and writes the G-code.
    expect(at('layers', {}, 1)).toEqual(LAYERS)
    expect(at('slice', {}, 1)).toEqual(SLICE)
    // Orient between the model's reading and the placement.
    const oriented = (command: 'info' | 'layers') => familiesFor(command, settingLookup(undefined, {}, DEFAULTS), 1, settingPoints(undefined, {}, DEFAULTS), true)
    expect(oriented('info')).toEqual(['model-load', 'orient', 'info'])
    expect(oriented('layers')).toEqual(['model-load', 'orient', ...LAYERS.slice(1)])
    const order = FAMILIES.map((f) => f.name)
    expect(order.indexOf('orient')).toBe(order.indexOf('model-load') + 1)
    const plain = at('slice', {}, 1)
    expect(plain).toContain('perimeters-arachne')
    expect(plain).toContain('infill-crosshatch')
    expect(plain).toContain('infill-monotonicline')
    expect(plain).toContain('gcode-export')
    expect(plain).not.toContain('supports')
    const lots = at('slice', { wall_generator: 'classic', enable_support: '1', support_type: 'tree(auto)', raft_layers: '2', ironing_type: 'top', fuzzy_skin: 'external', enable_prime_tower: '1', timelapse_type: '1', zaa_enabled: '1', sparse_infill_density: '0%' }, 1)
    for (const f of ['perimeters-classic', 'tree-supports', 'raft', 'ironing', 'fuzzy-skin', 'wipe-tower', 'step-contouring', 'infill-supportbase']) expect(lots).toContain(f)
    expect(lots).not.toContain('infill-crosshatch')
    // With supports on, the support check of supports off does not run.
    expect(lots).not.toContain('support-check')
    expect(at('slice', { enable_prime_tower: '1' }, 1)).not.toContain('wipe-tower')
    expect(at('slice', { enable_prime_tower: '1' }, 2)).toContain('wipe-tower')
    // The same settings reach the same families on a layers job, which reads them as a slice job does.
    const lotsLayers = at('layers', { wall_generator: 'classic', enable_support: '1', support_type: 'tree(auto)', raft_layers: '2', ironing_type: 'top', fuzzy_skin: 'external', enable_prime_tower: '1', timelapse_type: '1', zaa_enabled: '1', sparse_infill_density: '0%' }, 1)
    expect(lotsLayers).toEqual([...lots.filter((f) => f !== 'gcode-export' && f !== 'gcode-processor'), 'layers'])
  })

  test('the families that mark a job outside domain A, each told from the job and its model', () => {
    const at = (command: 'layers' | 'slice', overrides: Record<string, string>, filaments = 1, painted = false) => familiesFor(command, settingLookup(undefined, overrides, DEFAULTS), filaments, settingPoints(undefined, overrides, DEFAULTS), false, { painted })
    // More than one filament.
    expect(at('layers', {}, 2)).toContain('multi-filament')
    expect(at('layers', {}, 1)).not.toContain('multi-filament')
    // A model with painting.
    expect(at('slice', {}, 1, true)).toContain('painted-facets')
    expect(at('slice', {}, 1, false)).not.toContain('painted-facets')
    // Printing object by object.
    expect(at('layers', { print_sequence: 'by object' })).toContain('print-by-object')
    expect(at('layers', { print_sequence: 'by layer' })).not.toContain('print-by-object')
    // Multiline infill, where there is sparse infill to print.
    expect(at('slice', { fill_multiline: '2' })).toContain('fill-multiline')
    expect(at('slice', { fill_multiline: '1' })).not.toContain('fill-multiline')
    expect(at('slice', { fill_multiline: '3', sparse_infill_density: '0%' })).not.toContain('fill-multiline')
    // Supports as PrintObject::has_support has them: enforced support layers are supports too.
    expect(at('layers', { enforce_support_layers: '3' })).toEqual(expect.arrayContaining(['supports', 'step-simplify-support-path', 'infill-supportbase']))
    expect(at('layers', { enforce_support_layers: '3' })).not.toContain('support-check')
    // Each pattern outside domain A marks it, on any of the three surfaces or the sparse infill.
    expect(at('layers', { sparse_infill_pattern: 'gyroid' })).toContain('infill-gyroid')
    expect(at('layers', { top_surface_pattern: 'hilbertcurve' })).toContain('infill-hilbertcurve')
    expect(at('layers', { internal_solid_infill_pattern: 'alignedrectilinear' })).toContain('infill-alignedrectilinear')
    // The domain-A patterns mark nothing.
    for (const p of ['rectilinear', 'crosshatch', 'concentric']) expect(domainACut(at('layers', { sparse_infill_pattern: p }))).toContain('layers')
    // settingFamilies alone: walls and patterns, the support check with supports off.
    expect(settingFamilies(settingLookup(undefined, {}, DEFAULTS), 1, settingPoints(undefined, {}, DEFAULTS)).sort()).toEqual(['perimeters-arachne', 'infill-monotonicline', 'infill-monotonic', 'infill-crosshatch', 'support-check'].sort())
  })

  test("the settings are read at every level: the job's, each object's and each height range's", () => {
    const get = settingLookup(undefined, { sparse_infill_density: '0%' }, DEFAULTS)
    const levels = objectLookups(get, [
      { index: 0, overrides: { sparse_infill_density: '30%', wall_generator: 'classic' }, layerHeights: { mode: 'fixed' }, heightRanges: [{ from: 2, to: 4, overrides: { sparse_infill_pattern: 'gyroid' } }] },
      { index: 1, overrides: {}, layerHeights: { mode: 'fixed' }, heightRanges: [] },
    ])
    expect(levels.length).toBe(3)
    // A range reads its own setting, then its object's, then the job's.
    expect(levels.map((l) => [l('sparse_infill_pattern'), l('sparse_infill_density'), l('wall_generator')])).toEqual([
      ['crosshatch', '30%', 'classic'],
      ['gyroid', '30%', 'classic'],
      ['crosshatch', '0%', 'arachne'],
    ])
    const families = familiesFor('layers', get, 1, settingPoints(undefined, {}, DEFAULTS), false, { levels })
    // A height range whose sparse pattern is gyroid marks infill-gyroid, as place-object-settings shows.
    expect(families).toContain('infill-gyroid')
    expect(families).toContain('perimeters-classic')
    expect(families).toContain('perimeters-arachne')
    expect(domainACut(families)).toEqual(['model-load', 'placement', 'infill-gyroid'])
    // Without the levels, the job's own settings alone: no sparse pattern at 0 %.
    expect(familiesFor('layers', get, 1, settingPoints(undefined, {}, DEFAULTS))).not.toContain('infill-crosshatch')
  })

  test("a model's painting: the 3MF reader's paint attributes with a value, never an OBJ's colours or an STL", () => {
    expect([...PAINT_ATTRIBUTES]).toEqual(['paint_supports', 'paint_seam', 'paint_color', 'paint_fuzzy_skin'])
    const bambu = MADE_FILES['3mf-bambu']!()['bambu.3mf']!
    expect(modelPainting('bambu.3mf', bambu)).toEqual(['paint_supports', 'paint_seam', 'paint_color', 'paint_fuzzy_skin'])
    expect(modelPainting('BAMBU.3MF', bambu)).toEqual(modelPainting('bambu.3mf', bambu))
    // Each attribute alone, and an empty value, which paints nothing.
    for (const a of PAINT_ATTRIBUTES) expect(modelPainting('one.3mf', plain3mf(modelXml({ objects: [{ id: 1, mesh: indexedBox([0, 0, 0], [5, 5, 5], { 3: { [a]: '4' } }) }], items: [{ objectid: 1 }] })))).toEqual([a])
    expect(modelPainting('empty.3mf', plain3mf(modelXml({ objects: [{ id: 1, mesh: indexedBox([0, 0, 0], [5, 5, 5], { 3: { paint_color: '' } }) }], items: [{ objectid: 1 }] })))).toEqual([])
    expect(modelPainting('plain.3mf', MADE_FILES['3mf-two-objects']!()['two-objects.3mf']!)).toEqual([])
    // An OBJ in two materials paints nothing on the command line; an archive that cannot be read has none.
    expect(modelPainting('painted-long.obj', MADE_FILES['obj-mtl-long']!()['painted-long.obj']!)).toEqual([])
    expect(modelPainting('not-a-zip.3mf', MADE_FILES['3mf-not-a-zip']!()['not-a-zip.3mf']!)).toEqual([])
    // The painted made files: the Bambu project and its twin whose settings do not parse.
    expect([...PAINTED].sort()).toEqual(['3mf-bad-settings-xml', '3mf-bambu'])
    expect(paintedMeshes([{ id: 'a', painting: ['paint_color'] }, { id: 'b', painting: [] }, { id: 'c' }])).toEqual(new Set(['a']))
  })

  test('clumping detection with an exclusion area of more than two points puts the prime tower on for one filament', () => {
    const area = '145x310,256x310,256x326,145x326'
    const tower = (overrides: Record<string, string>) => familiesFor('slice', settingLookup(undefined, overrides, DEFAULTS), 1, settingPoints(undefined, overrides, DEFAULTS)).includes('wipe-tower')
    expect(tower({ enable_prime_tower: '1', enable_wrapping_detection: '1', wrapping_exclude_area: area })).toBe(true)
    expect(tower({ enable_prime_tower: '1', enable_wrapping_detection: '1', wrapping_exclude_area: '0x0,1x0,1x1' })).toBe(true)
    // Each of the three is needed; two points are not enough, nor is a closing comma a point.
    expect(tower({ enable_prime_tower: '1', enable_wrapping_detection: '1' })).toBe(false)
    expect(tower({ enable_prime_tower: '1', wrapping_exclude_area: area })).toBe(false)
    expect(tower({ enable_wrapping_detection: '1', wrapping_exclude_area: area })).toBe(false)
    expect(tower({ enable_prime_tower: '1', enable_wrapping_detection: '1', wrapping_exclude_area: '0x0,1x0,' })).toBe(false)
    // The area in a preset: an array by its length, text in pieces at its commas.
    const set = presetSets().h2d
    const points = settingPoints(set, {}, DEFAULTS)
    expect(points('wrapping_exclude_area')).toBe(4)
    expect(settingPoints({ ...set, machine: { ...set.machine, wrapping_exclude_area: '0x0,1x1' } }, {}, DEFAULTS)('wrapping_exclude_area')).toBe(2)
    expect(settingPoints(undefined, {}, DEFAULTS)('wrapping_exclude_area')).toBe(0)
    expect(familiesFor('slice', settingLookup(set, { enable_wrapping_detection: '1' }, DEFAULTS), 1, points)).toContain('wipe-tower')
    expect(familiesFor('slice', settingLookup(set, {}, DEFAULTS), 1, points)).not.toContain('wipe-tower')
  })

  test("points are counted as Orca's reader splits them at commas", () => {
    expect(['', '0x0', '0x0,1x1', '0x0,1x1,', ',', ',,', '0x0,,1x1'].map(commaPieces)).toEqual([0, 1, 2, 2, 1, 2, 3])
  })

  test('a compatibility condition Orca works out reaches preset-conditions, in the order the program checks them', () => {
    const set = presetSets().generic
    expect(presetFamilies(set)).toEqual([])
    const condition = { compatible_printers: [], compatible_printers_condition: PROCESS_CONDITION }
    expect(presetFamilies({ ...set, process: { ...set.process, ...condition } })).toEqual(['preset-conditions'])
    // A list takes the place of the condition; a process the list leaves out is refused first.
    expect(presetFamilies({ ...set, process: { ...set.process, compatible_printers_condition: PROCESS_CONDITION } })).toEqual([])
    expect(presetFamilies({ ...set, process: { ...set.process, compatible_printers: ['Other'] }, filaments: [{ ...set.filaments[0]!, ...condition }] })).toEqual([])
    // A filament's, with the printer, and with the process when the printer's list takes it.
    expect(presetFamilies({ ...set, filaments: [{ ...set.filaments[0]!, ...condition }] })).toEqual(['preset-conditions'])
    const prints = { compatible_prints: [], compatible_prints_condition: 'layer_height>0.1' }
    expect(presetFamilies({ ...set, filaments: [{ ...set.filaments[0]!, ...prints }] })).toEqual(['preset-conditions'])
    expect(presetFamilies({ ...set, filaments: [{ ...set.filaments[0]!, ...prints, compatible_printers: ['Other'] }] })).toEqual([])
  })
})

describe('cases', () => {
  const cases = corpusCases(protocolDir, examples)

  test('the smoke cases, each mesh as info, layers and slice, the Sketch-shaped and family jobs, the invalid examples and the refusals', () => {
    const ids = cases.map((c) => c.id)
    expect(new Set(ids).size).toBe(ids.length)
    for (const id of ids) expect(id).toMatch(/^[a-z0-9][a-z0-9-]*$/)
    // Every smoke case but refusals, whose jobs are the refusal cases.
    expect(SMOKE_LEFT_OUT).toEqual(['refusals'])
    expect(cases.filter((c) => c.kind === 'smoke').map((c) => c.smoke)).toEqual(SMOKE_CASES.map((c) => c.id).filter((id) => id !== 'refusals'))
    // Orca's 28 as info, layers and slice; its 21 OBJ files read as OBJ, as info, and three as layers too.
    expect(cases.filter((c) => c.id.startsWith('mesh-') && ORCA_MESHES.some((m) => m.id === c.mesh)).length).toBe(28 * 3)
    expect(cases.filter((c) => c.id.startsWith('mesh-obj-')).map((c) => c.id)).toEqual([...OBJ_FILE_MESHES.map((m) => `mesh-${m.id}-info`), ...OBJ_FILE_LAYERS.map((id) => `mesh-${id}-layers`)])
    for (const c of cases.filter((x) => x.id.startsWith('mesh-obj-'))) expect(c.job!.model).toBe(CORPUS_MESHES.find((m) => m.id === c.mesh)!.file)
    expect(cases.filter((c) => c.kind === 'example').length).toBe(examples.filter((n) => n.startsWith('job-')).length)
    expect(cases.filter((c) => c.kind === 'refusal').length).toBe(REFUSALS.length)
    const sketch = cases.filter((c) => c.id.startsWith('sketch-'))
    expect(sketch.map((c) => c.id)).toEqual(['sketch-layers-fixed', 'sketch-layers-profile', 'sketch-slice-pauses', 'sketch-slice-inserts', 'sketch-slice-height-ranges', 'sketch-slice-custom-gcode', 'sketch-slice-timelapse-tower'])
    const profile = sketch[1]!.job!.objects[0]!.layerHeights
    expect(profile.mode === 'profile' && profile.profile[0]).toBe(0)
    const tower = sketch[6]!.job!
    expect(tower.overrides.timelapse_type).toBe('1')
    expect(tower.overrides.enable_prime_tower).toBe('1')
    expect(cases.filter((c) => c.id.startsWith('family-infill-')).length).toBe(SPARSE_PATTERNS.length)
    for (const c of cases.filter((x) => x.kind === 'example')) expect(c.families).toEqual(['intake'])
    const clumping = cases.find((c) => c.id === 'family-wipe-tower-clumping')!.job!
    expect([clumping.command, clumping.presets, clumping.filaments, clumping.overrides]).toEqual(['slice', 'h2d', 1, { enable_wrapping_detection: '1' }])
    const condition = cases.find((c) => c.id === 'family-preset-conditions')!.job!
    expect([condition.command, condition.processChanges]).toEqual(['info', { compatible_printers: [], compatible_printers_condition: PROCESS_CONDITION }])
  })

  test('the cases phase O2 adds: the made files read, placement, preset conditions and orient', () => {
    const ids = (prefix: string) => cases.filter((c) => c.id.startsWith(prefix)).map((c) => c.id)
    const loads = ['obj-texcoords', 'obj-mtl', 'obj-mtl-long', 'obj-mtl-missing', 'obj-mixed', '3mf-two-objects', '3mf-components-inch', '3mf-bambu']
    expect(ids('model-')).toEqual([...loads.flatMap((m) => [`model-${m}-info`, `model-${m}-layers`]), 'model-3mf-only-modifiers-info', 'model-3mf-only-modifiers-layers', ...UNREADABLE_MESHES.map((m) => `model-${m}-info`)])
    for (const c of cases.filter((x) => x.id.startsWith('model-'))) expect(c.job!.model).toBe(MADE_MESHES.find((m) => m.id === c.mesh)!.file)
    expect(cases.find((c) => c.id === 'model-3mf-bambu-layers')!.job!.presets).toBe('generic2')
    expect(ids('place-')).toEqual([
      'place-several-objects',
      'place-arrange-off',
      'place-arrange-off-partly-outside',
      'place-every-object-not-printable',
      'place-no-object-inside',
      'place-bed-exclude-area',
      'place-round-bed',
      'place-tower-default-corner',
      'place-tower-off-excluded-area',
      'place-tower-set-by-job',
      'place-by-object',
      'place-temperature-difference',
      'place-objects-do-not-fit',
      'place-object-settings',
      'place-custom-gcodes',
    ])
    for (const c of cases.filter((x) => x.id.startsWith('place-'))) expect(c.job!.command).toBe('layers')
    expect(cases.find((c) => c.id === 'place-arrange-off')!.job!.arrange).toBe(false)
    expect(cases.find((c) => c.id === 'place-no-object-inside')!.job!.arrange).toBe(false)
    expect(cases.find((c) => c.id === 'place-no-object-inside')!.mesh).toBe('3mf-not-printable-off-bed')
    expect(cases.find((c) => c.id === 'place-round-bed')!.job!.overrides.printable_area).toBe(roundBed())
    expect(roundBed().split(',').length).toBe(36)
    expect(roundBed().split(',')[0]).toBe('120x0')
    expect(cases.find((c) => c.id === 'place-bed-exclude-area')!.job!.overrides.bed_exclude_area).toBe(EXCLUDED_MIDDLE)
    expect(cases.find((c) => c.id === 'place-tower-off-excluded-area')!.job!.overrides.bed_exclude_area).toBe(EXCLUDED_CORNER)
    expect(ids('preset-')).toEqual(['preset-filament-prints-condition', 'preset-filament-not-compatible', 'preset-process-not-compatible', 'preset-condition-unparsable', 'preset-filament-condition-unparsable', 'preset-condition-regex'])
    expect(cases.find((c) => c.id === 'preset-filament-condition-unparsable')!.job!.filamentChanges).toEqual({ compatible_prints: [], compatible_prints_condition: 'layer_height >=' })
    expect(cases.find((c) => c.id === 'preset-process-not-compatible')!.job!.processChanges).toEqual({ compatible_printers: [], compatible_printers_condition: FALSE_CONDITION })
    expect(ids('orient-')).toEqual(['orient-info', 'orient-layers'])
    for (const c of cases.filter((x) => x.id.startsWith('orient-'))) expect(c.job!.orient).toBe(true)
    // The cases that stop before the slice say where.
    expect(cases.filter((c) => c.stopsAt !== undefined).map((c) => [c.id, c.stopsAt])).toEqual([
      ['model-3mf-only-modifiers-layers', 'model-load'],
      ...UNREADABLE_MESHES.map((m) => [`model-${m}-info`, 'model-load']),
      ['place-arrange-off-partly-outside', 'placement'],
      ['place-no-object-inside', 'placement'],
      ['place-temperature-difference', 'placement'],
      ['place-objects-do-not-fit', 'placement'],
      ['preset-process-not-compatible', 'preset-conditions'],
    ])
    for (const c of cases) if (c.stopsAt !== undefined) expect(STOP_ORDER as readonly string[]).toContain(c.stopsAt)
  })

  test('the engine faults: recorded, with their reason, and held to no family', () => {
    const faults = ['mesh-20mmbox-cr-info', 'mesh-20mmbox-cr-layers', 'mesh-20mmbox-cr-slice', 'model-obj-mtl-info', 'model-obj-mtl-layers']
    expect(Object.keys(ENGINE_FAULTS)).toEqual(faults)
    expect(cases.filter((c) => c.fault !== undefined).map((c) => c.id)).toEqual(faults)
    for (const id of faults) expect(cases.find((c) => c.id === id)!.fault).toBe(ENGINE_FAULTS[id]!)
    const filled = withFamilies(cases, presetSets(), DEFAULTS, PAINTED)
    for (const id of faults) expect(filled.find((c) => c.id === id)!.families).toEqual([])
    const of = (id: string) => filled.find((c) => c.id === id)!.families
    // The same OBJ with its library named in 16 bytes or more, and naming a library that is
    // missing, are no fault: held to their families. An OBJ's materials paint nothing.
    expect(of('model-obj-mtl-long-info')).toEqual(['model-load', 'info'])
    expect(of('model-obj-mtl-long-layers')).toEqual(LAYERS)
    expect(of('model-obj-mtl-missing-info')).toEqual(['model-load', 'info'])
    expect(of('model-obj-mtl-missing-layers')).toEqual(LAYERS)
    // The fault is the path's length alone: the same cube and library, named in 11 bytes and in 19.
    const short = MADE_MESHES.find((m) => m.id === 'obj-mtl')!
    const long = MADE_MESHES.find((m) => m.id === 'obj-mtl-long')!
    expect(Buffer.byteLength(short.companions![0]!)).toBeLessThan(16)
    expect(Buffer.byteLength(long.companions![0]!)).toBeGreaterThanOrEqual(16)
    const a = MADE_FILES['obj-mtl']!()
    const b = MADE_FILES['obj-mtl-long']!()
    expect(b[long.file]!.toString('utf8')).toBe(a[short.file]!.toString('utf8').replace(`mtllib ${short.companions![0]}\n`, `mtllib ${long.companions![0]}\n`))
    expect(b[long.companions![0]!]!.equals(a[short.companions![0]!]!)).toBe(true)
  })

  test('familiesUpTo cuts a job at the family it stops in', () => {
    const all = ['preset-conditions', 'model-load', 'orient', 'placement', 'slice-mesh']
    expect(familiesUpTo(all, 'preset-conditions')).toEqual(['preset-conditions'])
    expect(familiesUpTo(all, 'model-load')).toEqual(['preset-conditions', 'model-load'])
    expect(familiesUpTo(all, 'placement')).toEqual(['preset-conditions', 'model-load', 'orient', 'placement'])
    expect(familiesUpTo(['model-load', 'placement', 'slice-mesh'], 'placement')).toEqual(['model-load', 'placement'])
    expect(() => familiesUpTo(all, 'slice-mesh')).toThrow(/not one of/)
    const filled = withFamilies(cases, presetSets(), DEFAULTS, PAINTED)
    const of = (id: string) => filled.find((c) => c.id === id)!.families
    expect(of('model-3mf-not-a-zip-info')).toEqual(['model-load'])
    expect(of('model-3mf-only-modifiers-info')).toEqual(['model-load', 'info'])
    expect(of('model-3mf-only-modifiers-layers')).toEqual(['model-load'])
    expect(of('place-several-objects')).toEqual(LAYERS)
    expect(of('place-objects-do-not-fit')).toEqual(['model-load', 'placement'])
    expect(of('place-temperature-difference')).toEqual(['model-load', 'placement'])
    // Refused in the slice stage before print.process() (Print::apply leaves the print empty): a
    // layers run is answered whole, refusal and all, so it holds every family a layers job does.
    expect(of('place-every-object-not-printable')).toEqual(LAYERS)
    expect(of('preset-process-not-compatible')).toEqual(['preset-conditions'])
    expect(of('preset-condition-regex')).toEqual(['preset-conditions', 'model-load', 'info'])
    expect(of('preset-filament-prints-condition')).toEqual(['preset-conditions', 'model-load', 'info'])
    // Orca prints this condition's parse error on stdout and takes the filament: no stop.
    expect(of('preset-filament-condition-unparsable')).toEqual(['preset-conditions', 'model-load', 'info'])
    // sketch-orca-rs declines an orient job before any output, and a declined run counts against
    // every family its case lists, so an orient case is held to orient alone (familiesFor still adds the others).
    expect(of('orient-info')).toEqual(['orient'])
    expect(of('orient-layers')).toEqual(['orient'])
    // Arrange off and no object inside the volume: the build volume check's own empty-plate.
    expect(of('place-no-object-inside')).toEqual(['model-load', 'placement'])
    expect(of('mesh-obj-20mm-cube-info')).toEqual(['model-load', 'info'])
  })

  test('a refusal is held to the families of the steps the C++ engine reaches before it refuses', () => {
    const intake = [
      'missing-model',
      'unknown-setting',
      'unknown-capitalised-setting',
      'preset-file-key',
      'renamed-key-dropped-value',
      'two-at-one-print-z',
      'profile-not-at-zero',
      'drive-rooted-path',
      // The presets stage and its folders, and a missing model with the stage's warnings.
      'missing-preset',
      'preset-not-json',
      'preset-not-object',
      'preset-wrong-type',
      'preset-bad-from',
      'preset-unreadable-value',
      'not-fdm',
      'process-not-compatible',
      'setting-bad-value',
      'flush-volume-past-int',
      'settings-orca-refuses',
      'resources-without-profiles',
      'work-folder-unmakeable',
      'preset-warnings',
    ]
    expect(REFUSALS.map((r) => r.id).filter((id) => REFUSAL_FAMILIES[id] === undefined)).toEqual(intake)
    const filled = withFamilies(cases, presetSets(), DEFAULTS, PAINTED)
    const of = (id: string) => filled.find((c) => c.id === `refusal-${id}`)!.families
    for (const id of intake) expect(of(id)).toEqual(['intake'])
    expect(of('flat-model')).toEqual(['model-load'])
    expect(of('lone-triangle')).toEqual(['model-load'])
    expect(of('too-big-for-the-bed')).toEqual(['model-load', 'placement'])
    // The model's reading and its objects index; then each step of the placement.
    for (const id of ['empty-model-file', 'stl-no-triangles', 'stl-truncated', 'stl-ascii-bad-facet', 'obj-face-out-of-range', 'object-index-past-model']) expect(of(id)).toEqual(['model-load'])
    // Every placement refusal, from object-whole-plate-setting to off-the-bed, in REFUSALS' order.
    const refusalIds = REFUSALS.map((r) => r.id)
    const placement = refusalIds.slice(refusalIds.indexOf('object-whole-plate-setting'), refusalIds.indexOf('off-the-bed') + 1)
    expect(placement).toEqual([
      'object-whole-plate-setting',
      'object-unknown-setting',
      'object-extruder-past-filaments',
      'range-object-setting',
      'range-zero-layer-height',
      'range-extruder-past-filaments',
      'color-change-without-extruder',
      'filament-change-one-filament',
      'filament-change-past-filaments',
      'pause-without-pause-gcode',
      'custom-gcode-by-object',
      'tower-set-past-the-bed',
      'tower-fits-nowhere',
      'off-the-bed',
    ])
    for (const id of placement) expect(of(id)).toEqual(['model-load', 'placement'])
    // Refused before print.process() (apply_layer_heights): slice-mesh and none of process()'s families.
    expect(of('layers-above-the-nozzle')).toEqual(['model-load', 'placement', 'slice-mesh'])
    const slice = of('gcode-folder-unmakeable')
    expect(slice).toEqual(filled.find((c) => c.id === 'smoke-cube-fixed')!.families)
    for (const f of ['model-load', 'placement', 'slice-mesh', 'perimeters-arachne', 'gcode-export', 'gcode-processor', 'step-conflict-check', 'tool-ordering', 'support-check']) expect(slice).toContain(f)
    // A slice refusal is refused once slice_print has returned: process()'s families, never `layers`.
    expect(slice).toEqual(SLICE)
    expect(of('pause-below-first-layer')).toEqual(slice.filter((f) => f !== 'gcode-export' && f !== 'gcode-processor'))
    expect(of('pause-below-first-layer')).not.toContain('layers')
    // crash-result crashes while slicing: a generic slice too.
    expect(filled.find((c) => c.id === 'smoke-crash-result')!.families).toEqual(slice)
    expect(filled.find((c) => c.id === 'smoke-file-collisions')!.families).toEqual(['intake'])
  })

  test("a job's families: its own filaments and process, its settings and its presets", () => {
    const filled = withFamilies(cases, presetSets(), DEFAULTS, PAINTED)
    const of = (id: string) => filled.find((c) => c.id === id)!.families
    // One filament of the H2D's two: the prime tower is clumping detection's.
    expect(of('family-wipe-tower-clumping')).toContain('wipe-tower')
    expect(of('family-preset-conditions')).toEqual(['preset-conditions', 'model-load', 'info'])
    expect(of('mesh-20mm-cube-info')).toEqual(['model-load', 'info'])
    // Two filaments with the process's prime tower: outside domain A, held to its markers and the front-objects families.
    expect(of('place-tower-default-corner')).toEqual(['model-load', 'placement', 'wipe-tower', 'multi-filament'])
    // The painted Bambu project on two filaments: its painting and its filaments mark it.
    expect(of('model-3mf-bambu-layers')).toEqual(expect.arrayContaining(['model-load', 'placement', 'multi-filament', 'painted-facets']))
    expect(of('model-3mf-bambu-layers').filter((f) => (O3_FAMILIES as readonly string[]).includes(f))).toEqual([])
    // An object's own sparse pattern outside domain A marks its job (a height range's gyroid).
    expect(of('place-object-settings')).toContain('infill-gyroid')
    expect(of('place-by-object')).toContain('print-by-object')
    // The one case of multiline infill.
    expect(of('process-fill-multiline-layers')).toEqual(['model-load', 'placement', 'fill-multiline'])
    const sets = presetSets()
    const clumping = filled.find((c) => c.id === 'family-wipe-tower-clumping')!.job!
    expect(templateSet(clumping, sets).filaments.length).toBe(1)
    expect(templateSet({ ...clumping, overrides: {} }, sets)).toEqual({ ...sets.h2d, filaments: sets.h2d.filaments.slice(0, 1) })
  })

  test("phase O3's two rules on made-up cases: domain A's cut and the slice rule", () => {
    const template = (command: 'info' | 'layers' | 'slice', extra: Partial<JobTemplate> = {}): JobTemplate => ({
      command,
      presets: 'generic',
      model: 'model.stl',
      overrides: {},
      objects: [],
      customGcodes: [],
      arrange: true,
      orient: false,
      gcode: command === 'slice',
      bambu3mf: false,
      stats: true,
      ...extra,
    })
    const job = (id: string, command: 'info' | 'layers' | 'slice', extra: Partial<JobTemplate> = {}, more: Partial<CorpusCase> = {}): CorpusCase => ({ id, kind: 'job', title: id, families: [], mesh: 'made-up', job: template(command, extra), ...more })
    const object = (overrides: Record<string, string>, heightRanges: { from: number; to: number; overrides: Record<string, string> }[] = []) => [{ index: 0, overrides, layerHeights: { mode: 'fixed' as const }, heightRanges }]
    const madeUp: CorpusCase[] = [
      job('plain-layers', 'layers'),
      job('plain-slice', 'slice'),
      job('plain-info', 'info', { overrides: { sparse_infill_pattern: 'gyroid' } }),
      job('gyroid-layers', 'layers', { overrides: { sparse_infill_pattern: 'gyroid' } }),
      job('gyroid-slice', 'slice', { overrides: { sparse_infill_pattern: 'gyroid' } }),
      job('range-gyroid-layers', 'layers', { objects: object({}, [{ from: 2, to: 4, overrides: { sparse_infill_pattern: 'gyroid' } }]) }),
      job('object-ironing-slice', 'slice', { objects: object({ ironing_type: 'top' }) }),
      job('object-classic-layers', 'layers', { objects: object({ wall_generator: 'classic' }) }),
      job('two-filaments-layers', 'layers', { presets: 'generic2' }),
      job('two-filaments-no-tower-slice', 'slice', { presets: 'generic2', overrides: { enable_prime_tower: '0' } }),
      job('painted-layers', 'layers', {}, { mesh: 'painted' }),
      job('painted-info', 'info', {}, { mesh: 'painted' }),
      job('by-object-slice', 'slice', { overrides: { print_sequence: 'by object' } }),
      job('multiline-layers', 'layers', { overrides: { fill_multiline: '2' } }),
      job('enforced-supports-layers', 'layers', { overrides: { enforce_support_layers: '2' } }),
      job('condition-gyroid-layers', 'layers', { overrides: { sparse_infill_pattern: 'gyroid' }, processChanges: { compatible_printers: [], compatible_printers_condition: PROCESS_CONDITION } }),
      job('condition-plain-slice', 'slice', { processChanges: { compatible_printers: [], compatible_printers_condition: PROCESS_CONDITION } }),
      job('stops-gyroid-layers', 'layers', { overrides: { sparse_infill_pattern: 'gyroid' } }, { stopsAt: 'placement' }),
      job('orient-gyroid-layers', 'layers', { overrides: { sparse_infill_pattern: 'gyroid' }, orient: true }),
      { id: 'smoke-layers-only', kind: 'smoke', title: '', families: [], smoke: 'layers-only' },
      { id: 'smoke-cube-fixed', kind: 'smoke', title: '', families: [], smoke: 'cube-fixed' },
      { id: 'smoke-prime-tower', kind: 'smoke', title: '', families: [], smoke: 'prime-tower' },
      { id: 'refusal-gcode-folder-unmakeable', kind: 'refusal', title: '', families: [], refusal: 'gcode-folder-unmakeable' },
      { id: 'refusal-pause-below-first-layer', kind: 'refusal', title: '', families: [], refusal: 'pause-below-first-layer' },
    ]
    const filled = withFamilies(madeUp, presetSets(), DEFAULTS, new Set(['painted']))
    const of = (id: string) => filled.find((c) => c.id === id)!.families
    // Domain A: a layers job holds every family process() runs and `layers`; a slice job those and the G-code's, never `layers`.
    expect(of('plain-layers')).toEqual(LAYERS)
    expect(of('plain-slice')).toEqual(SLICE)
    expect(of('object-classic-layers')).toEqual(['model-load', 'placement', 'slice-mesh', 'perimeters-classic', ...LAYERS.slice(3)])
    // An info job is answered before any marker is read.
    expect(of('plain-info')).toEqual(['model-load', 'info'])
    expect(of('painted-info')).toEqual(['model-load', 'info'])
    // Outside domain A, at any level and from the model: its markers and the front-objects families alone.
    expect(of('gyroid-layers')).toEqual(['model-load', 'placement', 'infill-gyroid'])
    expect(of('gyroid-slice')).toEqual(['model-load', 'placement', 'infill-gyroid'])
    expect(of('range-gyroid-layers')).toEqual(['model-load', 'placement', 'infill-gyroid'])
    expect(of('object-ironing-slice')).toEqual(['model-load', 'placement', 'ironing'])
    expect(of('two-filaments-layers')).toEqual(['model-load', 'placement', 'wipe-tower', 'multi-filament'])
    expect(of('two-filaments-no-tower-slice')).toEqual(['model-load', 'placement', 'multi-filament'])
    expect(of('painted-layers')).toEqual(['model-load', 'placement', 'painted-facets'])
    expect(of('by-object-slice')).toEqual(['model-load', 'placement', 'print-by-object'])
    expect(of('multiline-layers')).toEqual(['model-load', 'placement', 'fill-multiline'])
    expect(of('enforced-supports-layers')).toEqual(['model-load', 'placement', 'infill-supportbase', 'supports', 'step-simplify-support-path'])
    // preset-conditions is a front-objects family: kept beside the markers, and in domain A beside everything.
    expect(of('condition-gyroid-layers')).toEqual(['preset-conditions', 'model-load', 'placement', 'infill-gyroid'])
    expect(of('condition-plain-slice')).toEqual(['preset-conditions', ...SLICE])
    // A stop before the slice, and orient, come before the cut.
    expect(of('stops-gyroid-layers')).toEqual(['model-load', 'placement'])
    expect(of('orient-gyroid-layers')).toEqual(['orient'])
    // Smoke cases and refusals: a layers run holds `layers`, a slice run never.
    expect(of('smoke-layers-only')).toEqual(LAYERS)
    expect(of('smoke-cube-fixed')).toEqual(SLICE)
    expect(of('smoke-prime-tower')).toEqual(['model-load', 'placement', 'wipe-tower'])
    expect([...SMOKE_TOWER_FAMILIES]).toEqual(['model-load', 'placement', 'wipe-tower'])
    expect(SMOKE_TOWER_CASES).toEqual(['filaments', 'flush-volumes', 'prime-tower'])
    expect(Object.entries(SMOKE_FAMILIES).filter(([, f]) => f === 'layers').map(([id]) => id)).toEqual(['layers-only', 'sphere-adaptive'])
    expect(of('refusal-gcode-folder-unmakeable')).toEqual(SLICE)
    expect(of('refusal-pause-below-first-layer')).toEqual(SLICE.filter((f) => f !== 'gcode-export' && f !== 'gcode-processor'))
    // Both rules over every made-up case.
    for (const c of filled) {
      if (c.families.some((f) => MARKER_FAMILIES.includes(f))) {
        expect([c.id, c.families.filter((f) => (O3_FAMILIES as readonly string[]).includes(f))]).toEqual([c.id, []])
        expect([c.id, c.families.every((f) => MARKER_FAMILIES.includes(f) || (FRONT_OBJECTS_FAMILIES as readonly string[]).includes(f))]).toEqual([c.id, true])
      }
      const slices = (c.kind === 'job' && c.job!.command === 'slice') || c.kind === 'refusal' || (c.kind === 'smoke' && SMOKE_FAMILIES[c.smoke!] !== 'layers')
      if (slices) expect([c.id, c.families.includes('layers')]).toEqual([c.id, false])
    }
  })

  test('domainACut keeps a domain-A list whole and cuts any other to its markers and the front-objects families, in order', () => {
    expect(domainACut(LAYERS)).toEqual(LAYERS)
    expect(domainACut(SLICE)).toEqual(SLICE)
    expect(domainACut([])).toEqual([])
    expect(domainACut(['model-load', 'info'])).toEqual(['model-load', 'info'])
    expect(domainACut([...LAYERS, 'raft'])).toEqual(['model-load', 'placement', 'raft'])
    expect(domainACut(['preset-conditions', 'model-load', 'orient', 'placement', 'slice-mesh', 'infill-gyroid', 'supports', 'gcode-export'])).toEqual(['preset-conditions', 'model-load', 'placement', 'infill-gyroid', 'supports'])
    // Each marker alone cuts the job.
    for (const m of MARKER_FAMILIES) expect(domainACut([...SLICE, m])).toEqual(['model-load', 'placement', m])
  })

  test('the process jobs: domain-A layers and slice jobs on the existing meshes, each O3 family reached by both', () => {
    const jobs = processJobs()
    const ids = jobs.map((c) => c.id)
    expect(ids.length).toBe(29)
    expect(new Set(ids).size).toBe(ids.length)
    for (const c of jobs) {
      expect(c.id).toMatch(/^process-[a-z0-9-]+-(layers|slice)$/)
      expect(c.id.endsWith(`-${c.job!.command}`)).toBe(true)
      expect(c.kind).toBe('job')
      expect(c.job!.presets).toBe('generic')
      // On a mesh the corpus has already, never a painted one.
      const mesh = CORPUS_MESHES.find((m) => m.id === c.mesh)!
      expect(c.job!.model).toBe(mesh.file)
      expect(PAINTED.has(c.mesh!)).toBe(false)
    }
    // In corpusCases, after the orient jobs.
    const all = corpusCases(protocolDir, examples).map((c) => c.id)
    expect(all.slice(all.indexOf('orient-layers') + 1, all.indexOf('orient-layers') + 1 + ids.length)).toEqual(ids)
    const filled = withFamilies(jobs, presetSets(), DEFAULTS, PAINTED)
    const domainA = filled.filter((c) => !c.families.some((f) => MARKER_FAMILIES.includes(f)))
    // Every one but the multiline case is in domain A.
    expect(filled.filter((c) => !domainA.includes(c)).map((c) => c.id)).toEqual(['process-fill-multiline-layers'])
    for (const f of O3_FAMILIES) {
      expect([f, domainA.some((c) => c.job!.command === 'layers' && c.families.includes(f))]).toEqual([f, true])
      if (f !== 'layers') expect([f, domainA.some((c) => c.job!.command === 'slice' && c.families.includes(f))]).toEqual([f, true])
    }
    // The three families today's corpus reaches on one slice job alone get two layers jobs each and
    // more slices: perimeters-classic and infill-concentric three, infill-rectilinear two.
    for (const [f, slices] of [['perimeters-classic', 3], ['infill-rectilinear', 2], ['infill-concentric', 3]] as const) {
      expect([f, domainA.filter((c) => c.job!.command === 'layers' && c.families.includes(f)).length]).toEqual([f, 2])
      expect([f, domainA.filter((c) => c.job!.command === 'slice' && c.families.includes(f)).length]).toEqual([f, slices])
    }
    // Each refused process job reaches families past intake: a layers run answered whole, refusal and all.
    expect(Object.keys(PROCESS_REFUSED)).toEqual(['process-layer-height-above-nozzle-layers'])
    for (const id of Object.keys(PROCESS_REFUSED)) {
      expect(ids).toContain(id)
      expect(filled.find((c) => c.id === id)!.families).toEqual(LAYERS)
    }
  })

  test('a job template made concrete in a case folder', () => {
    const c = cases.find((x) => x.id === 'sketch-slice-timelapse-tower')!
    const presets = { machine: join(tmp, 'm.json'), process: join(tmp, 'p.json'), filaments: [join(tmp, 'f.json')] }
    const job = instantiateJob(c.job!, join(tmp, 'case'), join(tmp, 'res'), presets)
    expect(job.model.path).toBe(join(tmp, 'case', 'model.stl'))
    expect(job.workDir).toBe(join(tmp, 'case', 'work'))
    expect(job.output.gcode).toBe(join(tmp, 'case', 'out.gcode'))
    expect(job.output.bambu3mf).toBe(join(tmp, 'case', 'out.gcode.3mf'))
    expect(job.presets).toEqual(presets)
    const info = instantiateJob(cases.find((x) => x.id === 'mesh-buchse-3mf-info')!.job!, join(tmp, 'case'), join(tmp, 'res'), presets)
    expect(info.model.path).toBe(join(tmp, 'case', 'Büchse.3mf'))
    expect(info.output.gcode).toBe('')
  })

  test("a job runs with its first filaments, and with its own copy of a process it changes", () => {
    const sets = presetSets()
    const files = { machine: join(tmp, 'm.json'), process: join(tmp, 'p.json'), filaments: [join(tmp, 'f1.json'), join(tmp, 'f2.json')] }
    const dir = join(tmp, 'case')
    mkdirSync(dir, { recursive: true })
    const clumping = cases.find((x) => x.id === 'family-wipe-tower-clumping')!.job!
    expect(templateFiles(clumping, files, sets, dir)).toEqual({ ...files, filaments: [files.filaments[0]] })
    expect(existsSync(join(dir, 'process.json'))).toBe(false)
    expect(existsSync(join(dir, 'filament_1.json'))).toBe(false)
    const condition = cases.find((x) => x.id === 'family-preset-conditions')!.job!
    const own = templateFiles(condition, files, sets, dir)
    expect(own).toEqual({ ...files, process: join(dir, 'process.json') })
    const written = JSON.parse(readFileSync(own.process, 'utf8')) as Record<string, unknown>
    expect(written).toEqual({ ...sets.generic.process, compatible_printers: [], compatible_printers_condition: PROCESS_CONDITION })
    expect(instantiateJob(condition, dir, join(tmp, 'res'), own).presets.process).toBe(join(dir, 'process.json'))
    // A process change alone writes no copy of a filament.
    expect(existsSync(join(dir, 'filament_1.json'))).toBe(false)
  })

  test('a job runs with its own copies of the filaments it changes', () => {
    const sets = presetSets()
    const files = { machine: join(tmp, 'm.json'), process: join(tmp, 'p.json'), filaments: [join(tmp, 'f1.json')] }
    const dir = join(tmp, 'case-filament')
    mkdirSync(dir, { recursive: true })
    const notCompatible = cases.find((x) => x.id === 'preset-filament-not-compatible')!.job!
    expect(sets[notCompatible.presets].filaments.length).toBe(1)
    const own = templateFiles(notCompatible, files, sets, dir)
    expect(own).toEqual({ ...files, filaments: [join(dir, 'filament_1.json')] })
    expect(existsSync(join(dir, 'filament_1.json'))).toBe(true)
    expect(existsSync(join(dir, 'process.json'))).toBe(false)
    const written = JSON.parse(readFileSync(join(dir, 'filament_1.json'), 'utf8')) as Record<string, unknown>
    expect(written).toEqual({ ...sets[notCompatible.presets].filaments[0]!, compatible_printers: [], compatible_printers_condition: FALSE_CONDITION })
    expect(written.compatible_printers).toEqual([])
    expect(written.compatible_printers_condition).toBe(FALSE_CONDITION)
    expect(instantiateJob(notCompatible, dir, join(tmp, 'res'), own).presets.filaments).toEqual([join(dir, 'filament_1.json')])
    // The filament whose condition for the process does not parse gets its own copy the same way.
    const unparsable = cases.find((x) => x.id === 'preset-filament-condition-unparsable')!.job!
    const other = join(tmp, 'case-filament-unparsable')
    mkdirSync(other, { recursive: true })
    expect(templateFiles(unparsable, files, sets, other).filaments).toEqual([join(other, 'filament_1.json')])
    expect((JSON.parse(readFileSync(join(other, 'filament_1.json'), 'utf8')) as Record<string, unknown>).compatible_prints_condition).toBe('layer_height >=')
    // A job with no filament changes writes no copy of one.
    const plain = join(tmp, 'case-filament-plain')
    mkdirSync(plain, { recursive: true })
    expect(templateFiles(cases.find((x) => x.id === 'preset-condition-regex')!.job!, files, sets, plain).filaments).toEqual(files.filaments)
    expect(existsSync(join(plain, 'filament_1.json'))).toBe(false)
  })

  test("a made mesh's companions are copied into the case's folder beside the model", async () => {
    // The meshes, built into a corpus folder as `parity corpus` builds them.
    const data = join(tmp, 'data')
    for (const m of ORCA_MESHES) {
      const p = join(data, ...m.source.split('/'))
      mkdirSync(join(p, '..'), { recursive: true })
      writeFileSync(p, m.kind === 'obj' ? 'v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n' : `bytes of ${m.id}`)
    }
    const corpusDir = join(tmp, 'corpus')
    const doc = { meshes: buildMeshes(data, corpusDir), cases } as CorpusDocument
    const smoke = corpusSmokeCases(doc, { corpusDir, protocolDir }).find((c) => c.id === 'model-obj-mtl-info')!
    // The case runs with an engine that writes a result and does nothing else.
    const dir = join(tmp, 'case')
    mkdirSync(dir, { recursive: true })
    const sets = presetSets()
    const files = { machine: join(tmp, 'm.json'), process: join(tmp, 'p.json'), filaments: [join(tmp, 'f.json')] }
    const runs: string[][] = []
    const env = {
      exe: join(tmp, 'stub.exe'),
      dir,
      resources: join(tmp, 'res'),
      lock: {} as never,
      sets: sets as never,
      files: { generic: files, generic2: files, bambu: files, h2d: files } as never,
      exec: async (args: string[]) => {
        runs.push(args)
        writeFileSync(join(dir, 'result.json'), '{}\n')
        return { code: 0, timedOut: false, stdout: '' }
      },
    }
    expect(await smoke.run(env)).toEqual({ problems: [], warnings: [] })
    expect(runs.length).toBe(1)
    const made = MADE_FILES['obj-mtl']!()
    expect(readFileSync(join(dir, 'painted.obj')).equals(made['painted.obj']!)).toBe(true)
    expect(readFileSync(join(dir, 'painted.mtl')).equals(made['painted.mtl']!)).toBe(true)
    const job = JSON.parse(readFileSync(join(dir, 'job.json'), 'utf8')) as { model: { path: string } }
    expect(job.model.path).toBe(join(dir, 'painted.obj'))
  })

  test("an invalid example's paths move into the case folder; a relative model path stays relative", () => {
    const presets = { machine: join(tmp, 'm.json'), process: join(tmp, 'p.json'), filaments: [join(tmp, 'f.json')] }
    const read = (n: string) => JSON.parse(readFileSync(join(protocolDir, 'examples', 'invalid', n), 'utf8')) as Record<string, unknown>
    const moved = rewriteExample(read('job-no-filament.json'), join(tmp, 'case'), join(tmp, 'res'), presets)
    expect(moved.resources).toBe(join(tmp, 'res'))
    expect(moved.workDir).toBe(join(tmp, 'case', 'work'))
    expect((moved.model as { path: string }).path).toBe(join(tmp, 'case', 'cube.stl'))
    expect((moved.presets as { filaments: string[] }).filaments).toEqual([])
    expect((moved.presets as { machine: string }).machine).toBe(presets.machine)
    const relative = rewriteExample(read('job-relative-model-path.json'), join(tmp, 'case'), join(tmp, 'res'), presets)
    expect((relative.model as { path: string }).path).toBe('cube.stl')
    expect(JSON.stringify(relative)).not.toContain(EXAMPLE_FOLDER)
    expect(JSON.stringify(relative)).not.toContain(EXAMPLE_RESOURCES)
  })
})

describe('parity/corpus.json', () => {
  const file = join(repo, 'parity', 'corpus.json')

  test('is the corpus corpus.ts defines, with a source and SHA-256 for each mesh and families for each case', () => {
    expect(existsSync(file)).toBe(true)
    const doc = readCorpus(file)
    expect(doc.format).toBe(CORPUS_FORMAT)
    expect(doc.meshes.map((m) => m.id)).toEqual(CORPUS_MESHES.map((m) => m.id))
    for (const m of doc.meshes) {
      expect(m.sha256).toMatch(/^[0-9a-f]{64}$/)
      expect(m.sourceSha256).toMatch(/^[0-9a-f]{64}$/)
      if (m.kind !== 'obj') expect(m.sha256).toBe(m.sourceSha256)
      if (m.kind === 'made') expect(m.sha256).toBe(sha256Bytes(MADE_FILES[m.id]!()[m.file]!))
      expect(Object.keys(m.companionSha256 ?? {})).toEqual(CORPUS_MESHES.find((x) => x.id === m.id)!.companions ?? [])
      for (const [n, sha] of Object.entries(m.companionSha256 ?? {})) expect(sha).toBe(sha256Bytes(MADE_FILES[m.id]!()[n]!))
    }
    expect(doc.cases.map((c) => c.id)).toEqual(corpusCases(protocolDir, examples).map((c) => c.id))
    expect(doc.families.map((f) => f.name)).toEqual(FAMILIES.map((f) => f.name))
    // Every case but an engine fault is held to some family.
    for (const c of doc.cases) {
      if (c.fault === undefined) expect(c.families.length).toBeGreaterThan(0)
      else expect([c.id, c.families]).toEqual([c.id, []])
    }
    expect(doc.cases.filter((c) => c.fault !== undefined).map((c) => c.id)).toEqual(Object.keys(ENGINE_FAULTS))
    for (const f of doc.families) expect(doc.cases.some((c) => c.families.includes(f.name))).toBe(true)
    expect(readFileSync(file, 'utf8')).not.toMatch(/[A-Za-z]:\\\\Users|\/home\//)
  })

  test('holds the counts parity/CORPUS.md states', () => {
    const doc = readCorpus(file)
    const md = readFileSync(join(repo, 'parity', 'CORPUS.md'), 'utf8')
    const stated = (re: RegExp) => Number(re.exec(md)?.[1] ?? NaN)
    const kind = (k: string) => doc.cases.filter((c) => c.kind === k).length
    expect(stated(/There are (\d+) cases, of four kinds/)).toBe(doc.cases.length)
    for (const k of ['smoke', 'job', 'example', 'refusal']) expect([k, stated(new RegExp(`\\| \`${k}\` \\| (\\d+) \\|`))]).toEqual([k, kind(k)])
    expect(stated(/(\d+) jobs of the files the corpus makes, `model-\*`/)).toBe(doc.cases.filter((c) => c.id.startsWith('model-')).length)
    expect(stated(/There are (\d+) meshes:/)).toBe(doc.meshes.length)
    const made = doc.meshes.filter((m) => m.kind === 'made').length
    expect(stated(/and (\d+) files the corpus makes\./)).toBe(made)
    expect(stated(/`MADE_FILES` in `corpus.ts` writes (\d+) files/)).toBe(made)
    expect(stated(/There are (\d+) mesh jobs/)).toBe(doc.cases.filter((c) => c.id.startsWith('mesh-') && ORCA_MESHES.some((m) => m.id === c.mesh)).length)
    expect(stated(/(\d+) process jobs, `process-\*`/)).toBe(doc.cases.filter((c) => c.id.startsWith('process-')).length)
    expect(stated(/`FAMILIES` in `corpus.ts` lists (\d+) families/)).toBe(doc.families.length)
    expect(stated(/There are (\d+) cases outside domain A/)).toBe(doc.cases.filter((c) => c.families.some((f) => MARKER_FAMILIES.includes(f))).length)
  })

  test("domain A's cut: a case outside domain A holds no O3 family, only its markers and the front-objects families, today's as orca-o3-corpus lists them", () => {
    const doc = readCorpus(file)
    const outside = doc.cases.filter((c) => c.families.some((f) => MARKER_FAMILIES.includes(f)))
    for (const c of outside) {
      expect([c.id, c.families.filter((f) => (O3_FAMILIES as readonly string[]).includes(f))]).toEqual([c.id, []])
      expect([c.id, c.families.filter((f) => !MARKER_FAMILIES.includes(f) && !(FRONT_OBJECTS_FAMILIES as readonly string[]).includes(f))]).toEqual([c.id, []])
    }
    const tower = ['model-load', 'placement', 'wipe-tower']
    const twoFilaments = ['model-load', 'placement', 'wipe-tower', 'multi-filament']
    const patterns = ['alignedrectilinear', 'zigzag', 'crosszag', 'lockedzag', 'line', 'grid', 'triangles', 'tri-hexagon', 'cubic', 'adaptivecubic', 'quartercubic', 'supportcubic', 'lightning', 'honeycomb', '3dhoneycomb', 'lateral-honeycomb', 'lateral-lattice', 'tpmsd', 'tpmsfk', 'gyroid', 'hilbertcurve', 'archimedeanchords', 'octagramspiral']
    const today: [string, string[]][] = [
      ['smoke-filaments', tower],
      ['smoke-flush-volumes', tower],
      ['smoke-prime-tower', tower],
      ['sketch-slice-timelapse-tower', tower],
      ...patterns.map((p): [string, string[]] => [`family-infill-${p}`, ['model-load', 'placement', `infill-${p}`]]),
      ['family-supports', ['model-load', 'placement', 'infill-supportbase', 'supports', 'step-simplify-support-path']],
      ['family-tree-supports', ['model-load', 'placement', 'infill-supportbase', 'tree-supports', 'step-simplify-support-path']],
      ['family-raft', ['model-load', 'placement', 'raft']],
      ['family-ironing', ['model-load', 'placement', 'ironing']],
      ['family-fuzzy-skin', ['model-load', 'placement', 'fuzzy-skin']],
      ['family-contouring', ['model-load', 'placement', 'step-contouring']],
      ['family-wipe-tower-clumping', tower],
      ['model-3mf-bambu-layers', ['model-load', 'placement', 'wipe-tower', 'multi-filament', 'painted-facets']],
      ['place-tower-default-corner', twoFilaments],
      ['place-tower-off-excluded-area', twoFilaments],
      ['place-tower-set-by-job', twoFilaments],
      ['place-by-object', ['model-load', 'placement', 'print-by-object']],
      ['place-object-settings', ['model-load', 'placement', 'infill-gyroid']],
      ['place-custom-gcodes', twoFilaments],
    ]
    expect(today.length).toBe(41)
    expect(outside.filter((c) => !c.id.startsWith('process-')).map((c) => [c.id, c.families])).toEqual(today)
    // The cases this phase adds outside domain A: multiline infill alone.
    expect(outside.filter((c) => c.id.startsWith('process-')).map((c) => [c.id, c.families])).toEqual([['process-fill-multiline-layers', ['model-load', 'placement', 'fill-multiline']]])
    // Each marker family has a case, so the program's decline of it is held to one.
    for (const m of ['multi-filament', 'painted-facets', 'print-by-object', 'fill-multiline']) expect([m, outside.some((c) => c.families.includes(m))]).toEqual([m, true])
  })

  test('the slice rule: no slice job, slice refusal or smoke case with a slice run holds layers; the domain-A layers runs do', () => {
    const doc = readCorpus(file)
    for (const c of doc.cases) {
      const slices = (c.kind === 'job' && c.job!.command === 'slice') || c.kind === 'refusal' || (c.kind === 'smoke' && SMOKE_FAMILIES[c.smoke!] !== 'layers')
      if (slices) expect([c.id, c.families.includes('layers')]).toEqual([c.id, false])
      // A domain-A job that reaches process() holds the end of its command: layers for a layers job, the G-code's for a slice.
      const domainA = c.kind === 'job' && c.families.includes('slice-mesh') && !c.families.some((f) => MARKER_FAMILIES.includes(f))
      if (domainA && c.job!.command === 'layers') expect([c.id, c.families.includes('layers'), c.families.includes('gcode-export')]).toEqual([c.id, true, false])
      if (domainA && c.job!.command === 'slice') expect([c.id, c.families.includes('gcode-export'), c.families.includes('step-conflict-check')]).toEqual([c.id, true, true])
    }
    const of = (id: string) => doc.cases.find((c) => c.id === id)!.families
    expect(of('smoke-layers-only')).toEqual(of('smoke-sphere-adaptive'))
    expect(of('smoke-layers-only')).toContain('layers')
    for (const id of SMOKE_TOWER_CASES) expect(of(`smoke-${id}`)).toEqual([...SMOKE_TOWER_FAMILIES])
    expect(of('smoke-crash-result')).toEqual(of('smoke-cube-fixed'))
    expect(of('refusal-gcode-folder-unmakeable')).toEqual(of('smoke-cube-fixed'))
    expect(of('refusal-layers-above-the-nozzle')).toEqual(['model-load', 'placement', 'slice-mesh'])
  })

  test('each O3 family has a domain-A layers job, and each but layers a domain-A slice job; today\'s cases hold what orca-o3-corpus counts', () => {
    const doc = readCorpus(file)
    const domainA = doc.cases.filter((c) => c.kind === 'job' && !c.families.some((f) => MARKER_FAMILIES.includes(f)))
    for (const f of O3_FAMILIES) {
      expect([f, domainA.some((c) => c.job!.command === 'layers' && c.families.includes(f))]).toEqual([f, true])
      expect([f, domainA.some((c) => c.job!.command === 'slice' && c.families.includes(f))]).toEqual([f, f !== 'layers'])
    }
    // Today's cases (all but the process jobs), by kind: layers jobs, slice jobs, smoke cases, refusals.
    const today = doc.cases.filter((c) => !c.id.startsWith('process-'))
    expect(today.length).toBe(276)
    const count = (f: string) => {
      const of = today.filter((c) => c.families.includes(f))
      return [of.filter((c) => c.kind === 'job' && c.job!.command === 'layers').length, of.filter((c) => c.kind === 'job' && c.job!.command === 'slice').length, of.filter((c) => c.kind === 'smoke').length, of.filter((c) => c.kind === 'refusal').length]
    }
    const process = [43, 36, 11, 2]
    const expected: Record<string, number[]> = {
      'slice-mesh': [43, 36, 11, 3],
      'perimeters-classic': [0, 1, 0, 0],
      'perimeters-arachne': [43, 35, 11, 2],
      'infill-rectilinear': [0, 1, 0, 0],
      'infill-crosshatch': [43, 34, 11, 2],
      'infill-concentric': [0, 1, 0, 0],
      layers: [43, 0, 2, 0],
    }
    for (const f of O3_FAMILIES) expect([f, count(f)]).toEqual([f, expected[f] ?? process])
    // The process jobs give the three that had one slice job alone a domain-A layers job each and more slices.
    for (const f of ['perimeters-classic', 'infill-rectilinear', 'infill-concentric']) {
      expect(domainA.filter((c) => c.job!.command === 'layers' && c.families.includes(f)).length).toBeGreaterThanOrEqual(1)
      expect(domainA.filter((c) => c.job!.command === 'slice' && c.families.includes(f)).length).toBeGreaterThanOrEqual(2)
    }
    // Every refused process job reaches a family past intake, and every refusal past intake is unchanged by this phase.
    for (const id of Object.keys(PROCESS_REFUSED)) expect(doc.cases.find((c) => c.id === id)!.families.some((f) => f !== 'intake')).toBe(true)
  })

  test('the painting of each mesh, and the painted model reaching painted-facets', () => {
    const doc = readCorpus(file)
    const painted = doc.meshes.filter((m) => m.painting !== undefined)
    expect(painted.map((m) => m.id)).toEqual(['3mf-bambu', '3mf-bad-settings-xml'])
    for (const m of painted) expect([m.id, m.painting]).toEqual([m.id, modelPainting(m.file, MADE_FILES[m.id]!()[m.file]!)])
    expect(paintedMeshes(doc.meshes)).toEqual(PAINTED)
    // A painted model's job that reaches placement holds painted-facets; one stopped at model-load or answered by info does not.
    for (const c of doc.cases.filter((x) => x.kind === 'job' && PAINTED.has(x.mesh!))) {
      const reaches = c.job!.command !== 'info' && c.stopsAt === undefined && c.fault === undefined && !c.job!.orient
      expect([c.id, c.families.includes('painted-facets')]).toEqual([c.id, reaches])
    }
  })
})
