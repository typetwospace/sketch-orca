// The parity corpus (scripts/lib/parity/corpus.ts, parity/corpus.json): the 28 meshes and the OBJ
// to binary STL conversion Sketch's model.stl format sets, the families and which a job's settings
// reach, the cases and their ids, a job template made concrete in a case's folder, the invalid
// examples moved into one, and the committed corpus.json itself. Hermetic: temporary folders, the
// repository's own files.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  buildMeshes,
  configDefaults,
  CORPUS_FORMAT,
  CORPUS_MESHES,
  corpusCases,
  corpusMeshProblems,
  EXAMPLE_FOLDER,
  EXAMPLE_RESOURCES,
  FAMILIES,
  familiesFor,
  INFILL_PATTERNS,
  instantiateJob,
  objToBinaryStl,
  objTriangles,
  readCorpus,
  rewriteExample,
  settingLookup,
  SKETCH_STL_HEADER,
  sketchBinaryStl,
  SPARSE_PATTERNS,
  type CorpusDocument,
} from '../scripts/lib/parity/corpus.ts'
import { REFUSALS, SMOKE_CASES } from '../scripts/lib/smoke.ts'

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
  test('28 meshes: 21 OBJ files as model.stl, 6 STL files and one 3MF under their own names', () => {
    expect(CORPUS_MESHES.length).toBe(28)
    expect(CORPUS_MESHES.filter((m) => m.kind === 'obj').length).toBe(21)
    expect(CORPUS_MESHES.filter((m) => m.kind === 'stl').map((m) => m.file)).toEqual(['Prusa.stl', '20mmbox-CR.stl', '20mmbox-CRLF.stl', '20mmbox-LF.stl', '20mmbox-nonstandard.stl', '20mmbox-čřšřěá.stl'])
    expect(CORPUS_MESHES.filter((m) => m.kind === '3mf').map((m) => m.file)).toEqual(['Büchse.3mf'])
    for (const m of CORPUS_MESHES.filter((x) => x.kind === 'obj')) expect(m.file).toBe('model.stl')
    expect(new Set(CORPUS_MESHES.map((m) => m.id)).size).toBe(28)
    for (const m of CORPUS_MESHES) expect(m.id).toMatch(/^[a-z0-9][a-z0-9-]*$/)
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

  test('buildMeshes converts the OBJ files, copies the rest byte for byte, and corpusMeshProblems finds a changed file', () => {
    const data = join(tmp, 'data')
    for (const m of CORPUS_MESHES) {
      const p = join(data, ...m.source.split('/'))
      mkdirSync(join(p, '..'), { recursive: true })
      writeFileSync(p, m.kind === 'obj' ? 'v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n' : `bytes of ${m.id}`)
    }
    const corpus = join(tmp, 'corpus')
    const meshes = buildMeshes(data, corpus)
    expect(meshes.length).toBe(28)
    const obj = meshes.find((m) => m.kind === 'obj')!
    expect(obj.triangles).toBe(4)
    expect(obj.size).toBe(84 + 4 * 50)
    const unicode = meshes.find((m) => m.id === '20mmbox-unicode')!
    expect(readFileSync(join(corpus, 'meshes', unicode.id, unicode.file), 'utf8')).toBe('bytes of 20mmbox-unicode')
    const doc = { meshes } as CorpusDocument
    expect(corpusMeshProblems(doc, corpus)).toEqual([])
    writeFileSync(join(corpus, 'meshes', unicode.id, unicode.file), 'changed')
    expect(corpusMeshProblems(doc, corpus)).toEqual([expect.stringContaining('20mmbox-unicode')])
  })
})

describe('families', () => {
  test("every family once: the front end, the 30 infill patterns of Orca's InfillPattern, and the rest by Orca step", () => {
    expect(INFILL_PATTERNS.length).toBe(30)
    const names = FAMILIES.map((f) => f.name)
    expect(new Set(names).size).toBe(names.length)
    for (const n of ['version', 'config-def', 'intake', 'model-load', 'placement', 'info', 'slice-mesh', 'perimeters-classic', 'perimeters-arachne', 'supports', 'tree-supports', 'raft', 'ironing', 'fuzzy-skin', 'wipe-tower', 'gcode-export', 'gcode-processor']) {
      expect(names).toContain(n)
    }
    expect(names.filter((n) => n.startsWith('infill-')).length).toBe(30)
    expect(SPARSE_PATTERNS.length).toBe(26)
  })

  test('familiesFor reads the command and the settings', () => {
    const defaults = configDefaults({
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
      ],
    })
    expect(familiesFor('info', settingLookup(undefined, {}, defaults), 1)).toEqual(['model-load', 'placement', 'info'])
    expect(familiesFor('layers', settingLookup(undefined, {}, defaults), 1)).toEqual(['model-load', 'placement', 'slice-mesh'])
    const plain = familiesFor('slice', settingLookup(undefined, {}, defaults), 1)
    expect(plain).toContain('perimeters-arachne')
    expect(plain).toContain('infill-crosshatch')
    expect(plain).toContain('infill-monotonicline')
    expect(plain).toContain('gcode-export')
    expect(plain).not.toContain('supports')
    const lots = familiesFor('slice', settingLookup(undefined, { wall_generator: 'classic', enable_support: '1', support_type: 'tree(auto)', raft_layers: '2', ironing_type: 'top', fuzzy_skin: 'external', enable_prime_tower: '1', timelapse_type: '1', zaa_enabled: '1', sparse_infill_density: '0%' }, defaults), 1)
    for (const f of ['perimeters-classic', 'tree-supports', 'raft', 'ironing', 'fuzzy-skin', 'wipe-tower', 'step-contouring', 'infill-supportbase']) expect(lots).toContain(f)
    expect(lots).not.toContain('infill-crosshatch')
    expect(familiesFor('slice', settingLookup(undefined, { enable_prime_tower: '1' }, defaults), 1)).not.toContain('wipe-tower')
    expect(familiesFor('slice', settingLookup(undefined, { enable_prime_tower: '1' }, defaults), 2)).toContain('wipe-tower')
  })
})

describe('cases', () => {
  const cases = corpusCases(protocolDir, examples)

  test('the smoke cases, each mesh as info, layers and slice, the Sketch-shaped and family jobs, the invalid examples and the refusals', () => {
    const ids = cases.map((c) => c.id)
    expect(new Set(ids).size).toBe(ids.length)
    for (const id of ids) expect(id).toMatch(/^[a-z0-9][a-z0-9-]*$/)
    expect(cases.filter((c) => c.kind === 'smoke').length).toBe(SMOKE_CASES.length)
    expect(cases.filter((c) => c.id.startsWith('mesh-')).length).toBe(28 * 3)
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
    for (const c of cases.filter((x) => x.kind === 'example' || x.kind === 'refusal')) expect(c.families).toEqual(['intake'])
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
    }
    expect(doc.cases.map((c) => c.id)).toEqual(corpusCases(protocolDir, examples).map((c) => c.id))
    expect(doc.families.map((f) => f.name)).toEqual(FAMILIES.map((f) => f.name))
    for (const c of doc.cases) expect(c.families.length).toBeGreaterThan(0)
    for (const f of doc.families) expect(doc.cases.some((c) => c.families.includes(f.name))).toBe(true)
    expect(readFileSync(file, 'utf8')).not.toMatch(/[A-Za-z]:\\\\Users|\/home\//)
  })
})
