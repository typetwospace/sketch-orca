// The parity corpus (parity/corpus.json, parity/CORPUS.md): the jobs the Rust translation is held to
// the C++ engine on, and the families of Orca's work each case exercises. Four kinds of case:
//   smoke    each of the smoke run's cases (smoke.ts SMOKE_CASES) but `refusals`, run as the smoke
//            run runs it (the refusals are cases of their own, below);
//   job      a job written from the protocol's job schema (protocol/schema/job.schema.json): each of
//            Orca's 28 test meshes (<work>/src/tests/data) as an info, a layers and a slice job, and
//            the Sketch-shaped jobs (layer profiles, pauses, colour changes, height ranges, custom
//            G-code, a single-filament prime tower through smooth timelapse), and a job for each
//            family the presets leave off (each sparse infill pattern, both wall generators,
//            supports, tree supports, raft, ironing, fuzzy skin, Z contouring, the prime tower
//            clumping detection puts on with one filament, and a process preset whose
//            compatibility is a condition);
//            Phase O2 added the cases its families lacked: the 21 OBJ files read as OBJ, files
//            the corpus makes itself (3MF archives from its own zip writer, OBJ files with
//            texture coordinates, normals, quads and a material library, named by a short path,
//            by a long one and missing, and each reader's unreadable files), placement (several
//            objects, arrange off, excluded areas, a round bed, prime towers, printing by object,
//            far apart filament temperatures, per-object settings and height ranges, custom
//            G-code), preset conditions (among them a filament's condition for the process that
//            does not parse, the one parse error Orca prints on stdout) and orient jobs;
//            Phase O3 added the process jobs (processJobs): domain-A layers and slice jobs for
//            the families process() and the layers answer run (both wall generators, each
//            domain-A pattern, the elephant foot, polyholes, brim and skirt, several objects,
//            adaptive and variable layer heights, and a layer height above the nozzle refused),
//            and one job outside domain A, multiline infill, the one case of fill-multiline;
//   example  protocol/examples/invalid/job-*.json, its example paths moved into the case's folder;
//   refusal  each of the smoke run's refused jobs (smoke.ts REFUSALS), one case each, in the
//            families of the steps the C++ engine reaches before it refuses the job
//            (REFUSAL_FAMILIES): intake for a job it refuses while reading the job, the presets or
//            the model's file, else the steps of the model, the placement or the slice.
// The meshes stay in the work folder: `parity corpus` converts the 21 OBJ files to binary STL as
// Sketch writes model.stl (objToBinaryStl, CORPUS.md), copies them as they are too, copies the 6
// STL files and the 3MF as they are, writes the files the corpus makes (MADE_FILES) into
// <work>/parity/corpus/meshes/<mesh>/, and writes parity/corpus.json, which names each mesh's
// source and SHA-256. A case runs as a smoke case does (same.ts runEngineCases), so every output
// of every engine run is recorded the same way. Five cases are engine faults (ENGINE_FAULTS): the
// three of 20mmbox-CR.stl and the two of an OBJ whose material library's path is under 16 bytes;
// they are recorded and held to no family.
// Each case holds only the families its run can pass (withFamilies): a layers job every family
// process() runs and `layers`; a slice job those but `layers` (it declines after process() in
// phase O3), with the G-code's; a job that reaches a family outside domain A (MARKER_FAMILIES,
// read from its settings at every level and from its model's painting) only its markers and the
// front-objects families (domainACut), as the program declines it once its objects are placed.
// No Sketch code is in here.

import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { deflateRawSync } from 'node:zlib'
import { sha256Bytes } from '../files.ts'
import type { CustomGcode, Job, JobCommand, JobObject, Overrides } from '../job.ts'
import { cube, encodeBinaryStl, writeStl } from '../models.ts'
import { serializePreset, type PresetFiles, type PresetSet } from '../profiles.ts'
import { StepError } from '../run.ts'
import { cubeOverrides, PLAIN_OVERRIDES, REFUSALS, SMOKE_CASES, SMOKE_PRESETS, SMOKE_VERSION, type SmokeCase } from '../smoke.ts'
import { crc32, extractZipEntry, readZipEntries } from '../zip.ts'

export const CORPUS_FORMAT = 'sketch-orca-parity-corpus/1'

/** Orca's test meshes, below the engine's checkout (<work>/src). */
export const ORCA_TEST_DATA = 'tests/data'

/**
 * How a mesh reaches a case's folder: `obj`, an OBJ of Orca's converted to model.stl; `obj-file`,
 * an OBJ of Orca's as it is; `stl` and `3mf`, Orca's files as they are; `made`, a file the corpus
 * makes itself (MADE_FILES), whatever its format.
 */
export type MeshKind = 'obj' | 'obj-file' | 'stl' | '3mf' | 'made'

export interface CorpusMesh {
  id: string
  /** Below <work>/src/tests/data, with forward slashes; for a made file, `corpus.ts MADE_FILES <id>`. */
  source: string
  kind: MeshKind
  /** The model's file name in a case's folder: model.stl for a converted OBJ, else the source's own name. */
  file: string
  /** Files that go beside the model in a case's folder (an OBJ's material library), by name. */
  companions?: string[]
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

const objId = (n: string) => n.toLowerCase().replace(/_/g, '-')

/** Orca's 28 test meshes: 21 OBJ files (converted), 6 STL files and one 3MF (as they are). */
export const ORCA_MESHES: CorpusMesh[] = [
  ...OBJ_NAMES.map((n): CorpusMesh => ({ id: objId(n), source: `${n}.obj`, kind: 'obj', file: 'model.stl' })),
  { id: 'prusa-stl', source: 'test_3mf/Prusa.stl', kind: 'stl', file: 'Prusa.stl' },
  { id: '20mmbox-cr', source: 'test_stl/ASCII/20mmbox-CR.stl', kind: 'stl', file: '20mmbox-CR.stl' },
  { id: '20mmbox-crlf', source: 'test_stl/ASCII/20mmbox-CRLF.stl', kind: 'stl', file: '20mmbox-CRLF.stl' },
  { id: '20mmbox-lf', source: 'test_stl/ASCII/20mmbox-LF.stl', kind: 'stl', file: '20mmbox-LF.stl' },
  { id: '20mmbox-nonstandard', source: 'test_stl/ASCII/20mmbox-nonstandard.stl', kind: 'stl', file: '20mmbox-nonstandard.stl' },
  { id: '20mmbox-unicode', source: 'test_stl/Geräte/20mmbox-čřšřěá.stl', kind: 'stl', file: '20mmbox-čřšřěá.stl' },
  { id: 'buchse-3mf', source: 'test_3mf/Geräte/Büchse.3mf', kind: '3mf', file: 'Büchse.3mf' },
]

/** The 21 OBJ files again, as they are, so Orca's OBJ reader reads them (`obj-<mesh>`). */
export const OBJ_FILE_MESHES: CorpusMesh[] = OBJ_NAMES.map((n): CorpusMesh => ({ id: `obj-${objId(n)}`, source: `${n}.obj`, kind: 'obj-file', file: `${n}.obj` }))

/** The files the corpus makes itself (MADE_FILES), each under its file name in a case's folder. */
export const MADE_MESHES: CorpusMesh[] = [
  // OBJ files with what Orca's test meshes lack: texture coordinates, a material library (named
  // in under 16 bytes, in 16 or more, and missing), and quads, normals, negative indices, groups
  // and smoothing together.
  { id: 'obj-texcoords', source: 'corpus.ts MADE_FILES obj-texcoords', kind: 'made', file: 'texcoords.obj' },
  { id: 'obj-mtl', source: 'corpus.ts MADE_FILES obj-mtl', kind: 'made', file: 'painted.obj', companions: ['painted.mtl'] },
  { id: 'obj-mtl-long', source: 'corpus.ts MADE_FILES obj-mtl-long', kind: 'made', file: 'painted-long.obj', companions: ['painted-library.mtl'] },
  { id: 'obj-mtl-missing', source: 'corpus.ts MADE_FILES obj-mtl-missing', kind: 'made', file: 'unpainted.obj' },
  { id: 'obj-mixed', source: 'corpus.ts MADE_FILES obj-mixed', kind: 'made', file: 'mixed.obj' },
  // 3MF archives that load.
  { id: '3mf-two-objects', source: 'corpus.ts MADE_FILES 3mf-two-objects', kind: 'made', file: 'two-objects.3mf' },
  { id: '3mf-components-inch', source: 'corpus.ts MADE_FILES 3mf-components-inch', kind: 'made', file: 'components-inch.3mf' },
  { id: '3mf-bambu', source: 'corpus.ts MADE_FILES 3mf-bambu', kind: 'made', file: 'bambu.3mf' },
  { id: '3mf-only-modifiers', source: 'corpus.ts MADE_FILES 3mf-only-modifiers', kind: 'made', file: 'only-modifiers.3mf' },
  { id: '3mf-one-off-bed', source: 'corpus.ts MADE_FILES 3mf-one-off-bed', kind: 'made', file: 'one-off-bed.3mf' },
  { id: '3mf-not-printable', source: 'corpus.ts MADE_FILES 3mf-not-printable', kind: 'made', file: 'not-printable.3mf' },
  { id: '3mf-not-printable-off-bed', source: 'corpus.ts MADE_FILES 3mf-not-printable-off-bed', kind: 'made', file: 'not-printable-off-bed.3mf' },
  { id: '3mf-two-large', source: 'corpus.ts MADE_FILES 3mf-two-large', kind: 'made', file: 'two-large.3mf' },
  // Files each reader cannot read: broken archives, broken XML, and empty files.
  { id: '3mf-not-a-zip', source: 'corpus.ts MADE_FILES 3mf-not-a-zip', kind: 'made', file: 'not-a-zip.3mf' },
  { id: '3mf-zip-without-model', source: 'corpus.ts MADE_FILES 3mf-zip-without-model', kind: 'made', file: 'zip-without-model.3mf' },
  { id: '3mf-truncated', source: 'corpus.ts MADE_FILES 3mf-truncated', kind: 'made', file: 'truncated.3mf' },
  { id: '3mf-bad-crc', source: 'corpus.ts MADE_FILES 3mf-bad-crc', kind: 'made', file: 'bad-crc.3mf' },
  { id: '3mf-bad-model-xml', source: 'corpus.ts MADE_FILES 3mf-bad-model-xml', kind: 'made', file: 'bad-model-xml.3mf' },
  { id: '3mf-bad-rels-xml', source: 'corpus.ts MADE_FILES 3mf-bad-rels-xml', kind: 'made', file: 'bad-rels-xml.3mf' },
  { id: '3mf-bad-settings-xml', source: 'corpus.ts MADE_FILES 3mf-bad-settings-xml', kind: 'made', file: 'bad-settings-xml.3mf' },
  { id: '3mf-vertex-out-of-range', source: 'corpus.ts MADE_FILES 3mf-vertex-out-of-range', kind: 'made', file: 'vertex-out-of-range.3mf' },
  { id: '3mf-no-build-items', source: 'corpus.ts MADE_FILES 3mf-no-build-items', kind: 'made', file: 'no-build-items.3mf' },
  { id: '3mf-empty', source: 'corpus.ts MADE_FILES 3mf-empty', kind: 'made', file: 'empty.3mf' },
  { id: 'obj-empty', source: 'corpus.ts MADE_FILES obj-empty', kind: 'made', file: 'empty.obj' },
]

/** Every mesh of the corpus: Orca's 28, its 21 OBJ files as they are, and the files the corpus makes. */
export const CORPUS_MESHES: CorpusMesh[] = [...ORCA_MESHES, ...OBJ_FILE_MESHES, ...MADE_MESHES]

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

// ---------------------------------------------------------------- files the corpus makes

/** An entry of an archive writeZip writes: its name, its bytes, and whether it is stored rather than deflated. */
export interface ZipWriteEntry {
  name: string
  data: Buffer
  store?: boolean
}

/** The date writeZip gives every entry, in MS-DOS form: 1980-01-01, the first it can hold (time 00:00). */
export const ZIP_DOS_DATE = (0 << 9) | (1 << 5) | 1

/**
 * A zip archive of `entries`, in their order, the same bytes every time for the same entries and
 * zlib: each a local header (version 20, flags 0, or 0x800 for a name that is not ASCII, method 8
 * deflate through node:zlib's deflateRawSync at level 9, or 0 stored, the fixed date ZIP_DOS_DATE,
 * the CRC-32 and both sizes, no extra field) and its data, then the central directory with the same
 * fields (no comment, attributes 0) and the end record. No ZIP64: the corpus's files are small.
 */
export function writeZip(entries: ZipWriteEntry[]): Buffer {
  const parts: Buffer[] = []
  const central: Buffer[] = []
  let offset = 0
  for (const e of entries) {
    const name = Buffer.from(e.name, 'utf8')
    const body = e.store ? e.data : deflateRawSync(e.data, { level: 9 })
    const method = e.store ? 0 : 8
    const flags = /^[\x20-\x7e]*$/.test(e.name) ? 0 : 0x800
    const crc = crc32(e.data)
    const local = Buffer.alloc(30)
    local.writeUInt32LE(0x04034b50, 0)
    local.writeUInt16LE(20, 4)
    local.writeUInt16LE(flags, 6)
    local.writeUInt16LE(method, 8)
    local.writeUInt16LE(0, 10)
    local.writeUInt16LE(ZIP_DOS_DATE, 12)
    local.writeUInt32LE(crc, 14)
    local.writeUInt32LE(body.length, 18)
    local.writeUInt32LE(e.data.length, 22)
    local.writeUInt16LE(name.length, 26)
    local.writeUInt16LE(0, 28)
    const c = Buffer.alloc(46)
    c.writeUInt32LE(0x02014b50, 0)
    c.writeUInt16LE(20, 4)
    c.writeUInt16LE(20, 6)
    c.writeUInt16LE(flags, 8)
    c.writeUInt16LE(method, 10)
    c.writeUInt16LE(0, 12)
    c.writeUInt16LE(ZIP_DOS_DATE, 14)
    c.writeUInt32LE(crc, 16)
    c.writeUInt32LE(body.length, 20)
    c.writeUInt32LE(e.data.length, 24)
    c.writeUInt16LE(name.length, 28)
    c.writeUInt32LE(offset, 42)
    parts.push(local, name, body)
    central.push(c, name)
    offset += 30 + name.length + body.length
  }
  const dir = Buffer.concat(central)
  const end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50, 0)
  end.writeUInt16LE(entries.length, 8)
  end.writeUInt16LE(entries.length, 10)
  end.writeUInt32LE(dir.length, 12)
  end.writeUInt32LE(offset, 16)
  return Buffer.concat([...parts, dir, end])
}

type V3 = [number, number, number]

/** An indexed mesh, as a 3MF object holds one: vertices and triangles of vertex indices, each triangle's attributes (painting) by index. */
export interface IndexedMesh {
  vertices: V3[]
  triangles: [number, number, number][]
  paint?: Record<number, Record<string, string>>
}

/**
 * The box from `min` to `max` as 8 vertices and 12 outward triangles, in the order the probe's 3MF
 * fixtures use (parity/oracle/probe/fixtures/model): bottom, top, then the sides from the front.
 */
export function indexedBox(min: V3, max: V3, paint?: IndexedMesh['paint']): IndexedMesh {
  const [x0, y0, z0] = min
  const [x1, y1, z1] = max
  return {
    vertices: [
      [x0, y0, z0],
      [x1, y0, z0],
      [x1, y1, z0],
      [x0, y1, z0],
      [x0, y0, z1],
      [x1, y0, z1],
      [x1, y1, z1],
      [x0, y1, z1],
    ],
    triangles: [
      [3, 2, 1],
      [1, 0, 3],
      [4, 5, 6],
      [6, 7, 4],
      [0, 1, 5],
      [5, 4, 0],
      [1, 2, 6],
      [6, 5, 1],
      [2, 3, 7],
      [7, 6, 2],
      [3, 0, 4],
      [4, 7, 3],
    ],
    ...(paint ? { paint } : {}),
  }
}

const XML_HEAD = '<?xml version="1.0" encoding="UTF-8"?>\n'
const NS_CORE = 'http://schemas.microsoft.com/3dmanufacturing/core/2015/02'
const NS_PRODUCTION = 'http://schemas.microsoft.com/3dmanufacturing/production/2015/06'
const REL_3DMODEL = 'http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel'

/** A mesh element of a 3MF object. */
export function meshXml(m: IndexedMesh): string {
  const v = m.vertices.map(([x, y, z]) => `    <vertex x="${x}" y="${y}" z="${z}"/>\n`).join('')
  const t = m.triangles
    .map(([a, b, c], i) => {
      const attrs = Object.entries(m.paint?.[i] ?? {})
        .map(([k, val]) => ` ${k}="${val}"`)
        .join('')
      return `    <triangle v1="${a}" v2="${b}" v3="${c}"${attrs}/>\n`
    })
    .join('')
  return `  <mesh>\n   <vertices>\n${v}   </vertices>\n   <triangles>\n${t}   </triangles>\n  </mesh>\n`
}

/** A 3MF object: a mesh, or components (each another object's id, its transform, and its model file for a Bambu sub-model). */
export interface ModelObject3mf {
  id: number
  name?: string
  mesh?: IndexedMesh
  components?: { objectid: number; transform: string; path?: string }[]
  uuid?: string
}

/** A 3MF model file: its unit, metadata, objects and build items (an object's id, its transform and whether it prints). */
export function modelXml(o: { unit?: string; metadata?: [string, string][]; objects: ModelObject3mf[]; items: { objectid: number; transform?: string; printable?: boolean }[]; production?: boolean }): string {
  const ns = o.production ? ` xmlns:p="${NS_PRODUCTION}"` : ''
  const meta = (o.metadata ?? []).map(([k, v]) => `<metadata name="${k}">${v}</metadata>\n`).join('')
  const objects = o.objects
    .map((obj) => {
      const attrs = `id="${obj.id}"${obj.uuid ? ` p:UUID="${obj.uuid}"` : ''}${obj.name ? ` name="${obj.name}"` : ''} type="model"`
      const body = obj.mesh
        ? meshXml(obj.mesh)
        : `  <components>\n${(obj.components ?? []).map((c) => `   <component${c.path ? ` p:path="${c.path}"` : ''} objectid="${c.objectid}" transform="${c.transform}"/>\n`).join('')}  </components>\n`
      return ` <object ${attrs}>\n${body} </object>\n`
    })
    .join('')
  const items = o.items.map((i) => ` <item objectid="${i.objectid}"${i.transform ? ` transform="${i.transform}"` : ''}${i.printable === undefined ? '' : ` printable="${i.printable ? 1 : 0}"`}/>\n`).join('')
  return `${XML_HEAD}<model unit="${o.unit ?? 'millimeter'}" xml:lang="en-US" xmlns="${NS_CORE}"${ns}>\n${meta}<resources>\n${objects}</resources>\n<build>\n${items}</build>\n</model>\n`
}

/** A relationships file naming each model file as a 3D model part. */
export function relsXml(targets: string[]): string {
  return `${XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${targets.map((t, i) => `<Relationship Target="${t}" Id="rel-${i + 1}" Type="${REL_3DMODEL}"/>`).join('')}</Relationships>\n`
}

const CONTENT_TYPES = `${XML_HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/><Default Extension="config" ContentType="text/xml"/></Types>\n`

/** A plain 3MF archive: the content types, the root relationships, and one model file. */
export function plain3mf(model: string, extra: ZipWriteEntry[] = []): Buffer {
  return writeZip([
    { name: '[Content_Types].xml', data: Buffer.from(CONTENT_TYPES) },
    { name: '_rels/.rels', data: Buffer.from(relsXml(['/3D/3dmodel.model'])) },
    { name: '3D/3dmodel.model', data: Buffer.from(model) },
    ...extra,
  ])
}

/** A 3MF transform (the 3x3 rotation and scale by rows, then the translation) that moves by x, y, z, turned `quarterTurns` quarter turns about z. */
export function transform3mf(x: number, y: number, z: number, quarterTurns = 0): string {
  const [c, s] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
  ][((quarterTurns % 4) + 4) % 4]!
  return `${c} ${s} 0 ${-s} ${c} 0 0 0 1 ${x} ${y} ${z}`
}

const BAMBU_META: [string, string][] = [
  ['Application', 'BambuStudio-01.10.01.50'],
  ['BambuStudio:3mfVersion', '1'],
]

/**
 * A Bambu-style 3MF (as Bambu Studio writes one, and the probe's 3mf-3-bambu fixture): a root
 * model of objects made of components in sub-model files (3D/Objects/*.model, named in
 * 3D/_rels/3dmodel.model.rels), and Metadata/model_settings.config with each object's name and
 * filament, each part's name, type (normal_part or modifier_part) and filament, and the plate.
 */
export function bambu3mf(o: {
  objects: { id: number; name: string; extruder: number; parts: { id: number; name: string; subtype: 'normal_part' | 'modifier_part'; extruder: number; mesh: IndexedMesh; offset?: [number, number, number]; settings?: [string, string][] }[] }[]
  items: { objectid: number; transform: string; printable?: boolean }[]
  settings?: string
}): Buffer {
  const sub = o.objects.map((obj, k) => ({ path: `/3D/Objects/object_${k + 1}.model`, obj }))
  const root = modelXml({
    metadata: BAMBU_META,
    production: true,
    objects: sub.map(({ path, obj }) => ({
      id: obj.id,
      uuid: `0000000${obj.id}-61cb-4c03-9d28-80fed5dfa1dc`,
      components: obj.parts.map((p) => ({ objectid: p.id, path, transform: transform3mf(...(p.offset ?? [0, 0, 0])) })),
    })),
    items: o.items,
  })
  const subModels = sub.map(({ path, obj }) => ({
    name: path.slice(1),
    data: Buffer.from(
      modelXml({
        metadata: [BAMBU_META[1]!],
        production: true,
        objects: obj.parts.map((p) => ({ id: p.id, uuid: `000${p.id}0000-81cb-4c03-9d28-80fed5dfa1dc`, mesh: p.mesh })),
        items: [],
      }),
    ),
  }))
  const config =
    o.settings ??
    `${XML_HEAD}<config>\n${o.objects
      .map(
        (obj) =>
          ` <object id="${obj.id}">\n  <metadata key="name" value="${obj.name}"/>\n  <metadata key="extruder" value="${obj.extruder}"/>\n${obj.parts
            .map((p) => `  <part id="${p.id}" subtype="${p.subtype}">\n   <metadata key="name" value="${p.name}"/>\n${(p.settings ?? []).map(([k, v]) => `   <metadata key="${k}" value="${v}"/>\n`).join('')}   <metadata key="extruder" value="${p.extruder}"/>\n  </part>\n`)
            .join('')} </object>\n`,
      )
      .join('')} <plate>\n  <metadata key="plater_id" value="1"/>\n  <metadata key="plater_name" value=""/>\n  <metadata key="locked" value="false"/>\n${o.items
      .map((it, i) => `  <model_instance>\n   <metadata key="object_id" value="${it.objectid}"/>\n   <metadata key="instance_id" value="0"/>\n   <metadata key="identify_id" value="${100 + i}"/>\n  </model_instance>\n`)
      .join('')} </plate>\n</config>\n`
  return writeZip([
    { name: '[Content_Types].xml', data: Buffer.from(CONTENT_TYPES) },
    { name: '_rels/.rels', data: Buffer.from(relsXml(['/3D/3dmodel.model'])) },
    { name: '3D/3dmodel.model', data: Buffer.from(root) },
    { name: '3D/_rels/3dmodel.model.rels', data: Buffer.from(relsXml(sub.map((s) => s.path))) },
    ...subModels,
    { name: 'Metadata/model_settings.config', data: Buffer.from(config) },
  ])
}

/** The two objects of 3mf-two-objects: a 20 mm cube twice (two build items) and a 30 x 10 x 5 mm block turned a quarter turn. */
function twoObjectsModel(): string {
  return modelXml({
    metadata: [['Title', 'two objects']],
    objects: [
      { id: 1, name: 'cube', mesh: indexedBox([0, 0, 0], [20, 20, 20]) },
      { id: 2, name: 'block', mesh: indexedBox([0, 0, 0], [30, 10, 5]) },
    ],
    items: [
      { objectid: 1, transform: transform3mf(60, 60, 0) },
      { objectid: 1, transform: transform3mf(110, 60, 0) },
      { objectid: 2, transform: transform3mf(170, 150, 0, 1) },
    ],
  })
}

/** The Bambu-style project of 3mf-bambu: painting on its body (colour, supports, seam, fuzzy skin), as the probe's fixture paints it. */
const BAMBU_PAINT: IndexedMesh['paint'] = { 0: { paint_supports: '4' }, 1: { paint_seam: '8' }, 2: { paint_color: '8' }, 3: { paint_fuzzy_skin: '4' }, 4: { paint_color: '48' } }

function bambuProject(settings?: string): Buffer {
  return bambu3mf({
    objects: [
      {
        id: 3,
        name: 'Bracket',
        extruder: 1,
        parts: [
          { id: 1, name: 'body', subtype: 'normal_part', extruder: 2, mesh: indexedBox([0, 0, 0], [20, 20, 10], BAMBU_PAINT) },
          { id: 2, name: 'denser', subtype: 'modifier_part', extruder: 0, mesh: indexedBox([0, 0, 0], [6, 6, 6]), offset: [5, 5, 2], settings: [['sparse_infill_density', '80%']] },
        ],
      },
      { id: 5, name: 'Spare', extruder: 1, parts: [{ id: 4, name: 'spare', subtype: 'normal_part', extruder: 1, mesh: indexedBox([0, 0, 0], [10, 10, 10]) }] },
    ],
    items: [
      { objectid: 3, transform: transform3mf(100, 100, 0), printable: true },
      { objectid: 5, transform: transform3mf(200, 150, 0), printable: false },
    ],
    settings,
  })
}

/** An OBJ of the 20 mm cube in its 8 corners, with `faces` (the face lines) and what comes before them (`head`). */
function cubeObj(head: string, faces: string): string {
  return `${head}v 0 0 0\nv 20 0 0\nv 20 20 0\nv 0 20 0\nv 0 0 20\nv 20 0 20\nv 20 20 20\nv 0 20 20\n${faces}`
}

/** The cube's six faces as quads, outward, by corner number. */
const CUBE_QUADS: [number, number, number, number][] = [
  [1, 4, 3, 2],
  [5, 6, 7, 8],
  [1, 2, 6, 5],
  [2, 3, 7, 6],
  [3, 4, 8, 7],
  [4, 1, 5, 8],
]

/** The cube in two materials, red and blue, naming the material library `library`. */
function paintedObj(library: string): string {
  return cubeObj(`# the 20 mm cube in two materials\nmtllib ${library}\n`, `usemtl red\n${CUBE_QUADS.slice(0, 3).map((q) => `f ${q.join(' ')}\n`).join('')}usemtl blue\n${CUBE_QUADS.slice(3).map((q) => `f ${q.join(' ')}\n`).join('')}`)
}

/** The material library of paintedObj: its two colours. */
const PAINTED_MTL = '# two colours\nnewmtl red\nKd 0.8 0.1 0.1\nnewmtl blue\nKd 0.1 0.2 0.8\n'

/**
 * The bytes of each made file (MADE_MESHES), by mesh id: the file, and its companions by name.
 * Each is the same bytes every time (writeZip's for the archives).
 */
export const MADE_FILES: Record<string, () => Record<string, Buffer>> = {
  'obj-texcoords': () => ({
    'texcoords.obj': Buffer.from(
      cubeObj(
        '# the 20 mm cube with texture coordinates\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n',
        CUBE_QUADS.map((q) => `f ${q[0]}/1 ${q[1]}/2 ${q[2]}/3\nf ${q[0]}/1 ${q[2]}/3 ${q[3]}/4\n`).join(''),
      ),
    ),
  }),
  // The library's name is under 16 bytes (painted.mtl, 11): the C++ engine fails to load it (an
  // engine fault, ENGINE_FAULTS).
  'obj-mtl': () => ({
    'painted.obj': Buffer.from(paintedObj('painted.mtl')),
    'painted.mtl': Buffer.from(PAINTED_MTL),
  }),
  // The same cube and library, the library's name 16 bytes or more (painted-library.mtl, 19): the
  // C++ engine loads it.
  'obj-mtl-long': () => ({
    'painted-long.obj': Buffer.from(paintedObj('painted-library.mtl')),
    'painted-library.mtl': Buffer.from(PAINTED_MTL),
  }),
  // The same cube naming a material library that is not there: load_obj logs it and reads on. The
  // log line's path goes through the same dangling pointer (Format/OBJ.cpp:65); it is the case
  // folder and the name, 16 bytes or more, so it reads whole.
  'obj-mtl-missing': () => ({
    'unpainted.obj': Buffer.from(
      cubeObj('# the 20 mm cube naming a material library that is missing\nmtllib missing.mtl\n', `usemtl red\n${CUBE_QUADS.map((q) => `f ${q.join(' ')}\n`).join('')}`),
    ),
  }),
  'obj-mixed': () => ({
    'mixed.obj': Buffer.from(
      cubeObj(
        '# quads, normals, texture coordinates, negative indices, groups and smoothing\no mixed\ng body\ns 1\nvn 0 0 -1\nvn 0 0 1\nvt 0.5 0.5\n',
        `${CUBE_QUADS.slice(0, 2)
          .map((q, i) => `f ${q.map((c) => `${c}/1/${i + 1}`).join(' ')}\n`)
          .join('')}s off\n${CUBE_QUADS.slice(2, 4)
          .map((q) => `f ${q.map((c) => `${c}//1`).join(' ')}\n`)
          .join('')}g side\n${CUBE_QUADS.slice(4)
          .map((q) => `f ${q.map((c) => String(c - 9)).join(' ')}\n`)
          .join('')}`,
      ),
    ),
  }),
  '3mf-two-objects': () => ({ 'two-objects.3mf': plain3mf(twoObjectsModel()) }),
  '3mf-components-inch': () => ({
    'components-inch.3mf': plain3mf(
      modelXml({
        unit: 'inch',
        metadata: [['Title', 'components in inches']],
        objects: [
          { id: 1, mesh: indexedBox([0, 0, 0], [0.5, 0.5, 0.5]) },
          { id: 2, name: 'bar', mesh: indexedBox([0, 0, 0], [1, 0.25, 0.2]) },
          {
            id: 3,
            name: 'assembly',
            components: [
              { objectid: 1, transform: transform3mf(1.25, 0, 0) },
              { objectid: 2, transform: transform3mf(0, 0, 0.5, 1) },
            ],
          },
        ],
        items: [{ objectid: 3, transform: transform3mf(4, 4, 0) }],
      }),
    ),
  }),
  '3mf-bambu': () => ({ 'bambu.3mf': bambuProject() }),
  '3mf-only-modifiers': () => ({
    'only-modifiers.3mf': bambu3mf({
      objects: [{ id: 3, name: 'Ghost', extruder: 1, parts: [{ id: 1, name: 'modifier', subtype: 'modifier_part', extruder: 0, mesh: indexedBox([0, 0, 0], [20, 20, 10]) }] }],
      items: [{ objectid: 3, transform: transform3mf(100, 100, 0) }],
    }),
  }),
  '3mf-one-off-bed': () => ({
    'one-off-bed.3mf': plain3mf(
      modelXml({
        objects: [
          { id: 1, name: 'inside', mesh: indexedBox([0, 0, 0], [20, 20, 20]) },
          { id: 2, name: 'over the edge', mesh: indexedBox([0, 0, 0], [20, 20, 20]) },
        ],
        items: [
          { objectid: 1, transform: transform3mf(50, 50, 0) },
          { objectid: 2, transform: transform3mf(290, 50, 0) },
        ],
      }),
    ),
  }),
  '3mf-not-printable': () => ({
    'not-printable.3mf': plain3mf(
      modelXml({
        objects: [
          { id: 1, name: 'first', mesh: indexedBox([0, 0, 0], [20, 20, 20]) },
          { id: 2, name: 'second', mesh: indexedBox([0, 0, 0], [10, 10, 10]) },
        ],
        items: [
          { objectid: 1, transform: transform3mf(50, 50, 0), printable: false },
          { objectid: 2, transform: transform3mf(150, 150, 0), printable: false },
        ],
      }),
    ),
  }),
  // The same two objects not printable and outside the bed, so that no instance is inside the
  // volume: check_build_volume's own empty-plate (cli/run.cpp), before the slice.
  '3mf-not-printable-off-bed': () => ({
    'not-printable-off-bed.3mf': plain3mf(
      modelXml({
        objects: [
          { id: 1, name: 'first', mesh: indexedBox([0, 0, 0], [20, 20, 20]) },
          { id: 2, name: 'second', mesh: indexedBox([0, 0, 0], [10, 10, 10]) },
        ],
        items: [
          { objectid: 1, transform: transform3mf(-100, -100, 0), printable: false },
          { objectid: 2, transform: transform3mf(400, 400, 0), printable: false },
        ],
      }),
    ),
  }),
  '3mf-two-large': () => ({
    'two-large.3mf': plain3mf(
      modelXml({
        objects: [
          { id: 1, name: 'large', mesh: indexedBox([0, 0, 0], [200, 200, 10]) },
          { id: 2, name: 'larger', mesh: indexedBox([0, 0, 0], [210, 200, 10]) },
        ],
        items: [
          { objectid: 1, transform: transform3mf(50, 50, 0) },
          { objectid: 2, transform: transform3mf(40, 50, 0) },
        ],
      }),
    ),
  }),
  '3mf-not-a-zip': () => ({ 'not-a-zip.3mf': Buffer.from('this is not a zip archive\n') }),
  '3mf-zip-without-model': () => ({ 'zip-without-model.3mf': writeZip([{ name: 'readme.txt', data: Buffer.from('a zip archive with no 3D model in it\n') }]) }),
  '3mf-truncated': () => {
    const whole = plain3mf(twoObjectsModel())
    return { 'truncated.3mf': whole.subarray(0, Math.floor(whole.length / 2)) }
  },
  '3mf-bad-crc': () => {
    // The model file stored, one byte of it changed after its CRC-32 was taken.
    const model = Buffer.from(twoObjectsModel())
    const zip = writeZip([
      { name: '[Content_Types].xml', data: Buffer.from(CONTENT_TYPES) },
      { name: '_rels/.rels', data: Buffer.from(relsXml(['/3D/3dmodel.model'])) },
      { name: '3D/3dmodel.model', data: model, store: true },
    ])
    const at = zip.indexOf(model) + model.indexOf('two objects')
    zip[at] = 'T'.charCodeAt(0)
    return { 'bad-crc.3mf': zip }
  },
  '3mf-bad-model-xml': () => ({ 'bad-model-xml.3mf': plain3mf(twoObjectsModel().replace('</resources>', '</resource>')) }),
  '3mf-bad-rels-xml': () => ({
    'bad-rels-xml.3mf': writeZip([
      { name: '[Content_Types].xml', data: Buffer.from(CONTENT_TYPES) },
      { name: '_rels/.rels', data: Buffer.from(relsXml(['/3D/3dmodel.model']).replace('</Relationships>', '<Relationships>')) },
      { name: '3D/3dmodel.model', data: Buffer.from(twoObjectsModel()) },
    ]),
  }),
  '3mf-bad-settings-xml': () => ({ 'bad-settings-xml.3mf': bambuProject(`${XML_HEAD}<config>\n <object id="3">\n  <metadata key="name" value="Bracket"\n</config>\n`) }),
  '3mf-vertex-out-of-range': () => {
    const box = indexedBox([0, 0, 0], [20, 20, 20])
    box.triangles[11] = [4, 7, 99]
    return { 'vertex-out-of-range.3mf': plain3mf(modelXml({ objects: [{ id: 1, mesh: box }], items: [{ objectid: 1, transform: transform3mf(50, 50, 0) }] })) }
  },
  '3mf-no-build-items': () => ({ 'no-build-items.3mf': plain3mf(modelXml({ objects: [{ id: 1, mesh: indexedBox([0, 0, 0], [20, 20, 20]) }], items: [] })) }),
  '3mf-empty': () => ({ 'empty.3mf': Buffer.alloc(0) }),
  'obj-empty': () => ({ 'empty.obj': Buffer.alloc(0) }),
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
  { name: 'intake', step: 'front end', what: "reading and checking a job, its presets and its model's file, and the refusals" },
  { name: 'preset-conditions', step: 'presets', what: "a preset's compatibility condition (compatible_printers_condition, compatible_prints_condition), worked out for the job's printer and process" },
  { name: 'model-load', step: 'load', what: "reading the model (STL, OBJ, 3MF) and Orca's mesh repairs" },
  { name: 'orient', step: 'orient', what: 'orienting the objects' },
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
  { name: 'wipe-tower', step: 'psWipeTower', what: 'the prime tower' },
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
  // Phase O3's, after every family before them so parity.json keeps its order; each names the
  // step where it runs.
  { name: 'tool-ordering', step: 'psWipeTower', what: 'the tool ordering of a print by layer with no prime tower: the filaments of each layer and the custom G-code placed among them (ToolOrdering)' },
  { name: 'support-check', step: 'posSupportMaterial', what: 'with supports off, the check whether an object needs supports (is_support_necessary, its warning)' },
  { name: 'multi-filament', step: 'psWipeTower', what: 'more than one filament: tool changes, flushing and the filament checks (phase O5)' },
  { name: 'painted-facets', step: 'posSlice', what: 'a model with painted facets: colour, support, seam or fuzzy-skin painting (phase O5)' },
  { name: 'print-by-object', step: 'psSkirtBrim', what: 'printing object by object (print_sequence by object; phase O7)' },
  { name: 'fill-multiline', step: 'posInfill', what: 'multiline infill: each sparse infill line printed as several (fill_multiline above 1)' },
  { name: 'layers', step: 'layers', what: "the layers answer: the result's layer list and printZ, written once print.process() has returned (cli/run.cpp)" },
]

/** The families phase O3 certifies: what process() and the layers answer run in domain A. */
export const O3_FAMILIES = [
  'slice-mesh',
  'perimeters-classic',
  'perimeters-arachne',
  'infill-monotonic',
  'infill-monotonicline',
  'infill-rectilinear',
  'infill-crosshatch',
  'infill-concentric',
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
  'layers',
] as const

/** The fill patterns of domain A: those phase O3 translates. Every other pattern marks a job outside it. */
export const DOMAIN_A_PATTERNS = ['monotonic', 'monotonicline', 'rectilinear', 'crosshatch', 'concentric', 'concentricinternal'] as const

/**
 * The families that mark a job outside domain A (one filament, printed by layer, no prime tower,
 * supports, raft, ironing, fuzzy skin, Z contouring or painted facets, the domain-A patterns, one
 * line of sparse infill): the program declines such a job by name once its objects are placed.
 */
export const MARKER_FAMILIES: string[] = [
  ...INFILL_PATTERNS.filter((p) => !(DOMAIN_A_PATTERNS as readonly string[]).includes(p)).map((p) => `infill-${p}`),
  'supports',
  'tree-supports',
  'raft',
  'ironing',
  'fuzzy-skin',
  'wipe-tower',
  'step-contouring',
  'step-simplify-support-path',
  'multi-filament',
  'painted-facets',
  'print-by-object',
  'fill-multiline',
]

/** The families the front-objects boundary passes on a declined run (certify.ts BOUNDARY_FAMILIES['front-objects']). */
export const FRONT_OBJECTS_FAMILIES = ['preset-conditions', 'model-load', 'placement'] as const

/**
 * Domain A's cut: a job that reaches a marker (MARKER_FAMILIES) is declined once its objects are
 * placed, so it holds only its markers and the front-objects families it lists, never a family of
 * process(); any other job keeps `families` as they are.
 */
export function domainACut(families: string[]): string[] {
  if (!families.some((f) => MARKER_FAMILIES.includes(f))) return families
  return families.filter((f) => MARKER_FAMILIES.includes(f) || (FRONT_OBJECTS_FAMILIES as readonly string[]).includes(f))
}

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

/** How many points a list of them holds as a job sees it (settingPoints). */
export type SettingPoints = (key: string) => number

/**
 * How many pieces Orca's reader of a list of points (ConfigOptionPoints::deserialize, a
 * std::getline at each comma) splits `text` into: none for empty text, and none after a closing
 * comma (crates/orca-cli/src/decline.rs comma_pieces).
 */
export function commaPieces(text: string): number {
  return text === '' ? 0 : text.split(',').length - (text.endsWith(',') ? 1 : 0)
}

/**
 * The points of a list of them (wrapping_exclude_area), looked up as settingLookup looks a setting
 * up, which gives only an array's first value: the job's override and a preset's string in
 * commaPieces, a preset's array by its length, else Orca's default in commaPieces.
 */
export function settingPoints(set: PresetSet | undefined, overrides: Overrides, defaults: Map<string, string>): SettingPoints {
  return (key) => {
    if (key in overrides) return commaPieces(overrides[key] ?? '')
    for (const p of set ? [set.process, set.machine, ...set.filaments] : []) {
      const v = (p as Record<string, unknown>)[key]
      if (v !== undefined) return Array.isArray(v) ? v.length : commaPieces(String(v))
    }
    return commaPieces(defaults.get(key) ?? '')
  }
}

/** Orca's defaults by key, from the probe's configdef.json (each option's `default`). */
export function configDefaults(configdef: unknown): Map<string, string> {
  const out = new Map<string, string>()
  const options = (configdef as { options?: { optKey: string; default?: unknown }[] }).options ?? []
  for (const o of options) if (o.default !== undefined && o.default !== null) out.set(o.optKey, Array.isArray(o.default) ? String(o.default[0]) : String(o.default))
  return out
}

/**
 * Every step of process() that runs whatever the settings (Print.cpp process): the object steps,
 * the tool ordering (psWipeTower, for a print by layer with no prime tower), the skirt and brim and
 * the conflict check.
 */
const PROCESS_STEPS = ['step-estimate-curled-extrusions', 'step-prepare-infill', 'step-simplify-path', 'step-detect-overhangs-for-lift', 'step-simplify-wall', 'step-simplify-infill', 'step-skirt-brim', 'step-conflict-check', 'tool-ordering']

const PATTERN_KEYS = new Map<string, string>(INFILL_PATTERNS.map((p) => [p, `infill-${p}`]))

/**
 * The families one level of a job's settings turns on (the job's own, an object's or a height
 * range's, `get` reading each through the levels above it): the wall generator; the top, bottom
 * and internal solid patterns, and the sparse pattern when the density is above 0; supports, as
 * PrintObject::has_support has them (enable_support, or enforce_support_layers above 0), with
 * their simplification and base pattern, else the support check that runs without them
 * (generate_support_material's is_support_necessary); a raft, ironing, fuzzy skin, Z contouring;
 * the prime tower as Print::has_wipe_tower has it (two filaments or more, smooth timelapse, or
 * clumping detection with an exclusion area of more than two points, `points`); printing by
 * object (print_sequence); and multiline infill (fill_multiline above 1 where sparse infill is).
 */
export function settingFamilies(get: SettingLookup, filaments: number, points: SettingPoints): string[] {
  const out = new Set<string>()
  out.add(get('wall_generator') === 'classic' ? 'perimeters-classic' : 'perimeters-arachne')
  const density = parseFloat(get('sparse_infill_density') ?? '0')
  const patterns = [get('top_surface_pattern'), get('bottom_surface_pattern'), get('internal_solid_infill_pattern'), ...(density > 0 ? [get('sparse_infill_pattern')] : [])]
  for (const p of patterns) if (p && PATTERN_KEYS.has(p)) out.add(PATTERN_KEYS.get(p)!)
  if (get('enable_support') === '1' || Number(get('enforce_support_layers') ?? '0') > 0) {
    out.add((get('support_type') ?? '').startsWith('tree') ? 'tree-supports' : 'supports')
    out.add('step-simplify-support-path')
    out.add('infill-supportbase')
  } else out.add('support-check')
  if (Number(get('raft_layers') ?? '0') > 0) out.add('raft')
  if ((get('ironing_type') ?? 'no ironing') !== 'no ironing') out.add('ironing')
  if (!['none', 'disabled_fuzzy', undefined].includes(get('fuzzy_skin'))) out.add('fuzzy-skin')
  const wrapping = get('enable_wrapping_detection') === '1' && points('wrapping_exclude_area') > 2
  if (get('enable_prime_tower') === '1' && (filaments > 1 || get('timelapse_type') === '1' || wrapping)) out.add('wipe-tower')
  if (get('zaa_enabled') === '1') out.add('step-contouring')
  if (get('print_sequence') === 'by object') out.add('print-by-object')
  if (density > 0 && Number(get('fill_multiline') ?? '1') > 1) out.add('fill-multiline')
  return [...out]
}

/**
 * The setting lookups of a job's objects and height ranges, each over the level above it: an
 * object's overrides over the job's settings (`get`), a height range's over its object's. A 3MF's
 * own settings are not among them: cli/run.cpp load_model keeps only the filament (extruder) of
 * each object and part, and drops the project's height ranges and layer heights.
 */
export function objectLookups(get: SettingLookup, objects: JobObject[]): SettingLookup[] {
  const over = (overrides: Overrides, below: SettingLookup): SettingLookup => (key) => (key in overrides ? overrides[key] : below(key))
  return objects.flatMap((o) => {
    const object = over(o.overrides, get)
    return [object, ...o.heightRanges.map((r) => over(r.overrides, object))]
  })
}

/** What a job's model gives familiesFor besides its settings. */
export interface JobModel {
  /** The setting lookups of its objects and height ranges (objectLookups). */
  levels?: SettingLookup[]
  /** Whether the model has painted facets (modelPainting): colour, support, seam or fuzzy-skin painting. */
  painted?: boolean
}

/**
 * The families a job exercises, as its command, settings and model name them: model-load always;
 * orient when the job orients its objects (`orient`); placement for layers and slice (an info job
 * is answered before the objects are placed: cli/run.cpp run). A layers or slice job runs
 * process(): slice-mesh, the families its settings turn on at every level (settingFamilies: the
 * job's, each object's and each height range's), multi-filament with more than one filament,
 * painted-facets for a painted model, the pattern Fill.cpp gives narrow internal solid infill
 * (concentricinternal), and every step process() runs; a layers job then the layers answer
 * (`layers`), and a slice job the G-code (gcode-export, gcode-processor). A slice job holds no
 * `layers`: in phase O3 it declines after process(), before cli/run.cpp writes the layer list and
 * printZ, so no slice run passes it.
 */
export function familiesFor(command: JobCommand, get: SettingLookup, filaments: number, points: SettingPoints, orient = false, model: JobModel = {}): string[] {
  const out = new Set<string>(['model-load'])
  if (orient) out.add('orient')
  if (command !== 'info') out.add('placement')
  if (command === 'info') out.add('info')
  if (command === 'layers' || command === 'slice') {
    out.add('slice-mesh')
    for (const level of [get, ...(model.levels ?? [])]) for (const f of settingFamilies(level, filaments, points)) out.add(f)
    if (filaments > 1) out.add('multi-filament')
    if (model.painted) out.add('painted-facets')
    // Fill.cpp turns narrow internal solid infill into this pattern on any slice that has some.
    out.add('infill-concentricinternal')
    for (const s of PROCESS_STEPS) out.add(s)
    if (command === 'layers') out.add('layers')
    else {
      out.add('gcode-export')
      out.add('gcode-processor')
    }
  }
  return inFamilyOrder(out)
}

/** The triangle attributes Orca's 3MF reader takes as painting (Format/bbs_3mf.cpp: supports, seam, colour, fuzzy skin). */
export const PAINT_ATTRIBUTES = ['paint_supports', 'paint_seam', 'paint_color', 'paint_fuzzy_skin'] as const

/**
 * The painting a model's file holds, as Orca reads it from the command line: for a 3MF
 * (Model::read_from_file's load_bbs_3mf), each of PAINT_ATTRIBUTES that some triangle of a model
 * part gives a value that is not empty (set_triangle_from_string then fills that facets set, so
 * the volume is painted); none for an STL, and none for an OBJ, whose colours never paint on the
 * command line. An archive that cannot be read has none: its job stops at model-load.
 */
export function modelPainting(file: string, bytes: Buffer): string[] {
  if (!file.toLowerCase().endsWith('.3mf')) return []
  const found = new Set<string>()
  try {
    for (const e of readZipEntries(bytes)) {
      if (!e.name.toLowerCase().endsWith('.model')) continue
      const text = extractZipEntry(bytes, e).toString('utf8')
      for (const a of PAINT_ATTRIBUTES) if (new RegExp(`\\s${a}="[^"]+"`).test(text)) found.add(a)
    }
  } catch {
    return []
  }
  return PAINT_ATTRIBUTES.filter((a) => found.has(a))
}

/** The meshes whose model is painted (an entry's `painting`), by id. */
export function paintedMeshes(meshes: { id: string; painting?: string[] }[]): Set<string> {
  return new Set(meshes.filter((m) => (m.painting ?? []).length > 0).map((m) => m.id))
}

/** The painted meshes among the files the corpus makes (MADE_FILES), as buildMeshes finds them. */
export function madePaintedMeshes(): Set<string> {
  return new Set(MADE_MESHES.filter((m) => modelPainting(m.file, MADE_FILES[m.id]!()[m.file]!).length > 0).map((m) => m.id))
}

/** Families once each, in parity.json's order (FAMILIES). */
export function inFamilyOrder(families: Iterable<string>): string[] {
  const order = new Map(FAMILIES.map((f, i) => [f.name, i]))
  return [...new Set(families)].sort((a, b) => order.get(a)! - order.get(b)!)
}

/**
 * A preset's compatibility as Orca has it (Preset.cpp is_compatible_with_printer and
 * is_compatible_with_print): `undefined` where Orca works its condition out (a condition, no
 * list), else whether its list takes one of `names` (true with no list or no name).
 */
function compatibleByList(preset: Record<string, unknown>, list: string, condition: string, names: string[]): boolean | undefined {
  const l = Array.isArray(preset[list]) ? (preset[list] as unknown[]) : []
  if (l.length === 0) return typeof preset[condition] === 'string' && preset[condition] !== '' ? undefined : true
  return names[0] === '' || names.some((n) => l.includes(n))
}

/**
 * The families a job's presets reach besides those of its settings: preset-conditions where Orca
 * works a compatibility condition out, in the order crates/orca-cli/src/presets.rs checks them
 * (and declines at the first): the process's with the printer (a process the printer's list
 * leaves out is refused there), then each filament's with the printer and, when that holds, with
 * the process.
 */
export function presetFamilies(set: PresetSet): string[] {
  const name = (p: Record<string, unknown>) => (typeof p.name === 'string' ? p.name : '')
  const machine = set.machine as Record<string, unknown>
  const printer = [name(machine), ...(machine.from !== 'system' && typeof machine.inherits === 'string' ? [machine.inherits] : [])]
  const process = compatibleByList(set.process, 'compatible_printers', 'compatible_printers_condition', printer)
  if (process === undefined) return ['preset-conditions']
  if (!process) return []
  for (const f of set.filaments) {
    const forPrinter = compatibleByList(f, 'compatible_printers', 'compatible_printers_condition', printer)
    if (forPrinter === undefined) return ['preset-conditions']
    if (forPrinter && compatibleByList(f, 'compatible_prints', 'compatible_prints_condition', [name(set.process)]) === undefined) return ['preset-conditions']
  }
  return []
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
  /** How many of its set's filaments it uses, the first ones (all of them when left out). */
  filaments?: number
  /** Settings written over its set's process preset, in the case's own copy of it (process.json). */
  processChanges?: Record<string, unknown>
  /** Settings written over each of its filament presets, in the case's own copies (filament_<n>.json, from 1). */
  filamentChanges?: Record<string, unknown>
}

/** The preset set a job template uses: its set's first `filaments`, its process and filaments with their changes. */
export function templateSet(t: JobTemplate, sets: Record<PresetKey, PresetSet>): PresetSet {
  const set = sets[t.presets]
  const filaments = t.filaments === undefined ? set.filaments : set.filaments.slice(0, t.filaments)
  return {
    machine: set.machine,
    process: t.processChanges ? { ...set.process, ...t.processChanges } : set.process,
    filaments: t.filamentChanges ? filaments.map((f) => ({ ...f, ...t.filamentChanges })) : filaments,
  }
}

/**
 * The preset files a job template runs with in `dir`: its set's files, its first `filaments`, and
 * with process or filament changes the case's own copies of those presets (dir/process.json,
 * dir/filament_<n>.json).
 */
export function templateFiles(t: JobTemplate, files: PresetFiles, sets: Record<PresetKey, PresetSet>, dir: string): PresetFiles {
  const own = templateSet(t, sets)
  let process = files.process
  if (t.processChanges) {
    process = join(dir, 'process.json')
    writeFileSync(process, serializePreset(own.process))
  }
  let filaments = t.filaments === undefined ? files.filaments : files.filaments.slice(0, t.filaments)
  if (t.filamentChanges) {
    filaments = own.filaments.map((f, i) => {
      const path = join(dir, `filament_${i + 1}.json`)
      writeFileSync(path, serializePreset(f))
      return path
    })
  }
  return { machine: files.machine, process, filaments }
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
  /**
   * job: the family the C++ engine refuses the job in (one of STOP_ORDER): the
   * case is held only to the families up to it, as a refusal is (REFUSAL_FAMILIES).
   */
  stopsAt?: string
  /** An engine fault (ENGINE_FAULTS): why the case is recorded and held to no family. */
  fault?: string
}

/** The families a job reaches before it slices, in run()'s order (cli/run.cpp), for a job's `stopsAt`. */
export const STOP_ORDER = ['preset-conditions', 'model-load', 'orient', 'placement'] as const

/**
 * The cases that are engine faults (CORPUS.md, "An engine fault", decided 2026-11-06): the C++
 * engine dies of undefined behaviour while it loads 20mmbox-CR.stl (a solid line of 3200 bytes into
 * admesh's 256-byte buffer), so the Rust translation answers with a defined result instead
 * (orca-mesh's stl_open refuses an overlong solid line; the program says model-unreadable).
 * They stay in the corpus, so record.json shows any change in the C++ engine there, and are taken
 * out of their families: no family is certified or declined on them.
 */
export const ENGINE_FAULTS: Record<string, string> = Object.fromEntries([
  ...(['info', 'layers', 'slice'] as const).map((c) => [
    `mesh-20mmbox-cr-${c}`,
    "an engine fault: the C++ engine overruns admesh's 256-byte solid name on a file of CR line ends and dies while it loads the model (exit 9, no result); the Rust program refuses the file as model-unreadable instead (CORPUS.md)",
  ]),
  // load_obj (Format/OBJ.cpp:56) keeps c_str() of a temporary path string and reads the library's
  // path through it once the string is gone. A case runs in its own folder, so a library beside
  // the OBJ is found by its bare name. painted.mtl is 11 bytes: a path under 16 bytes reads as
  // empty there, so mtlparse opens "" and the load fails (model-unreadable, "load mtl in obj:
  // failed to parse", the log line with an empty path). The same OBJ with a library path of 16
  // bytes or more loads (model-obj-mtl-long), as the freed block still holds the path. An answer
  // that changes with nothing but the length of a path is the fault (CORPUS.md); what the engine
  // gives at 16 bytes or more, the same on every pass, is the reference, so model-obj-mtl-long and
  // model-obj-mtl-missing are held to their families.
  ...(['info', 'layers'] as const).map((c) => [
    `model-obj-mtl-${c}`,
    'an engine fault: the C++ engine reads its material library\'s path through a dangling c_str() of a temporary string (Format/OBJ.cpp load_obj); a path under 16 bytes, as painted.mtl found by its bare name in the case\'s folder is, reads as empty, so mtlparse opens "" and the load fails as model-unreadable ("load mtl in obj: failed to parse", its log line with an empty path), while the same OBJ with a library path of 16 bytes or more loads (model-obj-mtl-long); the Rust program reads the library the OBJ names (CORPUS.md)',
  ]),
])

/**
 * The families of each smoke case, by hand: what its jobs run (smoke.ts). `layers` is what
 * familiesFor gives a layers job on the generic presets: layers-only and sphere-adaptive run
 * domain-A layers jobs alone, answered whole, so they hold every family process() runs and the
 * layers answer. Every other smoke case runs a slice (withFamilies), so none holds `layers`.
 */
export const SMOKE_FAMILIES: Record<string, string[] | 'layers'> = {
  version: ['version'],
  'config-def': ['config-def'],
  'file-collisions': ['intake'],
  info: ['model-load', 'info'],
  'layers-only': 'layers',
  'sphere-adaptive': 'layers',
}

/**
 * The smoke case the corpus leaves out: `refusals` runs every refused job in one case, across the
 * families they reach; each is a refusal case of its own instead (corpusCases).
 */
export const SMOKE_LEFT_OUT = ['refusals']

/**
 * The families of the refusals the C++ engine makes past reading the job, its presets and its
 * model's file (smoke.ts REFUSALS): those of the steps it reaches before it refuses, so a
 * refusal is held to them. `slice` is every family of the refusal's slice (its generic presets
 * and the cube's overrides, familiesFor: every family process() runs, never `layers`, and the
 * G-code's), `slice-before-gcode` those but the G-code export and its processor: both are refused
 * once slice_print has returned, so in phase O3 they decline past the end of process() as any
 * domain-A slice does. layers-above-the-nozzle is refused before print.process() (apply_layer_heights),
 * so it holds slice-mesh and none of process()'s families. Every other refusal is intake's.
 */
export const REFUSAL_FAMILIES: Record<string, string[] | 'slice' | 'slice-before-gcode'> = {
  'flat-model': ['model-load'],
  'lone-triangle': ['model-load'],
  'too-big-for-the-bed': ['model-load', 'placement'],
  'layers-above-the-nozzle': ['model-load', 'placement', 'slice-mesh'],
  'pause-below-first-layer': 'slice-before-gcode',
  'gcode-folder-unmakeable': 'slice',
  // The model's reading (load_model, the objects index check).
  'empty-model-file': ['model-load'],
  'stl-no-triangles': ['model-load'],
  'stl-truncated': ['model-load'],
  'stl-ascii-bad-facet': ['model-load'],
  'obj-face-out-of-range': ['model-load'],
  'object-index-past-model': ['model-load'],
  // Placement (apply_object_settings, set_custom_gcodes, place_prime_tower, arrange_objects, check_build_volume).
  'object-whole-plate-setting': ['model-load', 'placement'],
  'object-unknown-setting': ['model-load', 'placement'],
  'object-extruder-past-filaments': ['model-load', 'placement'],
  'range-object-setting': ['model-load', 'placement'],
  'range-zero-layer-height': ['model-load', 'placement'],
  'range-extruder-past-filaments': ['model-load', 'placement'],
  'color-change-without-extruder': ['model-load', 'placement'],
  'filament-change-one-filament': ['model-load', 'placement'],
  'filament-change-past-filaments': ['model-load', 'placement'],
  'pause-without-pause-gcode': ['model-load', 'placement'],
  'custom-gcode-by-object': ['model-load', 'placement'],
  'tower-set-past-the-bed': ['model-load', 'placement'],
  'tower-fits-nowhere': ['model-load', 'placement'],
  'off-the-bed': ['model-load', 'placement'],
}

/** Smoke cases that run a prime tower (two filaments, or the tower case). */
export const SMOKE_TOWER_CASES = ['filaments', 'flush-volumes', 'prime-tower']

/**
 * The families of a smoke case of SMOKE_TOWER_CASES: its runs are outside domain A, declined once
 * their objects are placed, so it keeps its marker and the front-objects families it reaches.
 */
export const SMOKE_TOWER_FAMILIES = ['model-load', 'placement', 'wipe-tower']

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
 * The compatibility condition the preset-conditions job's process gets in place of its list of
 * printers: the generic printer's nozzle, so Orca finds the process made for it.
 */
export const PROCESS_CONDITION = 'nozzle_diameter[0]==0.4'

/**
 * A job for each family no other case reaches as the presets stand: a slice of each sparse infill
 * pattern on the 20 mm cube, both wall generators, and supports, tree supports, a raft, ironing,
 * fuzzy skin and Z contouring, each switched on by its setting; the prime tower clumping detection
 * puts on, a slice on the H2D with one filament (its machine preset has an exclusion area of four
 * points); and the cube's info with a process whose compatibility with the printer is a condition.
 */
export function familyJobs(): CorpusCase[] {
  const mesh = (id: string) => CORPUS_MESHES.find((x) => x.id === id)!
  const fill = mesh(FAMILY_MESHES.fill)
  const overhang = mesh(FAMILY_MESHES.overhang)
  const job = (id: string, title: string, m: CorpusMesh, overrides: Overrides, extra: Partial<JobTemplate> = {}): CorpusCase => ({
    id,
    kind: 'job',
    title,
    families: [],
    mesh: m.id,
    job: plainJob(extra.command ?? 'slice', m.file, { overrides, ...extra }),
  })
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
    job('family-wipe-tower-clumping', 'the 20 mm cube on the H2D with one filament and clumping detection, which puts the prime tower on', fill, { enable_wrapping_detection: '1' }, { presets: 'h2d', filaments: 1 }),
    job(
      'family-preset-conditions',
      "the 20 mm cube's info with a process whose compatibility with the printer is a condition (compatible_printers_condition, no list)",
      fill,
      {},
      { command: 'info', processChanges: { compatible_printers: [], compatible_printers_condition: PROCESS_CONDITION } },
    ),
  ]
}

/** The OBJ files of Orca's that are also sliced into layers as OBJ: quads, and normals (with and without its object upside down). */
export const OBJ_FILE_LAYERS = ['obj-extruder-idler-quads', 'obj-v', 'obj-a-upsidedown'] as const

/** Orca's 21 OBJ files read as OBJ: each as an info job, and OBJ_FILE_LAYERS as layers jobs too. */
export function objFileJobs(): CorpusCase[] {
  return [
    ...OBJ_FILE_MESHES.map((m): CorpusCase => ({ id: `mesh-${m.id}-info`, kind: 'job', title: `${m.source} read as OBJ, info`, families: [], mesh: m.id, job: plainJob('info', m.file) })),
    ...OBJ_FILE_LAYERS.map((id): CorpusCase => {
      const m = CORPUS_MESHES.find((x) => x.id === id)!
      return { id: `mesh-${m.id}-layers`, kind: 'job', title: `${m.source} read as OBJ, layers`, families: [], mesh: m.id, job: plainJob('layers', m.file) }
    }),
  ]
}

/** The made files (MADE_MESHES) that cannot be read: a case each, an info job that stops in model-load. */
export const UNREADABLE_MESHES = [
  '3mf-not-a-zip',
  '3mf-zip-without-model',
  '3mf-truncated',
  '3mf-bad-crc',
  '3mf-bad-model-xml',
  '3mf-bad-rels-xml',
  '3mf-bad-settings-xml',
  '3mf-vertex-out-of-range',
  '3mf-no-build-items',
  '3mf-empty',
  'obj-empty',
] as const

/**
 * The made files read (`model-<mesh>-<command>`): each OBJ and 3MF that loads as an info and a
 * layers job (the Bambu project with two filaments, for its parts' extruders), the object of only
 * modifiers (an info job checks no solid, a layers job refuses it), and each unreadable file.
 */
export function madeModelJobs(): CorpusCase[] {
  const job = (meshId: string, command: JobCommand, extra: Partial<JobTemplate> = {}, stopsAt?: string): CorpusCase => {
    const m = CORPUS_MESHES.find((x) => x.id === meshId)!
    const id = `model-${m.id}-${command}`
    return { id, kind: 'job', title: `${m.file} (made by the corpus), ${command}`, families: [], mesh: m.id, job: plainJob(command, m.file, extra), ...(stopsAt ? { stopsAt } : {}), ...(ENGINE_FAULTS[id] ? { fault: ENGINE_FAULTS[id] } : {}) }
  }
  const both = (meshId: string, extra: Partial<JobTemplate> = {}) => [job(meshId, 'info', extra), job(meshId, 'layers', extra)]
  return [
    ...both('obj-texcoords'),
    ...both('obj-mtl'),
    ...both('obj-mtl-long'),
    ...both('obj-mtl-missing'),
    ...both('obj-mixed'),
    ...both('3mf-two-objects'),
    ...both('3mf-components-inch'),
    ...both('3mf-bambu', { presets: 'generic2' }),
    job('3mf-only-modifiers', 'info'),
    job('3mf-only-modifiers', 'layers', {}, 'model-load'),
    ...UNREADABLE_MESHES.map((id) => job(id, 'info', {}, 'model-load')),
  ]
}

/** The bed of the generic printer (300 mm square from 0,0) cut to its back right quarter, as bed_exclude_area points. */
export const EXCLUDED_CORNER = '150x150,300x150,300x300,150x300'
/** The middle of the generic printer's bed, as bed_exclude_area points. */
export const EXCLUDED_MIDDLE = '100x100,200x100,200x200,100x200'

/** A round (delta's) printable area: a 36-gon of radius `r` mm around 0,0, as printable_area points. */
export function roundBed(r = 120, n = 36): string {
  return Array.from({ length: n }, (_, k) => {
    const a = (2 * Math.PI * k) / n
    return `${Number((r * Math.cos(a)).toFixed(4))}x${Number((r * Math.sin(a)).toFixed(4))}`
  }).join(',')
}

/**
 * The placement cases, layers jobs (they reach the boundary without slicing): several objects
 * arranged; arrange off with the objects where their file puts them, one of them partly off the
 * bed (not-printable -52), all not printable (empty-plate) and all not printable with none inside
 * the bed (empty-plate at the build volume check); an excluded area and a round bed;
 * prime towers (two filaments at the default corner, moved off an excluded area, and placed by the
 * job); printing by object; far apart filament temperatures (temperature-difference) and objects
 * that do not fit (arrange-failed); per-object settings and height ranges; custom G-code of every
 * type. The four refused while arranging or at the build volume check stop at placement
 * (stopsAt); empty-plate comes from the slice stage (Print::apply leaves the print empty, past the
 * `slice` line of cli/run.cpp), before print.process(); a layers run is answered whole, refusal
 * and all, so that one holds every family a domain-A layers job does.
 * The refusals of placement with one object are smoke.ts REFUSALS.
 */
export function placementJobs(): CorpusCase[] {
  const mesh = (id: string) => CORPUS_MESHES.find((x) => x.id === id)!
  const job = (id: string, title: string, meshId: string, extra: Partial<JobTemplate>, stopsAt?: string): CorpusCase => ({
    id,
    kind: 'job',
    title,
    families: [],
    mesh: meshId,
    job: plainJob('layers', mesh(meshId).file, extra),
    ...(stopsAt ? { stopsAt } : {}),
  })
  const cube = FAMILY_MESHES.fill
  const two = '3mf-two-objects'
  const tower = (overrides: Overrides): Partial<JobTemplate> => ({ presets: 'generic2', overrides: { enable_prime_tower: '1', ...overrides }, customGcodes: [{ printZ: 5, type: 'filament', extruder: 2 }] })
  return [
    job('place-several-objects', 'three instances of two objects arranged on the plate', two, {}),
    job('place-arrange-off', 'two objects where their file puts them, arrange off', two, { arrange: false }),
    job('place-arrange-off-partly-outside', 'arrange off with one object partly off the bed (not-printable, -52)', '3mf-one-off-bed', { arrange: false }, 'placement'),
    // Refused in the slice stage before print.process() (cli/run.cpp: Print::apply leaves the print empty); a layers run is answered whole, so it holds every family a layers job does.
    job('place-every-object-not-printable', 'arrange off with every object marked not printable (empty-plate)', '3mf-not-printable', { arrange: false }),
    // Refused at the build volume check (cli/run.cpp check_build_volume): every object marked not
    // printable and none inside the volume (update_print_volume_state counts instances inside
    // whether printable or not), so the program must make this empty-plate itself.
    job('place-no-object-inside', 'arrange off with every object marked not printable and none inside the bed (empty-plate)', '3mf-not-printable-off-bed', { arrange: false }, 'placement'),
    job('place-bed-exclude-area', 'the 20 mm cube arranged around an excluded area in the middle of the bed', cube, { overrides: { bed_exclude_area: EXCLUDED_MIDDLE } }),
    job('place-round-bed', 'two objects arranged on a round (delta) printable area around 0,0', two, { overrides: { printable_area: roundBed() } }),
    job('place-tower-default-corner', 'two filaments: the prime tower at its default corner', cube, tower({})),
    job('place-tower-off-excluded-area', 'two filaments: the prime tower moved off an excluded area over its default corner', cube, tower({ bed_exclude_area: EXCLUDED_CORNER })),
    job('place-tower-set-by-job', 'two filaments: the prime tower where the job sets it (wipe_tower_x, wipe_tower_y)', cube, tower({ wipe_tower_x: '40', wipe_tower_y: '40' })),
    job('place-by-object', 'two objects printed by object (print_sequence)', two, { overrides: { print_sequence: 'by object' } }),
    // Refused while arranging (cli/run.cpp: the objects' filament temperature classes off one plate).
    job(
      'place-temperature-difference',
      'two objects on filaments of far apart temperatures, PLA and PC (temperature-difference)',
      two,
      { presets: 'generic2', overrides: { filament_type: 'PLA;PC' }, objects: [{ index: 1, overrides: { extruder: '2' }, layerHeights: { mode: 'fixed' }, heightRanges: [] }] },
      'placement',
    ),
    job('place-objects-do-not-fit', 'two objects that do not fit on the plate together (arrange-failed)', '3mf-two-large', {}, 'placement'),
    job('place-object-settings', 'the 20 mm cube with settings of its own and two height ranges', cube, {
      objects: [
        {
          index: 0,
          overrides: { wall_loops: '3', sparse_infill_density: '30%', extruder: '1' },
          layerHeights: { mode: 'fixed' },
          heightRanges: [
            { from: 5, to: 10, overrides: { layer_height: '0.1', wall_loops: '4' } },
            { from: 12, to: 15, overrides: { sparse_infill_pattern: 'gyroid' } },
          ],
        },
      ],
    }),
    job('place-custom-gcodes', 'two filaments: a pause, a colour change, custom G-code and a filament change at layers of the 20 mm cube', cube, {
      presets: 'generic2',
      customGcodes: [
        { printZ: 3, type: 'pause', note: 'corpus: a pause' },
        { printZ: 6, type: 'color', color: '#30c030', extruder: 2 },
        { printZ: 9, type: 'custom', gcode: 'M117 corpus custom' },
        { printZ: 12, type: 'filament', extruder: 1 },
      ],
    }),
  ]
}

/** A compatibility condition that does not hold for the generic printer (its nozzle is 0.4 mm). */
export const FALSE_CONDITION = 'nozzle_diameter[0]==0.6'

/**
 * The preset-conditions cases, info jobs on the 20 mm cube: a filament whose compatibility with the
 * process is a condition; a filament and a process whose condition is false (the warning
 * filament-not-compatible, the refusal process-not-compatible); a condition Orca's placeholder
 * parser cannot parse, on the process (Orca only logs it, Preset.cpp:833, at a level the engine
 * does not print, so stdout stays empty) and on a filament's condition for the process (Orca
 * prints the parse error on stdout, Preset.cpp:785, and takes the filament); and one with a
 * regular expression.
 */
export function presetJobs(): CorpusCase[] {
  const m = CORPUS_MESHES.find((x) => x.id === FAMILY_MESHES.fill)!
  const job = (id: string, title: string, extra: Partial<JobTemplate>, stopsAt?: string): CorpusCase => ({ id, kind: 'job', title, families: [], mesh: m.id, job: plainJob('info', m.file, extra), ...(stopsAt ? { stopsAt } : {}) })
  const process = (condition: string): Partial<JobTemplate> => ({ processChanges: { compatible_printers: [], compatible_printers_condition: condition } })
  return [
    job('preset-filament-prints-condition', "a filament whose compatibility with the process is a condition (compatible_prints_condition, no list)", { filamentChanges: { compatible_prints: [], compatible_prints_condition: 'layer_height >= 0.1' } }),
    job('preset-filament-not-compatible', 'a filament whose condition for the printer is false (the warning filament-not-compatible)', { filamentChanges: { compatible_printers: [], compatible_printers_condition: FALSE_CONDITION } }),
    job('preset-process-not-compatible', 'a process whose condition for the printer is false (process-not-compatible)', process(FALSE_CONDITION), 'preset-conditions'),
    job('preset-condition-unparsable', 'a process whose condition does not parse', process('nozzle_diameter[0]==')),
    job('preset-filament-condition-unparsable', 'a filament whose condition for the process does not parse (Orca prints it on stdout and takes the filament)', { filamentChanges: { compatible_prints: [], compatible_prints_condition: 'layer_height >=' } }),
    job('preset-condition-regex', 'a process whose condition is a regular expression match', process('printer_model=~/.*Voron.*/')),
  ]
}

/** The orient cases: an info and a layers job of the 20 mm cube with orient on, which sketch-orca-rs declines at orient (held to orient alone, withFamilies). */
export function orientJobs(): CorpusCase[] {
  const m = CORPUS_MESHES.find((x) => x.id === FAMILY_MESHES.fill)!
  return (['info', 'layers'] as const).map((command) => ({ id: `orient-${command}`, kind: 'job', title: `the 20 mm cube, ${command}, orient on`, families: [], mesh: m.id, job: plainJob(command, m.file, { orient: true }) }))
}

/** The process jobs (processJobs) the C++ engine refuses, with what it refuses them with: each reaches a family past intake. */
export const PROCESS_REFUSED: Record<string, string> = {
  'process-layer-height-above-nozzle-layers': 'validate-error from Print::validate ("Layer height cannot exceed nozzle diameter."), before print.process()',
}

/**
 * The process jobs (phase O3): domain-A layers and slice jobs on the existing meshes, on the
 * generic set, for the families process() and the layers answer run: the classic walls (a layers
 * job and slices) and the Arachne walls set by name on thin walls; the rectilinear and concentric
 * patterns, sparse and on the surfaces (a layers job and slices), and the surfaces' patterns
 * swapped; the elephant foot compensation; polyholes; brims (outer, outer and inner) and skirts;
 * several objects sliced; adaptive and variable layer heights (a profile, a height range's layer
 * height); an object's and its height ranges' own walls and patterns; and a layer height above the
 * nozzle, which Print::validate refuses before print.process() (PROCESS_REFUSED). One more is
 * outside domain A on purpose: multiline infill, which no other case sets, so fill-multiline has a
 * case and the program's decline of it is held to one (domainACut).
 */
export function processJobs(): CorpusCase[] {
  const job = (id: string, title: string, meshId: string, command: 'layers' | 'slice', extra: Partial<JobTemplate> = {}): CorpusCase => {
    const m = CORPUS_MESHES.find((x) => x.id === meshId)!
    return { id, kind: 'job', title, families: [], mesh: m.id, job: plainJob(command, m.file, extra) }
  }
  const cube = FAMILY_MESHES.fill
  const obj = (o: Partial<JobObject>): JobObject => ({ index: 0, overrides: {}, layerHeights: { mode: 'fixed' }, heightRanges: [], ...o })
  return [
    job('process-walls-classic-layers', 'cube_with_hole, layers, the classic wall generator', 'cube-with-hole', 'layers', { overrides: { wall_generator: 'classic' } }),
    job('process-walls-classic-slice', 'cube_with_concave_hole sliced with the classic wall generator', 'cube-with-concave-hole', 'slice', { overrides: { wall_generator: 'classic' } }),
    job('process-walls-classic-thin-slice', 'two_hollow_squares (thin walls) sliced with the classic wall generator', 'two-hollow-squares', 'slice', { overrides: { wall_generator: 'classic' } }),
    job('process-walls-arachne-layers', 'two_hollow_squares, layers, the Arachne wall generator set by name with 4 walls', 'two-hollow-squares', 'layers', { overrides: { wall_generator: 'arachne', wall_loops: '4' } }),
    job('process-infill-rectilinear-layers', 'the 20 mm cube, layers, rectilinear sparse infill at 25 %', cube, 'layers', { overrides: { sparse_infill_pattern: 'rectilinear', sparse_infill_density: '25%' } }),
    job('process-infill-rectilinear-slice', 'V sliced with rectilinear sparse infill at 40 %', 'v', 'slice', { overrides: { sparse_infill_pattern: 'rectilinear', sparse_infill_density: '40%' } }),
    job('process-surfaces-rectilinear-slice', 'cube_with_hole sliced with rectilinear top, bottom and internal solid infill', 'cube-with-hole', 'slice', {
      overrides: { top_surface_pattern: 'rectilinear', bottom_surface_pattern: 'rectilinear', internal_solid_infill_pattern: 'rectilinear' },
    }),
    job('process-infill-concentric-layers', 'the 20 mm cube, layers, concentric sparse infill at 25 %', cube, 'layers', { overrides: { sparse_infill_pattern: 'concentric', sparse_infill_density: '25%' } }),
    job('process-infill-concentric-slice', 'cube_with_concave_hole sliced with concentric sparse infill at 30 %', 'cube-with-concave-hole', 'slice', { overrides: { sparse_infill_pattern: 'concentric', sparse_infill_density: '30%' } }),
    job('process-surfaces-concentric-slice', 'cube_with_hole sliced with concentric top and bottom surfaces', 'cube-with-hole', 'slice', { overrides: { top_surface_pattern: 'concentric', bottom_surface_pattern: 'concentric' } }),
    job('process-surfaces-swapped-slice', 'the 20 mm cube sliced with monotonic top, monotonic line bottom and internal solid infill', cube, 'slice', {
      overrides: { top_surface_pattern: 'monotonic', bottom_surface_pattern: 'monotonicline', internal_solid_infill_pattern: 'monotonicline' },
    }),
    job('process-elephant-foot-layers', 'the 20 mm cube, layers, elephant foot compensation of 0.2 mm over 3 layers', cube, 'layers', { overrides: { elefant_foot_compensation: '0.2', elefant_foot_compensation_layers: '3' } }),
    job('process-elephant-foot-slice', 'cube_with_hole sliced with elephant foot compensation of 0.15 mm', 'cube-with-hole', 'slice', { overrides: { elefant_foot_compensation: '0.15' } }),
    job('process-polyholes-layers', 'extruder_idler, layers, its round holes made polyholes', 'extruder-idler', 'layers', { overrides: { hole_to_polyhole: '1' } }),
    job('process-polyholes-slice', 'sloping_hole sliced with polyholes, not twisted', 'sloping-hole', 'slice', { overrides: { hole_to_polyhole: '1', hole_to_polyhole_twisted: '0' } }),
    job('process-brim-outer-slice', 'the 20 mm cube sliced with an outer brim of 5 mm and no skirt', cube, 'slice', { overrides: { brim_type: 'outer_only', brim_width: '5', skirt_loops: '0' } }),
    job('process-brim-inner-layers', 'cube_with_hole, layers, an outer and inner brim of 3 mm', 'cube-with-hole', 'layers', { overrides: { brim_type: 'outer_and_inner', brim_width: '3' } }),
    job('process-skirt-slice', '2x20x10 sliced with a skirt of 2 loops 4 mm out and no brim', '2x20x10', 'slice', { overrides: { skirt_loops: '2', skirt_distance: '4', brim_type: 'no_brim' } }),
    job('process-skirt-layers', 'the 20 mm cube, layers, a skirt of 3 loops 2 layers tall', cube, 'layers', { overrides: { skirt_loops: '3', skirt_height: '2' } }),
    job('process-several-objects-slice', 'three instances of two objects arranged on the plate and sliced', '3mf-two-objects', 'slice'),
    job('process-adaptive-layers', 'overhang, layers, adaptive layer heights at quality 0.5', 'overhang', 'layers', { objects: [obj({ layerHeights: { mode: 'adaptive', quality: 0.5 } })] }),
    job('process-adaptive-slice', 'bridge sliced with adaptive layer heights at quality 0.3', 'bridge', 'slice', { objects: [obj({ layerHeights: { mode: 'adaptive', quality: 0.3 } })] }),
    job('process-profile-slice', 'the 20 mm cube sliced at the heights of a boundary profile', cube, 'slice', {
      overrides: SKETCH_LAYERS,
      objects: [obj({ layerHeights: { mode: 'profile', profile: [0, 0.2, 5, 0.12, 12, 0.24, 20, 0.2] } })],
    }),
    job('process-height-range-layers', 'the 20 mm cube, layers, a height range from 6 to 12 mm at 0.12 mm layers', cube, 'layers', {
      overrides: SKETCH_LAYERS,
      objects: [obj({ heightRanges: [{ from: 6, to: 12, overrides: { layer_height: '0.12' } }] })],
    }),
    job('process-height-range-slice', 'the 20 mm cube sliced with a height range from 6 to 12 mm at 0.12 mm layers', cube, 'slice', {
      overrides: SKETCH_LAYERS,
      objects: [obj({ heightRanges: [{ from: 6, to: 12, overrides: { layer_height: '0.12' } }] })],
    }),
    job('process-object-levels-layers', 'the 20 mm cube, layers, classic walls of its own and two height ranges of rectilinear and concentric sparse infill', cube, 'layers', {
      objects: [
        obj({
          overrides: { wall_generator: 'classic', sparse_infill_density: '30%' },
          heightRanges: [
            { from: 4, to: 8, overrides: { sparse_infill_pattern: 'rectilinear' } },
            { from: 12, to: 16, overrides: { sparse_infill_pattern: 'concentric' } },
          ],
        }),
      ],
    }),
    job('process-object-levels-slice', 'cube_with_hole sliced with classic walls of its own and a height range of concentric sparse infill', 'cube-with-hole', 'slice', {
      objects: [obj({ overrides: { wall_generator: 'classic' }, heightRanges: [{ from: 3, to: 7, overrides: { sparse_infill_pattern: 'concentric' } }] })],
    }),
    job('process-layer-height-above-nozzle-layers', 'the 20 mm cube, layers, a layer height of 0.5 mm on the 0.4 mm nozzle (refused by Print::validate)', cube, 'layers', { overrides: { layer_height: '0.5' } }),
    // Outside domain A: the one case that sets multiline infill, so the program's decline of fill-multiline is held to it.
    job('process-fill-multiline-layers', 'the 20 mm cube, layers, each sparse infill line printed as 2 (outside domain A: declined as fill-multiline)', cube, 'layers', { overrides: { fill_multiline: '2' } }),
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
  // smoke-refusals is left out (SMOKE_LEFT_OUT): its jobs are the refusal cases below.
  const smoke = SMOKE_CASES.filter((c) => !SMOKE_LEFT_OUT.includes(c.id)).map((c): CorpusCase => ({ id: `smoke-${c.id}`, kind: 'smoke', title: c.title, families: [], smoke: c.id }))
  const meshes = ORCA_MESHES.flatMap((m) =>
    (['info', 'layers', 'slice'] as const).map((command): CorpusCase => {
      const id = `mesh-${m.id}-${command}`
      return { id, kind: 'job', title: `${m.source}, ${command}`, families: [], mesh: m.id, job: plainJob(command, m.file), ...(ENGINE_FAULTS[id] ? { fault: ENGINE_FAULTS[id] } : {}) }
    }),
  )
  const refusals = REFUSALS.map((r): CorpusCase => {
    const f = REFUSAL_FAMILIES[r.id]
    return { id: `refusal-${r.id}`, kind: 'refusal', title: `the smoke refusal ${r.id} (${r.code})`, families: f === undefined ? ['intake'] : Array.isArray(f) ? f : [], refusal: r.id }
  })
  return [
    ...smoke,
    ...meshes,
    ...sketchJobs(),
    ...familyJobs(),
    ...objFileJobs(),
    ...madeModelJobs(),
    ...placementJobs(),
    ...presetJobs(),
    ...orientJobs(),
    ...processJobs(),
    ...exampleJobs(protocolDir, exampleNames),
    ...refusals,
  ]
}

/** `families` cut to those up to `stopsAt` (STOP_ORDER), the family the C++ engine refuses the job in. */
export function familiesUpTo(families: string[], stopsAt: string): string[] {
  const last = (STOP_ORDER as readonly string[]).indexOf(stopsAt)
  if (last < 0) throw new StepError(`a case stops at ${stopsAt}, which is not one of ${STOP_ORDER.join(', ')}`)
  return families.filter((f) => {
    const i = (STOP_ORDER as readonly string[]).indexOf(f)
    return i >= 0 && i <= last
  })
}

/**
 * Fills in each case's families, so that each holds only the families its run can pass: jobs from
 * their settings at every level, their model's painting (`painted`, mesh ids), their orient and
 * their command (familiesFor) and presets (presetFamilies), cut at the family a job stops in
 * (stopsAt), or orient alone for a job that orients its objects, or, for a job outside domain A,
 * cut to its markers and the front-objects families (domainACut); smoke cases from SMOKE_FAMILIES
 * (a generic layers job for `layers`), SMOKE_TOWER_FAMILIES for those that run a prime tower, or
 * as a generic slice; the refusals of a slice (REFUSAL_FAMILIES) as their generic slice with the
 * cube's overrides; and none for an engine fault (`fault`).
 */
export function withFamilies(cases: CorpusCase[], sets: Record<PresetKey, PresetSet>, defaults: Map<string, string>, painted: ReadonlySet<string>): CorpusCase[] {
  const generic = (command: 'layers' | 'slice', overrides: Overrides) => familiesFor(command, settingLookup(sets.generic, overrides, defaults), 1, settingPoints(sets.generic, overrides, defaults))
  return cases.map((c) => {
    if (c.fault !== undefined) return { ...c, families: [] }
    if (c.kind === 'job') {
      const j = c.job!
      // An orient job is held to orient alone: sketch-orca-rs declines it where orientation would
      // start, before any output, and a declined run counts against every family its case lists
      // (certify.ts familyVerdicts), so model-load, placement and info could never certify while
      // these cases listed them. familiesFor still adds orient beside the others (decline.rs mirrors it).
      if (j.orient) return { ...c, families: ['orient'] }
      const set = templateSet(j, sets)
      const get = settingLookup(set, j.overrides, defaults)
      const model: JobModel = { levels: objectLookups(get, j.objects), painted: painted.has(c.mesh!) }
      const all = inFamilyOrder([...presetFamilies(set), ...familiesFor(j.command, get, set.filaments.length, settingPoints(set, j.overrides, defaults), j.orient, model)])
      // A job the C++ engine refuses before the slice is held to the families up to that stop; any
      // other is cut to its markers when it is outside domain A, as the program declines it once
      // its objects are placed, before process() (decline.rs mirrors the markers).
      return { ...c, families: c.stopsAt === undefined ? domainACut(all) : familiesUpTo(all, c.stopsAt) }
    }
    if (c.kind === 'smoke') {
      const fixed = SMOKE_FAMILIES[c.smoke!]
      if (fixed === 'layers') return { ...c, families: generic('layers', PLAIN_OVERRIDES) }
      if (fixed) return { ...c, families: fixed }
      if (SMOKE_TOWER_CASES.includes(c.smoke!)) return { ...c, families: SMOKE_TOWER_FAMILIES }
      return { ...c, families: generic('slice', PLAIN_OVERRIDES) }
    }
    if (c.kind === 'refusal') {
      const f = REFUSAL_FAMILIES[c.refusal!]
      if (f === 'slice') return { ...c, families: generic('slice', cubeOverrides) }
      if (f === 'slice-before-gcode') return { ...c, families: generic('slice', cubeOverrides).filter((x) => x !== 'gcode-export' && x !== 'gcode-processor') }
    }
    return c
  })
}

// ---------------------------------------------------------------- the corpus file

export interface CorpusMeshEntry extends CorpusMesh {
  /** The source's bytes; for a made file, the file's. */
  sourceSha256: string
  /** The file a case gets (the converted STL for an OBJ). */
  sha256: string
  size: number
  /** Triangles of a converted OBJ. */
  triangles?: number
  /** The companions' bytes, by name, in `companions` order. */
  companionSha256?: Record<string, string>
  /** The painting the model holds (modelPainting), when it holds some: its jobs reach painted-facets. */
  painting?: string[]
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

/**
 * Converts, copies or makes every mesh (CORPUS_MESHES) into `corpusDir` (fresh) and returns their
 * entries: Orca's from `testData`, the made files (and their companions) from MADE_FILES, each with
 * the painting its file holds (modelPainting), when it holds some.
 */
export function buildMeshes(testData: string, corpusDir: string): CorpusMeshEntry[] {
  return buildMeshFiles(testData, corpusDir).map((m) => {
    const painting = modelPainting(m.file, readFileSync(meshPath(corpusDir, m)))
    return painting.length > 0 ? { ...m, painting } : m
  })
}

function buildMeshFiles(testData: string, corpusDir: string): CorpusMeshEntry[] {
  rmSync(join(corpusDir, 'meshes'), { recursive: true, force: true })
  return CORPUS_MESHES.map((m) => {
    const to = meshPath(corpusDir, m)
    mkdirSync(join(corpusDir, 'meshes', m.id), { recursive: true })
    if (m.kind === 'made') {
      const make = MADE_FILES[m.id]
      if (!make) throw new StepError(`the made mesh ${m.id} has no entry in MADE_FILES`)
      const files = make()
      const names = [m.file, ...(m.companions ?? [])]
      if (JSON.stringify(Object.keys(files).sort()) !== JSON.stringify([...names].sort())) throw new StepError(`MADE_FILES ${m.id} makes ${Object.keys(files).join(', ')}, not ${names.join(', ')}`)
      for (const n of names) writeFileSync(join(corpusDir, 'meshes', m.id, n), files[n]!)
      const bytes = files[m.file]!
      const companions = m.companions?.length ? { companionSha256: Object.fromEntries(m.companions.map((n) => [n, sha256Bytes(files[n]!)])) } : {}
      return { ...m, sourceSha256: sha256Bytes(bytes), sha256: sha256Bytes(bytes), size: bytes.length, ...companions }
    }
    const src = join(testData, ...m.source.split('/'))
    if (!existsSync(src)) throw new StepError(`no ${m.source} in ${testData}; the engine's checkout (<work>/src) is at another commit`)
    const bytes = readFileSync(src)
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
    for (const [n, sha] of Object.entries(m.companionSha256 ?? {})) {
      const c = join(corpusDir, 'meshes', m.id, n)
      if (!existsSync(c)) out.push(`${m.id}: no ${c}`)
      else if (sha256Bytes(readFileSync(c)) !== sha) out.push(`${m.id}: ${c} is not the file corpus.json names`)
    }
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
            for (const n of m.companions ?? []) copyFileSync(join(o.corpusDir, 'meshes', m.id, n), join(env.dir, n))
            const files = templateFiles(c.job!, env.files[c.job!.presets], env.sets, env.dir)
            return runCorpusJob(env, instantiateJob(c.job!, env.dir, env.resources, files) as unknown as Record<string, unknown>)
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
