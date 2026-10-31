// Orca's vendor presets, flattened into the one-file presets an engine job names.
//
// The tree is `resources/profiles` of the fetched source: `<Vendor>.json` indexes each bundle
// (machine_list, process_list and filament_list, each entry a name and a sub_path inside
// `<Vendor>/`), and each preset file holds only what differs from the preset it `inherits`.
// Flattening walks that chain to its root and lays the levels over each other, the child winning
// key by key, which is what Orca's PresetBundle does. A parent is looked up in the preset's own
// bundle first, then in OrcaFilamentLibrary, the shared filament bundle Orca loads first.
//
// The result keeps every setting and `type`, `name`, `filament_id` and `compatible_printers`;
// `from` becomes "system" (vendor files also say "System", "User" or nothing). Orca's command line
// takes only "system" or "User" there (anything else is a bad config, exit -5), and a "User" preset
// names its system preset in `inherits`. The bookkeeping keys `inherits`, `instantiation`,
// `setting_id`, `renamed_from` and `description` are dropped. Values are passed through as the
// vendor files write them (text, or lists of text), which is what Orca's JSON loader reads.

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { isAbsolute, join, relative } from 'node:path'
import { writeText } from './files.ts'

export const FILAMENT_LIBRARY = 'OrcaFilamentLibrary'

export type PresetKind = 'machine' | 'process' | 'filament'
export const PRESET_KINDS: readonly PresetKind[] = ['machine', 'process', 'filament']

const LIST_OF: Record<PresetKind, string> = { machine: 'machine_list', process: 'process_list', filament: 'filament_list' }

/** Bookkeeping keys Orca resolves itself; they never reach a flattened file. */
export const DROPPED_KEYS = ['inherits', 'instantiation', 'setting_id', 'renamed_from', 'description'] as const

export class ProfileError extends Error {}

export interface PresetEntry {
  kind: PresetKind
  /** The bundle: `<Vendor>.json` without `.json`. */
  vendor: string
  name: string
  /** sub_path inside the vendor folder, as the index gives it. */
  file: string
  json: Record<string, unknown>
  /** False for bases nobody selects (`"instantiation": "false"`). */
  instantiable: boolean
}

export interface ProfileTree {
  profilesDir: string
  /** Bundles read, OrcaFilamentLibrary first. */
  vendors: string[]
  entries: Map<string, PresetEntry>
  /** What could not be read, in plain words; loading goes on past it. */
  notes: string[]
}

/** A flattened preset as written for the engine. */
export type FlatPreset = Record<string, unknown> & { type: PresetKind; name: string; from: 'system' | 'User' }

function entryKey(kind: PresetKind, vendor: string, name: string): string {
  return `${kind}\u0000${vendor}\u0000${name}`
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function byText(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0
}

function inside(dir: string, full: string): boolean {
  const rel = relative(dir, full)
  return rel !== '' && !isAbsolute(rel) && rel.split(/[\\/]/)[0] !== '..'
}

/**
 * Reads the vendor bundles under `profilesDir` (all of them, or `vendors` plus OrcaFilamentLibrary).
 * Unreadable or mislisted files become notes; a vendor asked for by name must exist.
 */
export function loadProfileTree(profilesDir: string, opts: { vendors?: string[] } = {}): ProfileTree {
  if (!existsSync(profilesDir)) throw new ProfileError(`no profiles folder at ${profilesDir}`)
  let vendors = readdirSync(profilesDir)
    .filter((n) => n.toLowerCase().endsWith('.json'))
    .map((n) => n.slice(0, -5))
  if (opts.vendors) {
    for (const v of opts.vendors) if (!vendors.includes(v)) throw new ProfileError(`no vendor bundle ${v}.json in ${profilesDir}`)
    const wanted = new Set([...opts.vendors, FILAMENT_LIBRARY])
    vendors = vendors.filter((v) => wanted.has(v))
  }
  vendors.sort((a, b) => (a === FILAMENT_LIBRARY ? -1 : b === FILAMENT_LIBRARY ? 1 : byText(a, b)))
  const tree: ProfileTree = { profilesDir, vendors: [], entries: new Map(), notes: [] }
  for (const vendor of vendors) {
    let index: unknown
    try {
      index = JSON.parse(readFileSync(join(profilesDir, `${vendor}.json`), 'utf8'))
    } catch (err) {
      tree.notes.push(`${vendor}.json could not be read: ${(err as Error).message}`)
      continue
    }
    if (!isRecord(index)) {
      tree.notes.push(`${vendor}.json is not a vendor bundle`)
      continue
    }
    tree.vendors.push(vendor)
    const vendorDir = join(profilesDir, vendor)
    for (const kind of PRESET_KINDS) {
      const list = index[LIST_OF[kind]]
      if (list === undefined) continue
      if (!Array.isArray(list)) {
        tree.notes.push(`${vendor}.json: ${LIST_OF[kind]} is not a list`)
        continue
      }
      for (const item of list) {
        const listed = isRecord(item) && typeof item.name === 'string' ? item.name : undefined
        const subPath = isRecord(item) && typeof item.sub_path === 'string' ? item.sub_path : undefined
        if (!listed || !subPath) {
          tree.notes.push(`${vendor}.json: an entry of ${LIST_OF[kind]} has no name or sub_path`)
          continue
        }
        const full = join(vendorDir, subPath)
        if (!inside(vendorDir, full)) {
          tree.notes.push(`${vendor}.json: "${listed}" points outside the vendor folder (${subPath})`)
          continue
        }
        let json: unknown
        try {
          json = JSON.parse(readFileSync(full, 'utf8'))
        } catch (err) {
          tree.notes.push(`${vendor}/${subPath} could not be read: ${(err as Error).message}`)
          continue
        }
        if (!isRecord(json)) {
          tree.notes.push(`${vendor}/${subPath} is not a JSON object`)
          continue
        }
        if (json.type !== kind) {
          tree.notes.push(`${vendor}/${subPath} has type ${JSON.stringify(json.type)} but is listed in ${LIST_OF[kind]}`)
          continue
        }
        // Orca names a preset (and finds parents) by the file's own "name".
        const name = typeof json.name === 'string' && json.name !== '' ? json.name : listed
        const key = entryKey(kind, vendor, name)
        if (tree.entries.has(key)) {
          tree.notes.push(`${vendor}: ${kind} "${name}" is listed twice; the first is kept`)
          continue
        }
        const inst = json.instantiation
        tree.entries.set(key, { kind, vendor, name, file: subPath, json, instantiable: inst !== 'false' && inst !== false })
      }
    }
  }
  return tree
}

/** A preset by name: in `vendor` when given, else the one selectable preset of that name (bases only when no selectable one exists). */
export function findPreset(tree: ProfileTree, kind: PresetKind, name: string, vendor?: string): PresetEntry {
  if (vendor !== undefined) {
    const e = tree.entries.get(entryKey(kind, vendor, name))
    if (!e) throw new ProfileError(`no ${kind} preset "${name}" in ${vendor}`)
    return e
  }
  const all = [...tree.entries.values()].filter((e) => e.kind === kind && e.name === name)
  const selectable = all.filter((e) => e.instantiable)
  const pick = selectable.length > 0 ? selectable : all
  if (pick.length === 0) throw new ProfileError(`no ${kind} preset named "${name}" in ${tree.vendors.join(', ')}`)
  if (pick.length > 1) throw new ProfileError(`${kind} preset "${name}" exists in ${pick.map((e) => e.vendor).sort().join(', ')}; name the vendor`)
  return pick[0]!
}

function label(e: PresetEntry): string {
  return `${e.kind} "${e.name}" (${e.vendor}/${e.file})`
}

/** The preset and its ancestors, the preset first. Refuses a missing parent and a loop. */
export function inheritsChain(tree: ProfileTree, entry: PresetEntry): PresetEntry[] {
  const chain = [entry]
  const seen = new Set([entryKey(entry.kind, entry.vendor, entry.name)])
  for (let at = entry; ; ) {
    const parentName = at.json.inherits
    if (parentName === undefined || parentName === '') return chain
    if (typeof parentName !== 'string') throw new ProfileError(`${label(at)}: "inherits" is not text`)
    const parent = tree.entries.get(entryKey(at.kind, at.vendor, parentName)) ?? tree.entries.get(entryKey(at.kind, FILAMENT_LIBRARY, parentName))
    if (!parent) {
      const where = at.vendor === FILAMENT_LIBRARY ? FILAMENT_LIBRARY : `${at.vendor} or ${FILAMENT_LIBRARY}`
      throw new ProfileError(`${label(entry)}: ${at === entry ? 'it' : `its ancestor "${at.name}"`} inherits "${parentName}", which is not a ${at.kind} preset of ${where}`)
    }
    const key = entryKey(parent.kind, parent.vendor, parent.name)
    if (seen.has(key)) throw new ProfileError(`${label(entry)}: the inherits chain loops (${[...chain.map((c) => c.name), parent.name].join(' -> ')})`)
    seen.add(key)
    chain.push(parent)
    at = parent
  }
}

/** The preset with its whole inherits chain laid over each other, child winning, ready for the engine. */
export function flattenPreset(tree: ProfileTree, kind: PresetKind, name: string, vendor?: string): FlatPreset {
  const entry = findPreset(tree, kind, name, vendor)
  const merged: Record<string, unknown> = {}
  for (const level of inheritsChain(tree, entry).reverse()) {
    for (const [k, v] of Object.entries(level.json)) merged[k] = v
  }
  for (const k of DROPPED_KEYS) delete merged[k]
  delete merged.type
  delete merged.name
  delete merged.from
  return structuredClone({ type: kind, name: entry.name, from: 'system' as const, ...merged })
}

/**
 * A user preset on top of a flattened system preset: Orca's command line reads `from: "User"` and
 * takes the system name (for compatible_printers) from `inherits`.
 */
export function asUserPreset(base: FlatPreset, name: string, overrides: Record<string, unknown> = {}): FlatPreset {
  if (base.from !== 'system') throw new ProfileError(`"${base.name}" is not a system preset; a user preset must inherit a system one`)
  const rest: Record<string, unknown> = { ...structuredClone(base), ...structuredClone(overrides) }
  for (const k of ['type', 'name', 'from', 'inherits']) delete rest[k]
  return { type: base.type, name, from: 'User', inherits: base.name, ...rest }
}

/** The name Orca's command line matches compatible_printers against: a user preset's parent, else its own. */
export function systemName(p: FlatPreset): string {
  if (p.from === 'User') {
    if (typeof p.inherits !== 'string' || p.inherits === '') throw new ProfileError(`user preset "${p.name}" names no system preset in "inherits"`)
    return p.inherits
  }
  return p.name
}

/** A list setting as text: JSON lists as they are, a text value split at ";" (Orca's serialization). */
export function settingList(p: Record<string, unknown>, key: string): string[] {
  const v = p[key]
  if (Array.isArray(v)) return v.map(String).filter((s) => s !== '')
  if (typeof v === 'string') return v.split(';').map((s) => s.trim()).filter((s) => s !== '')
  if (typeof v === 'number') return [String(v)]
  return []
}

/** A numeric setting (`"0.2"`, `["0.1", "0.1"]`), element `index` of a list; undefined when absent or not a number (a percentage, say). */
export function settingNumber(p: Record<string, unknown>, key: string, index = 0): number | undefined {
  const v = p[key]
  let raw: unknown = v
  if (Array.isArray(v)) raw = v[Math.min(index, v.length - 1)]
  else if (typeof v === 'string' && v.includes(',') === false && v.includes(';')) raw = v.split(';')[index] ?? v.split(';')[0]
  if (typeof raw === 'number') return Number.isFinite(raw) ? raw : undefined
  if (typeof raw !== 'string' || raw.trim() === '' || !/^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/.test(raw.trim())) return undefined
  return Number(raw.trim())
}

export interface PresetSet {
  machine: FlatPreset
  process: FlatPreset
  filaments: FlatPreset[]
}

/** The presets of a job by name. */
export interface PresetSpec {
  /** The bundle of the machine and process; filaments are looked up there, then in OrcaFilamentLibrary. Without it each name must be unique. */
  vendor?: string
  machine: string
  process: string
  filaments: readonly string[]
}

/** Flattens the presets `spec` names. Only finds them; checkPresetSet checks that they go together. */
export function presetSet(tree: ProfileTree, spec: PresetSpec): PresetSet {
  const pick = (kind: PresetKind, name: string): FlatPreset => {
    if (spec.vendor === undefined) return flattenPreset(tree, kind, name)
    const bundles = kind === 'filament' ? [spec.vendor, FILAMENT_LIBRARY] : [spec.vendor]
    const vendor = bundles.find((v) => tree.entries.has(entryKey(kind, v, name)))
    if (vendor === undefined) throw new ProfileError(`no ${kind} preset "${name}" in ${bundles.join(' or ')}`)
    return flattenPreset(tree, kind, name, vendor)
  }
  return { machine: pick('machine', spec.machine), process: pick('process', spec.process), filaments: spec.filaments.map((f) => pick('filament', f)) }
}

/** The preset files a job names (absolute paths). */
export interface PresetFiles {
  machine: string
  process: string
  filaments: string[]
}

/**
 * Who slices a preset set: sketch-orca (the engine), or the stock Orca command line (what a client
 * may fall back on when no engine is built).
 */
export type PresetRunner = 'engine' | 'stock'

/**
 * The checks made before slicing, early and with a plain message: each file's type, `from` is
 * "system" or "User" (a user preset names its system preset), at least one filament, and the
 * process is made for the machine. A process that lists compatible_printers must list the machine.
 * One that lists none and relies on compatible_printers_condition is taken by the engine, which
 * evaluates the condition as Orca's GUI does (is_compatible_with_printer) and refuses it as
 * process-not-compatible when it does not hold; it gets a note here. The stock command line checks
 * the list only and refuses such a process (process not compatible, -17), so for `stock` it is
 * refused here too. Returns notes on what is not refused (a filament not listed for the machine).
 */
export function checkPresetSet(set: PresetSet, runner: PresetRunner = 'engine'): string[] {
  const notes: string[] = []
  const one = (p: FlatPreset, kind: PresetKind, what: string) => {
    if (p.type !== kind) throw new ProfileError(`${what} "${p.name}" is a ${String(p.type)} preset, not a ${kind} preset`)
    if (p.from !== 'system' && p.from !== 'User') throw new ProfileError(`${what} "${p.name}": "from" must be "system" or "User", got ${JSON.stringify(p.from)}`)
    if (typeof p.name !== 'string' || p.name === '') throw new ProfileError(`${what}: the preset has no name`)
    systemName(p)
  }
  one(set.machine, 'machine', 'machine')
  one(set.process, 'process', 'process')
  if (set.filaments.length === 0) throw new ProfileError('a job needs at least one filament preset')
  set.filaments.forEach((f, i) => one(f, 'filament', `filament ${i + 1}`))
  const machine = systemName(set.machine)
  const listed = settingList(set.process, 'compatible_printers')
  if (listed.length === 0) {
    const cond = typeof set.process.compatible_printers_condition === 'string' ? set.process.compatible_printers_condition.trim() : ''
    if (runner === 'stock') {
      throw new ProfileError(
        `process "${set.process.name}" lists no compatible_printers${cond ? ` (it relies on the condition ${JSON.stringify(cond)})` : ''}; Orca's command line refuses it (process not compatible, -17). List "${machine}" in its compatible_printers.`,
      )
    }
    if (cond) notes.push(`process "${set.process.name}" lists no compatible_printers; the engine checks its condition ${JSON.stringify(cond)} against "${machine}" (the stock command line would refuse it)`)
  } else if (!listed.includes(machine)) {
    throw new ProfileError(`process "${set.process.name}" is not for machine "${machine}": its compatible_printers are ${listed.map((n) => `"${n}"`).join(', ')}`)
  }
  set.filaments.forEach((f, i) => {
    const fl = settingList(f, 'compatible_printers')
    if (fl.length > 0 && !fl.includes(machine)) notes.push(`filament ${i + 1} "${f.name}" does not list "${machine}" in its compatible_printers (Orca's command line does not check it)`)
  })
  return notes
}

export function serializePreset(p: FlatPreset): string {
  return JSON.stringify(p, null, 2) + '\n'
}

/** Writes machine.json, process.json and filament_<n>.json into `dir` after checkPresetSet; returns their paths. */
export function writePresetSet(dir: string, set: PresetSet, runner: PresetRunner = 'engine'): PresetFiles & { notes: string[] } {
  const notes = checkPresetSet(set, runner)
  const machine = join(dir, 'machine.json')
  const process = join(dir, 'process.json')
  writeText(machine, serializePreset(set.machine))
  writeText(process, serializePreset(set.process))
  const filaments = set.filaments.map((f, i) => {
    const p = join(dir, `filament_${i + 1}.json`)
    writeText(p, serializePreset(f))
    return p
  })
  return { machine, process, filaments, notes }
}
