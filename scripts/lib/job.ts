// The files sketch-orca reads and writes (protocol/README.md; JSON Schemas in protocol/schema/): the job
// (`sketch-orca-job/1`) and the result (`sketch-orca-result/1`), plus the progress lines.
// writeJob refuses a job the engine would refuse, with the engine's code (cli/job.cpp makes the
// same checks); readResult refuses a result that does not follow the contract, naming the field.
// An optional field of a job given as null is the same as one left out, on both sides.

import { readFileSync, writeFileSync } from 'node:fs'

export const JOB_FORMAT = 'sketch-orca-job/1'
export const RESULT_FORMAT = 'sketch-orca-result/1'

export const JOB_COMMANDS = ['slice', 'layers', 'info'] as const
export type JobCommand = (typeof JOB_COMMANDS)[number]

export const CUSTOM_GCODE_TYPES = ['pause', 'color', 'filament', 'custom'] as const
export type CustomGcodeType = (typeof CUSTOM_GCODE_TYPES)[number]

export const MODEL_EXTENSIONS = ['.3mf', '.stl', '.obj'] as const

/**
 * Named failure codes. The engine's own (contract) and the meanings of the stock Orca command
 * line's exit codes; the result may carry others, as long as they are kebab-case words.
 */
export const ENGINE_ERROR_CODES = ['model-unreadable', 'preset-invalid', 'not-printable', 'slice-failed', 'job-invalid'] as const

/**
 * Exit code -> name. The engine exits with the stock Orca command line's code for the same meaning
 * (OrcaSlicer.cpp cli_errors, v2.4.2) and names it as its failure table does (cli/common.cpp; where
 * several names share a code, the first). The codes the engine never uses keep the stock meaning.
 */
export const ORCA_EXIT_CODES: Record<number, string> = {
  [-1]: 'environment',
  [-2]: 'invalid-params',
  [-3]: 'file-not-found',
  [-5]: 'preset-invalid',
  [-6]: 'model-unreadable',
  [-7]: 'not-fdm',
  [-8]: 'unsupported',
  [-13]: 'output-failed',
  [-14]: 'out-of-memory',
  [-17]: 'process-not-compatible',
  [-18]: 'config-invalid',
  [-21]: 'arrange-failed',
  [-22]: 'orient-failed',
  [-50]: 'empty-plate',
  [-51]: 'validate-error',
  [-52]: 'not-printable',
  [-58]: 'time-limit',
  [-59]: 'triangle-limit',
  [-60]: 'nothing-after-skip',
  [-61]: 'filament-bed-mismatch',
  [-62]: 'temperature-difference',
  [-63]: 'collision',
  [-64]: 'collision',
  [-65]: 'spiral-vase',
  [-100]: 'slice-failed',
  [-101]: 'path-conflicts',
  [-102]: 'gcode-in-unprintable-area',
}

export class JobError extends Error {
  /**
   * The code the engine refuses the same job with: job-invalid, or the contract's own name
   * (custom-gcode-invalid, layer-profile-invalid) where it has one.
   */
  readonly code: string
  /** The object a layer height profile refusal is about. */
  readonly objectIndex?: number
  constructor(message: string, options: { code?: string; objectIndex?: number } = {}) {
    super(message)
    this.name = 'JobError'
    this.code = options.code ?? 'job-invalid'
    if (options.objectIndex !== undefined) this.objectIndex = options.objectIndex
  }
}

export type Overrides = Record<string, string>

export type LayerHeights = { mode: 'fixed' } | { mode: 'adaptive'; quality: number } | { mode: 'profile'; profile: number[] }

export interface HeightRange {
  from: number
  to: number
  overrides: Overrides
}

export interface JobObject {
  index: number
  overrides: Overrides
  layerHeights: LayerHeights
  heightRanges: HeightRange[]
}

export interface CustomGcode {
  /** The print Z of the layer this runs before. */
  printZ: number
  type: CustomGcodeType
  /** 1-based; for filament (the extruder to switch to) and color. */
  extruder?: number
  /** `#rrggbb`, for color. */
  color?: string
  /** For custom: the G-code itself. */
  gcode?: string
  /** What the person at the printer does (for the program that runs the engine to show; not sent to the printer). */
  note?: string
}

export interface JobOutput {
  gcode: string
  bambu3mf: string | null
  stats: boolean
}

export interface Job {
  format: typeof JOB_FORMAT
  command: JobCommand
  resources: string
  workDir: string
  model: { path: string }
  presets: { machine: string; process: string; filaments: string[] }
  overrides: Overrides
  objects: JobObject[]
  customGcodes: CustomGcode[]
  arrange: boolean
  orient: boolean
  output: JobOutput
}

/** Two custom G-codes closer than this are at the same print Z (Orca runs one per layer). */
export const Z_EPSILON = 1e-6

/**
 * An override's key: a letter, then letters, digits and _. Most of Orca's keys are lower case, but
 * not all (the filament setting required_nozzle_HRC). Whether Orca has the setting is the engine's
 * to say (setting-invalid); cli/job.cpp takes the same keys.
 */
export const OVERRIDE_KEY_RE = /^[A-Za-z][A-Za-z0-9_]*$/
const CODE_RE = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/
const COLOR_RE = /^#[0-9a-fA-F]{6}$/
/**
 * Keys that belong to the preset files, never to overrides, in any letter case: Orca's loader reads
 * type, name, from and inherits in any case (ConfigBase::load_from_json).
 */
export const RESERVED_OVERRIDES: ReadonlySet<string> = new Set(['type', 'name', 'from', 'inherits', 'compatible_printers'])

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function record(v: unknown, field: string): Record<string, unknown> {
  if (!isRecord(v)) throw new JobError(`${field} must be an object`)
  return v
}

function text(v: unknown, field: string): string {
  if (typeof v !== 'string') throw new JobError(`${field} must be text`)
  return v
}

function bool(v: unknown, field: string, fallback?: boolean): boolean {
  if (v == null && fallback !== undefined) return fallback
  if (typeof v !== 'boolean') throw new JobError(`${field} must be true or false`)
  return v
}

function num(v: unknown, field: string): number {
  if (typeof v !== 'number' || !Number.isFinite(v)) throw new JobError(`${field} must be a number`)
  return v
}

/**
 * Boost.Filesystem's is_absolute on Windows, which the engine asks (find_root_directory_start in its
 * libs/filesystem/src/path.cpp): a root name and a separator right after it. The root name is a
 * drive or device (C:, a letter then letters, digits or $, then :), a UNC server (\\server), or one
 * of the prefixes \\?\, \\.\ and \??\ followed by a drive or device, or else by a name up to the
 * next separator. Either separator counts.
 */
export const WINDOWS_ABSOLUTE = /^(?:[A-Za-z][A-Za-z0-9$]*:[\\/]|(?:[\\/]{2}[?.]|[\\/]\?\?)[\\/](?:[A-Za-z][A-Za-z0-9$]*:[\\/]|(?![A-Za-z][A-Za-z0-9$]*:)[^\\/]*[\\/])|[\\/]{2}(?![?.][\\/])[^\\/]+[\\/])/

/**
 * Boost.Filesystem's is_absolute elsewhere: a root directory. One / starts it, and so do three or
 * more; two (//) and a name after them are a root name (//server), so the root directory is the /
 * after that name.
 */
export const POSIX_ABSOLUTE = /^(?:\/(?!\/)|\/\/(?:\/|[^/]+\/))/

/**
 * Whether a path is absolute as the engine reads it (Boost.Filesystem's is_absolute): on Windows a
 * drive and a root (C:\... or C:/...) or a UNC share (\\server\share...) (WINDOWS_ABSOLUTE);
 * elsewhere /... (POSIX_ABSOLUTE). A Windows path from the root of the current drive (\jobs) is
 * not: it would depend on the drive. protocol/examples/paths/absolute.json lists cases a client
 * checks its own rule against.
 */
export function isEngineAbsolute(p: string, platform: NodeJS.Platform = process.platform): boolean {
  return (platform === 'win32' ? WINDOWS_ABSOLUTE : POSIX_ABSOLUTE).test(p)
}

function absPath(v: unknown, field: string, extensions?: readonly string[], platform: NodeJS.Platform = process.platform): string {
  const p = text(v, field)
  if (p === '' || !isEngineAbsolute(p, platform)) throw new JobError(`${field} must be an absolute path, got ${JSON.stringify(p)}`)
  if (extensions && !extensions.some((e) => p.toLowerCase().endsWith(e))) throw new JobError(`${field} must end with ${extensions.join(' or ')}, got ${JSON.stringify(p)}`)
  return p
}

/** Override values as Orca writes them; numbers and booleans are turned into that text. */
export function normalizeOverrides(v: unknown, field: string): Overrides {
  if (v == null) return {}
  const o = record(v, field)
  const out: Overrides = {}
  for (const [k, raw] of Object.entries(o)) {
    if (!OVERRIDE_KEY_RE.test(k)) throw new JobError(`${field}: "${k}" is not an Orca setting key`)
    if (RESERVED_OVERRIDES.has(k.toLowerCase())) throw new JobError(`${field}: "${k}" belongs to the preset files, not to overrides`)
    if (typeof raw === 'string') out[k] = raw
    else if (typeof raw === 'number' && Number.isFinite(raw)) out[k] = String(raw)
    else if (typeof raw === 'boolean') out[k] = raw ? '1' : '0'
    else throw new JobError(`${field}.${k} must be text (the value as Orca writes it)`)
  }
  return out
}

/** `objectIndex` is the object the profile is for, which a layer-profile-invalid refusal names. */
export function normalizeLayerHeights(v: unknown, field: string, objectIndex?: number): LayerHeights {
  if (v == null) return { mode: 'fixed' }
  const o = record(v, field)
  if (o.mode === 'fixed') return { mode: 'fixed' }
  if (o.mode === 'adaptive') {
    const quality = o.quality == null ? 0.5 : num(o.quality, `${field}.quality`)
    if (quality < 0 || quality > 1) throw new JobError(`${field}.quality must be between 0 (finest) and 1 (coarsest), got ${quality}`)
    return { mode: 'adaptive', quality }
  }
  if (o.mode === 'profile') {
    if (!Array.isArray(o.profile)) throw new JobError(`${field}.profile must be a list [z0, h0, z1, h1, ...]`)
    const profile = o.profile.map((x, i) => num(x, `${field}.profile[${i}]`))
    if (profile.length < 4 || profile.length % 2 !== 0) throw new JobError(`${field}.profile must hold pairs of z and height, at least two`)
    if (profile[0] !== 0) throw new JobError(`${field}.profile must start at z 0, the bottom of the object`, { code: 'layer-profile-invalid', objectIndex })
    for (let i = 0; i < profile.length; i += 2) {
      if (i > 0 && profile[i]! < profile[i - 2]!) throw new JobError(`${field}.profile: z ${profile[i]} comes after z ${profile[i - 2]}`)
      if (!(profile[i + 1]! > 0)) throw new JobError(`${field}.profile: the height at z ${profile[i]} must be above 0`)
    }
    return { mode: 'profile', profile }
  }
  throw new JobError(`${field}.mode must be fixed, adaptive or profile, got ${JSON.stringify(o.mode)}`)
}

function normalizeObjects(v: unknown): JobObject[] {
  if (v == null) return []
  if (!Array.isArray(v)) throw new JobError('objects must be a list')
  const seen = new Set<number>()
  return v.map((raw, i) => {
    const f = `objects[${i}]`
    const o = record(raw, f)
    const index = num(o.index, `${f}.index`)
    if (!Number.isInteger(index) || index < 0 || index > 1e9) throw new JobError(`${f}.index must be a whole number from 0`)
    if (seen.has(index)) throw new JobError(`${f}.index ${index} is listed twice`)
    seen.add(index)
    let ranges: HeightRange[] = []
    if (o.heightRanges != null) {
      if (!Array.isArray(o.heightRanges)) throw new JobError(`${f}.heightRanges must be a list`)
      ranges = o.heightRanges.map((r, j) => {
        const rf = `${f}.heightRanges[${j}]`
        const ro = record(r, rf)
        const from = num(ro.from, `${rf}.from`)
        const to = num(ro.to, `${rf}.to`)
        if (from < 0 || !(to > from)) throw new JobError(`${rf} must have 0 <= from < to, got ${from}..${to}`)
        return { from, to, overrides: normalizeOverrides(ro.overrides, `${rf}.overrides`) }
      })
      for (let j = 1; j < ranges.length; j++) {
        if (ranges[j]!.from < ranges[j - 1]!.to) throw new JobError(`${f}.heightRanges[${j}] starts at ${ranges[j]!.from}, inside the range before it (to ${ranges[j - 1]!.to})`)
      }
    }
    return { index, overrides: normalizeOverrides(o.overrides, `${f}.overrides`), layerHeights: normalizeLayerHeights(o.layerHeights, `${f}.layerHeights`, index), heightRanges: ranges }
  })
}

function normalizeCustomGcodes(v: unknown): CustomGcode[] {
  if (v == null) return []
  if (!Array.isArray(v)) throw new JobError('customGcodes must be a list')
  const out = v.map((raw, i): CustomGcode => {
    const f = `customGcodes[${i}]`
    const o = record(raw, f)
    const printZ = num(o.printZ, `${f}.printZ`)
    if (!(printZ > 0)) throw new JobError(`${f}.printZ must be above 0: there is no layer below the first`, { code: 'custom-gcode-invalid' })
    const type = o.type
    if (typeof type !== 'string' || !(CUSTOM_GCODE_TYPES as readonly string[]).includes(type)) {
      throw new JobError(`${f}.type must be ${CUSTOM_GCODE_TYPES.join(', ')}, got ${JSON.stringify(type)}`)
    }
    const c: CustomGcode = { printZ, type: type as CustomGcodeType }
    if (o.extruder != null) {
      const e = num(o.extruder, `${f}.extruder`)
      if (!Number.isInteger(e) || e < 1 || e > 1e6) throw new JobError(`${f}.extruder must be a whole number from 1`)
      c.extruder = e
    } else if (type === 'filament') {
      throw new JobError(`${f}: a filament change needs the extruder to switch to`)
    }
    if (o.color != null) {
      const color = text(o.color, `${f}.color`)
      if (!COLOR_RE.test(color)) throw new JobError(`${f}.color must be #rrggbb, got ${JSON.stringify(color)}`)
      c.color = color.toLowerCase()
    } else if (type === 'color') {
      throw new JobError(`${f}: a color change needs the color`)
    }
    if (o.gcode != null) c.gcode = text(o.gcode, `${f}.gcode`)
    if (type === 'custom' && (c.gcode ?? '').trim() === '') throw new JobError(`${f}: a custom G-code needs its gcode`)
    if (o.note != null) c.note = text(o.note, `${f}.note`)
    return c
  })
  out.sort((a, b) => a.printZ - b.printZ)
  for (let i = 1; i < out.length; i++) {
    if (out[i]!.printZ - out[i - 1]!.printZ < Z_EPSILON) throw new JobError(`two custom G-codes at print Z ${out[i]!.printZ}; Orca runs one per layer`, { code: 'custom-gcode-invalid' })
  }
  return out
}

/**
 * Checks a job (parsed JSON or built in code) and returns it complete, with defaults filled in. Throws JobError naming the field.
 * Its paths must be absolute on `opts.platform`, the system the engine runs on (this one by default).
 */
export function validateJob(value: unknown, opts: { platform?: NodeJS.Platform } = {}): Job {
  const platform = opts.platform ?? process.platform
  const o = record(value, 'job')
  if (o.format !== JOB_FORMAT) throw new JobError(`format must be "${JOB_FORMAT}", got ${JSON.stringify(o.format)}`)
  const command = o.command
  if (typeof command !== 'string' || !(JOB_COMMANDS as readonly string[]).includes(command)) {
    throw new JobError(`command must be ${JOB_COMMANDS.join(', ')}, got ${JSON.stringify(command)}`)
  }
  const model = record(o.model, 'model')
  const presets = record(o.presets, 'presets')
  if (!Array.isArray(presets.filaments) || presets.filaments.length === 0) throw new JobError('presets.filaments must list at least one filament preset')
  let output: JobOutput
  if (o.output == null) {
    if (command === 'slice') throw new JobError('a slice job needs output.gcode')
    output = { gcode: '', bambu3mf: null, stats: false }
  } else {
    const out = record(o.output, 'output')
    const gcode = command === 'slice' || (out.gcode != null && out.gcode !== '') ? absPath(out.gcode, 'output.gcode', ['.gcode'], platform) : ''
    const bambu3mf = out.bambu3mf == null ? null : absPath(out.bambu3mf, 'output.bambu3mf', ['.3mf'], platform)
    output = { gcode, bambu3mf, stats: bool(out.stats, 'output.stats', true) }
  }
  return {
    format: JOB_FORMAT,
    command: command as JobCommand,
    resources: absPath(o.resources, 'resources', undefined, platform),
    workDir: absPath(o.workDir, 'workDir', undefined, platform),
    model: { path: absPath(model.path, 'model.path', MODEL_EXTENSIONS, platform) },
    presets: {
      machine: absPath(presets.machine, 'presets.machine', ['.json'], platform),
      process: absPath(presets.process, 'presets.process', ['.json'], platform),
      filaments: presets.filaments.map((p, i) => absPath(p, `presets.filaments[${i}]`, ['.json'], platform)),
    },
    overrides: normalizeOverrides(o.overrides, 'overrides'),
    objects: normalizeObjects(o.objects),
    customGcodes: normalizeCustomGcodes(o.customGcodes),
    arrange: bool(o.arrange, 'arrange', true),
    orient: bool(o.orient, 'orient', false),
    output,
  }
}

/** Fixed key order, two-space indent, LF. */
export function serializeJob(job: Job): string {
  return JSON.stringify(validateJob(job), null, 2) + '\n'
}

export function writeJob(path: string, job: Job | unknown): Job {
  const valid = validateJob(job)
  writeFileSync(path, JSON.stringify(valid, null, 2) + '\n')
  return valid
}

export function readJob(path: string): Job {
  let value: unknown
  try {
    value = JSON.parse(readFileSync(path, 'utf8'))
  } catch (err) {
    throw new JobError(`${path}: not JSON (${(err as Error).message})`)
  }
  return validateJob(value)
}

// ---------------------------------------------------------------- result

export interface ResultWarning {
  code: string
  message: string
  objectIndex?: number
}

export interface ResultLayers {
  objectIndex: number
  /** [bottom, top] of each layer, ascending. */
  ranges: [number, number][]
}

export interface FilamentUse {
  extruder: number
  mm: number
  cm3: number
  g: number
  cost: number
}

export interface ResultStats {
  timeSeconds: number
  layerSeconds: number[]
  filament: FilamentUse[]
  features: Record<string, { seconds: number; mm: number }>
}

/** One object of the model, as `info` returns it (millimetres, the model's own coordinates). */
export interface InfoObject {
  objectIndex: number
  name: string
  instances: number
  triangles: number
  min: [number, number, number]
  max: [number, number, number]
  size: [number, number, number]
}

/** What `info` returns: the model's objects, the bed and the preset names; other fields as the engine writes them. */
export interface ResultInfo {
  objects: InfoObject[]
  bed: Record<string, unknown>
  printer: string
  process: string
  filaments: string[]
  [key: string]: unknown
}

export interface JobResult {
  format: typeof RESULT_FORMAT
  ok: boolean
  code: string
  message: string
  /** What the engine exits with: 0 when ok, else the stock Orca command line's code for the failure. */
  exitCode?: number
  /** The object a failure is about. */
  objectIndex?: number
  /** The job's command, once the engine could read the job. */
  command?: JobCommand
  warnings: ResultWarning[]
  layers: ResultLayers[]
  printZ: number[]
  /** What a slice wrote; empty for layers and info, and for a failed run (which has no layers either). */
  outputs: { gcode: string; bambu3mf: string }
  stats?: ResultStats
  /** What `info` returns. */
  info?: ResultInfo
  /** The engine that wrote the result, as `version` prints it. */
  engine?: Record<string, unknown>
}

function rnum(v: unknown, field: string, min?: number): number {
  if (typeof v !== 'number' || !Number.isFinite(v)) throw new JobError(`result ${field} must be a number`)
  if (min !== undefined && v < min) throw new JobError(`result ${field} must be at least ${min}, got ${v}`)
  return v
}

function rlist(v: unknown, field: string): unknown[] {
  if (v === undefined) return []
  if (!Array.isArray(v)) throw new JobError(`result ${field} must be a list`)
  return v
}

function rtext(v: unknown, field: string): string {
  if (typeof v !== 'string') throw new JobError(`result ${field} must be text`)
  return v
}

function rwhole(v: unknown, field: string, min?: number): number {
  const n = rnum(v, field, min)
  if (!Number.isInteger(n)) throw new JobError(`result ${field} must be a whole number, got ${n}`)
  return n
}

function rvec3(v: unknown, field: string): [number, number, number] {
  if (!Array.isArray(v) || v.length !== 3) throw new JobError(`result ${field} must be [x, y, z]`)
  return [rnum(v[0], `${field}[0]`), rnum(v[1], `${field}[1]`), rnum(v[2], `${field}[2]`)]
}

function validateInfo(v: unknown): ResultInfo {
  if (!isRecord(v)) throw new JobError('result info must be an object')
  if (!Array.isArray(v.objects)) throw new JobError('result info.objects must be a list')
  const objects = v.objects.map((o, i): InfoObject => {
    const f = `info.objects[${i}]`
    if (!isRecord(o)) throw new JobError(`result ${f} must be an object`)
    return {
      objectIndex: rwhole(o.objectIndex, `${f}.objectIndex`, 0),
      name: rtext(o.name, `${f}.name`),
      instances: rwhole(o.instances, `${f}.instances`, 0),
      triangles: rwhole(o.triangles, `${f}.triangles`, 0),
      min: rvec3(o.min, `${f}.min`),
      max: rvec3(o.max, `${f}.max`),
      size: rvec3(o.size, `${f}.size`),
    }
  })
  if (!isRecord(v.bed)) throw new JobError('result info.bed must be an object')
  if (!Array.isArray(v.filaments)) throw new JobError('result info.filaments must be a list')
  const filaments = v.filaments.map((x, i) => rtext(x, `info.filaments[${i}]`))
  return { ...v, objects, bed: v.bed, printer: rtext(v.printer, 'info.printer'), process: rtext(v.process, 'info.process'), filaments }
}

/** Checks a parsed result and returns it typed; missing lists default to empty. */
export function validateResult(value: unknown): JobResult {
  if (!isRecord(value)) throw new JobError('result is not a JSON object')
  if (value.format !== RESULT_FORMAT) throw new JobError(`result format must be "${RESULT_FORMAT}", got ${JSON.stringify(value.format)}`)
  if (typeof value.ok !== 'boolean') throw new JobError('result ok must be true or false')
  const code = value.code
  if (typeof code !== 'string' || !CODE_RE.test(code)) throw new JobError(`result code must be a kebab-case name, got ${JSON.stringify(code)}`)
  if (value.ok && code !== 'ok') throw new JobError(`result is ok but its code is "${code}"`)
  if (!value.ok && code === 'ok') throw new JobError('result is not ok but its code is "ok"')
  if (value.message !== undefined && typeof value.message !== 'string') throw new JobError('result message must be text')
  const message = typeof value.message === 'string' ? value.message : ''
  const warnings = rlist(value.warnings, 'warnings').map((w, i): ResultWarning => {
    if (!isRecord(w) || typeof w.code !== 'string' || typeof w.message !== 'string') throw new JobError(`result warnings[${i}] needs code and message`)
    const out: ResultWarning = { code: w.code, message: w.message }
    if (w.objectIndex !== undefined) out.objectIndex = rnum(w.objectIndex, `warnings[${i}].objectIndex`, 0)
    return out
  })
  const layers = rlist(value.layers, 'layers').map((l, i): ResultLayers => {
    if (!isRecord(l)) throw new JobError(`result layers[${i}] must be an object`)
    const objectIndex = rnum(l.objectIndex, `layers[${i}].objectIndex`, 0)
    const ranges = rlist(l.ranges, `layers[${i}].ranges`).map((r, j): [number, number] => {
      if (!Array.isArray(r) || r.length !== 2) throw new JobError(`result layers[${i}].ranges[${j}] must be [bottom, top]`)
      const lo = rnum(r[0], `layers[${i}].ranges[${j}][0]`)
      const hi = rnum(r[1], `layers[${i}].ranges[${j}][1]`)
      if (!(hi > lo)) throw new JobError(`result layers[${i}].ranges[${j}] has top ${hi} not above bottom ${lo}`)
      return [lo, hi]
    })
    for (let j = 1; j < ranges.length; j++) {
      if (ranges[j]![1] <= ranges[j - 1]![1]) throw new JobError(`result layers[${i}].ranges[${j}] is not above the layer before it`)
    }
    return { objectIndex, ranges }
  })
  const printZ = rlist(value.printZ, 'printZ').map((z, i) => rnum(z, `printZ[${i}]`))
  for (let i = 1; i < printZ.length; i++) if (!(printZ[i]! > printZ[i - 1]!)) throw new JobError(`result printZ[${i}] ${printZ[i]} is not above ${printZ[i - 1]}`)
  if (value.outputs !== undefined && !isRecord(value.outputs)) throw new JobError('result outputs must be an object')
  const outs: Record<string, unknown> = isRecord(value.outputs) ? value.outputs : {}
  const outText = (k: string) => {
    const v = outs[k]
    if (v === undefined || v === null) return ''
    if (typeof v !== 'string') throw new JobError(`result outputs.${k} must be text`)
    return v
  }
  const result: JobResult = { format: RESULT_FORMAT, ok: value.ok, code, message, warnings, layers, printZ, outputs: { gcode: outText('gcode'), bambu3mf: outText('bambu3mf') } }
  if (value.exitCode !== undefined) {
    const exitCode = rwhole(value.exitCode, 'exitCode')
    if (value.ok && exitCode !== 0) throw new JobError(`result is ok but its exitCode is ${exitCode}`)
    if (!value.ok && exitCode === 0) throw new JobError('result is not ok but its exitCode is 0')
    result.exitCode = exitCode
  }
  if (value.objectIndex !== undefined) result.objectIndex = rwhole(value.objectIndex, 'objectIndex', 0)
  if (value.command !== undefined) {
    if (typeof value.command !== 'string' || !(JOB_COMMANDS as readonly string[]).includes(value.command)) {
      throw new JobError(`result command must be ${JOB_COMMANDS.join(', ')}, got ${JSON.stringify(value.command)}`)
    }
    result.command = value.command as JobCommand
  }
  if (value.info !== undefined && value.info !== null) result.info = validateInfo(value.info)
  if (value.engine !== undefined && value.engine !== null) {
    if (!isRecord(value.engine)) throw new JobError('result engine must be an object')
    result.engine = value.engine
  }
  if (value.stats !== undefined && value.stats !== null) {
    const s = value.stats
    if (!isRecord(s)) throw new JobError('result stats must be an object')
    const features: ResultStats['features'] = {}
    if (s.features !== undefined) {
      if (!isRecord(s.features)) throw new JobError('result stats.features must be an object')
      for (const [k, f] of Object.entries(s.features)) {
        if (!isRecord(f)) throw new JobError(`result stats.features["${k}"] must be an object`)
        features[k] = { seconds: rnum(f.seconds, `stats.features["${k}"].seconds`, 0), mm: rnum(f.mm, `stats.features["${k}"].mm`, 0) }
      }
    }
    result.stats = {
      timeSeconds: rnum(s.timeSeconds, 'stats.timeSeconds', 0),
      layerSeconds: rlist(s.layerSeconds, 'stats.layerSeconds').map((x, i) => rnum(x, `stats.layerSeconds[${i}]`, 0)),
      filament: rlist(s.filament, 'stats.filament').map((f, i): FilamentUse => {
        if (!isRecord(f)) throw new JobError(`result stats.filament[${i}] must be an object`)
        return {
          extruder: rnum(f.extruder, `stats.filament[${i}].extruder`, 1),
          mm: rnum(f.mm, `stats.filament[${i}].mm`, 0),
          cm3: rnum(f.cm3, `stats.filament[${i}].cm3`, 0),
          g: rnum(f.g, `stats.filament[${i}].g`, 0),
          cost: rnum(f.cost, `stats.filament[${i}].cost`, 0),
        }
      }),
      features,
    }
  }
  return result
}

export function parseResult(text: string): JobResult {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch (err) {
    throw new JobError(`result is not JSON (${(err as Error).message})`)
  }
  return validateResult(value)
}

export function readResult(path: string): JobResult {
  return parseResult(readFileSync(path, 'utf8'))
}

/**
 * One line of the progress file. The engine's stages, in order: job, presets, model, orient,
 * arrange, slice, gcode (the export, and Orca's own lines during it), 3mf, stats, then done at 100
 * when the run is ok, or failed at the percent reached, with the failure's message.
 */
export interface ProgressLine {
  stage: string
  percent: number
  message: string
}

/** The --progress file: one JSON object per line ({ stage, percent, message }); blank lines are skipped. */
export function parseProgress(text: string): ProgressLine[] {
  const out: ProgressLine[] = []
  text.split(/\r?\n/).forEach((line, i) => {
    if (line.trim() === '') return
    let v: unknown
    try {
      v = JSON.parse(line)
    } catch {
      throw new JobError(`progress line ${i + 1} is not JSON`)
    }
    if (!isRecord(v) || typeof v.stage !== 'string' || typeof v.percent !== 'number' || v.percent < 0 || v.percent > 100) {
      throw new JobError(`progress line ${i + 1} needs stage and a percent from 0 to 100`)
    }
    out.push({ stage: v.stage, percent: v.percent, message: typeof v.message === 'string' ? v.message : '' })
  })
  return out
}
