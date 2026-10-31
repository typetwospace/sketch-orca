import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { workHome, workPaths } from '../scripts/lib/paths.ts'
import {
  asUserPreset,
  checkPresetSet,
  DROPPED_KEYS,
  FILAMENT_LIBRARY,
  findPreset,
  flattenPreset,
  inheritsChain,
  loadProfileTree,
  presetSet,
  serializePreset,
  settingList,
  settingNumber,
  systemName,
  writePresetSet,
  type FlatPreset,
  type PresetSet,
  type ProfileTree,
} from '../scripts/lib/profiles.ts'
import { currentPlatform } from '../scripts/lib/toolchain.ts'

// A small profiles folder laid out like Orca's resources/profiles: an index per vendor bundle,
// preset files that hold only what differs from the preset they inherit.
const FILES: Record<string, unknown> = {
  'OrcaFilamentLibrary.json': {
    name: 'OrcaFilamentLibrary',
    filament_list: [
      { name: 'fdm_filament_common', sub_path: 'filament/fdm_filament_common.json' },
      { name: 'fdm_filament_pla', sub_path: 'filament/fdm_filament_pla.json' },
      { name: 'Generic PLA @System', sub_path: 'filament/Generic PLA @System.json' },
    ],
  },
  'OrcaFilamentLibrary/filament/fdm_filament_common.json': { type: 'filament', name: 'fdm_filament_common', from: 'system', instantiation: 'false', filament_type: ['PLA'], nozzle_temperature: ['200'], compatible_printers: [] },
  'OrcaFilamentLibrary/filament/fdm_filament_pla.json': { type: 'filament', name: 'fdm_filament_pla', inherits: 'fdm_filament_common', from: 'system', instantiation: 'false', nozzle_temperature: ['210'] },
  'OrcaFilamentLibrary/filament/Generic PLA @System.json': { type: 'filament', name: 'Generic PLA @System', inherits: 'fdm_filament_pla', from: 'system', setting_id: 'GFSL99', filament_id: 'GFL99', instantiation: 'true' },
  'Acme.json': {
    name: 'Acme',
    machine_list: [
      { name: 'fdm_machine_common', sub_path: 'machine/fdm_machine_common.json' },
      { name: 'Acme One 0.4 nozzle', sub_path: 'machine/Acme One 0.4 nozzle.json' },
      { name: 'Escapee', sub_path: '../OrcaFilamentLibrary.json' },
    ],
    process_list: [
      { name: 'fdm_process_common', sub_path: 'process/fdm_process_common.json' },
      { name: '0.20mm Standard @Acme', sub_path: 'process/0.20mm Standard @Acme.json' },
      { name: '0.20mm Condition @Acme', sub_path: 'process/0.20mm Condition @Acme.json' },
      { name: 'Shared Name', sub_path: 'process/shared.json' },
    ],
    filament_list: [
      { name: 'Acme PLA', sub_path: 'filament/Acme PLA.json' },
      { name: 'Wrong type', sub_path: 'filament/wrong type.json' },
      { name: 'Missing', sub_path: 'filament/missing.json' },
      { name: 'Acme PLA', sub_path: 'filament/Acme PLA copy.json' },
    ],
  },
  'Acme/machine/fdm_machine_common.json': {
    type: 'machine',
    name: 'fdm_machine_common',
    from: 'system',
    instantiation: 'false',
    nozzle_diameter: ['0.4'],
    min_layer_height: ['0.07'],
    max_layer_height: ['0.3'],
    machine_pause_gcode: 'M601',
    gcode_flavor: 'marlin',
  },
  'Acme/machine/Acme One 0.4 nozzle.json': {
    type: 'machine',
    name: 'Acme One 0.4 nozzle',
    inherits: 'fdm_machine_common',
    from: 'System',
    setting_id: 'GM001',
    renamed_from: 'Acme 1',
    description: 'a test printer',
    instantiation: 'true',
    printer_model: 'Acme One',
    max_layer_height: ['0.28'],
    machine_pause_gcode: 'PAUSE',
  },
  'Acme/process/fdm_process_common.json': { type: 'process', name: 'fdm_process_common', from: 'system', instantiation: false, layer_height: '0.2', initial_layer_print_height: '0.2', wall_loops: '2', compatible_printers: [] },
  'Acme/process/0.20mm Standard @Acme.json': { type: 'process', name: '0.20mm Standard @Acme', inherits: 'fdm_process_common', from: 'system', compatible_printers: ['Acme One 0.4 nozzle'], initial_layer_print_height: '0.25' },
  'Acme/process/0.20mm Condition @Acme.json': { type: 'process', name: '0.20mm Condition @Acme', inherits: 'fdm_process_common', from: 'system', compatible_printers_condition: 'printer_model=="Acme One"' },
  'Acme/process/shared.json': { type: 'process', name: 'Shared Name', from: 'system', compatible_printers: ['Acme One 0.4 nozzle'] },
  'Acme/filament/Acme PLA.json': { type: 'filament', name: 'Acme PLA', inherits: 'Generic PLA @System', from: 'system', compatible_printers: ['Acme One 0.4 nozzle'], nozzle_temperature: ['215'] },
  'Acme/filament/Acme PLA copy.json': { type: 'filament', name: 'Acme PLA', from: 'system', nozzle_temperature: ['999'] },
  'Acme/filament/wrong type.json': { type: 'process', name: 'Wrong type' },
  'Loopy.json': {
    name: 'Loopy',
    machine_list: [
      { name: 'Loop A', sub_path: 'machine/a.json' },
      { name: 'Loop B', sub_path: 'machine/b.json' },
      { name: 'Orphan', sub_path: 'machine/orphan.json' },
    ],
    process_list: [{ name: 'Shared Name', sub_path: 'process/shared.json' }],
  },
  'Loopy/machine/a.json': { type: 'machine', name: 'Loop A', inherits: 'Loop B', from: 'system' },
  'Loopy/machine/b.json': { type: 'machine', name: 'Loop B', inherits: 'Loop A', from: 'system' },
  'Loopy/machine/orphan.json': { type: 'machine', name: 'Orphan', inherits: 'Nobody', from: 'system' },
  'Loopy/process/shared.json': { type: 'process', name: 'Shared Name', from: 'system', compatible_printers: [] },
}

let root: string
let profiles: string
let tree: ProfileTree

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), 'orca-profiles-'))
  profiles = join(root, 'profiles')
  for (const [rel, value] of Object.entries(FILES)) {
    const p = join(profiles, ...rel.split('/'))
    mkdirSync(dirname(p), { recursive: true })
    writeFileSync(p, JSON.stringify(value, null, 4))
  }
  tree = loadProfileTree(profiles)
})

afterAll(() => {
  rmSync(root, { recursive: true, force: true })
})

const ACME = { vendor: 'Acme', machine: 'Acme One 0.4 nozzle', process: '0.20mm Standard @Acme', filaments: ['Acme PLA'] }

describe('profile tree', () => {
  test('reads every bundle, OrcaFilamentLibrary first, and notes what it cannot use', () => {
    expect(tree.vendors).toEqual([FILAMENT_LIBRARY, 'Acme', 'Loopy'])
    expect(tree.notes).toEqual([
      'Acme.json: "Escapee" points outside the vendor folder (../OrcaFilamentLibrary.json)',
      'Acme/filament/wrong type.json has type "process" but is listed in filament_list',
      expect.stringMatching(/^Acme\/filament\/missing\.json could not be read/),
      'Acme: filament "Acme PLA" is listed twice; the first is kept',
    ])
    expect(findPreset(tree, 'filament', 'Acme PLA', 'Acme').json.nozzle_temperature).toEqual(['215'])
  })

  test('loading only some vendors keeps OrcaFilamentLibrary, and an unknown vendor is refused', () => {
    expect(loadProfileTree(profiles, { vendors: ['Loopy'] }).vendors).toEqual([FILAMENT_LIBRARY, 'Loopy'])
    expect(() => loadProfileTree(profiles, { vendors: ['Nope'] })).toThrow(/no vendor bundle Nope\.json/)
    expect(() => loadProfileTree(join(root, 'none'))).toThrow(/no profiles folder/)
  })

  test('findPreset: by vendor, or the one selectable preset of that name', () => {
    expect(findPreset(tree, 'machine', 'Acme One 0.4 nozzle').vendor).toBe('Acme')
    expect(findPreset(tree, 'machine', 'fdm_machine_common').instantiable).toBe(false)
    expect(() => findPreset(tree, 'process', 'Shared Name')).toThrow(/exists in Acme, Loopy; name the vendor/)
    expect(findPreset(tree, 'process', 'Shared Name', 'Loopy').vendor).toBe('Loopy')
    expect(() => findPreset(tree, 'machine', 'Nope')).toThrow(/no machine preset named "Nope"/)
    expect(() => findPreset(tree, 'machine', 'Loop A', 'Acme')).toThrow(/no machine preset "Loop A" in Acme/)
  })

  test('inheritsChain walks to the root, crossing into OrcaFilamentLibrary', () => {
    const chain = inheritsChain(tree, findPreset(tree, 'filament', 'Acme PLA', 'Acme'))
    expect(chain.map((e) => `${e.vendor}/${e.name}`)).toEqual(['Acme/Acme PLA', 'OrcaFilamentLibrary/Generic PLA @System', 'OrcaFilamentLibrary/fdm_filament_pla', 'OrcaFilamentLibrary/fdm_filament_common'])
  })

  test('inheritsChain refuses a missing parent and a loop', () => {
    expect(() => inheritsChain(tree, findPreset(tree, 'machine', 'Orphan'))).toThrow(/inherits "Nobody", which is not a machine preset of Loopy or OrcaFilamentLibrary/)
    expect(() => inheritsChain(tree, findPreset(tree, 'machine', 'Loop A'))).toThrow(/loops \(Loop A -> Loop B -> Loop A\)/)
  })
})

describe('flattening', () => {
  test('the child wins key by key; bookkeeping keys go; from becomes system', () => {
    const m = flattenPreset(tree, 'machine', 'Acme One 0.4 nozzle')
    expect(m).toEqual({
      type: 'machine',
      name: 'Acme One 0.4 nozzle',
      from: 'system',
      nozzle_diameter: ['0.4'],
      min_layer_height: ['0.07'],
      max_layer_height: ['0.28'],
      machine_pause_gcode: 'PAUSE',
      gcode_flavor: 'marlin',
      printer_model: 'Acme One',
    })
    for (const k of DROPPED_KEYS) expect(k in m).toBe(false)
    expect(Object.keys(m).slice(0, 3)).toEqual(['type', 'name', 'from'])
  })

  test('a filament takes its parents from OrcaFilamentLibrary', () => {
    expect(flattenPreset(tree, 'filament', 'Acme PLA', 'Acme')).toEqual({
      type: 'filament',
      name: 'Acme PLA',
      from: 'system',
      filament_type: ['PLA'],
      nozzle_temperature: ['215'],
      compatible_printers: ['Acme One 0.4 nozzle'],
      filament_id: 'GFL99',
    })
  })

  test('flattened presets are copies: changing one leaves the tree alone', () => {
    const a = flattenPreset(tree, 'machine', 'Acme One 0.4 nozzle')
    ;(a.nozzle_diameter as string[])[0] = '0.6'
    expect(flattenPreset(tree, 'machine', 'Acme One 0.4 nozzle').nozzle_diameter).toEqual(['0.4'])
  })

  test('presetSet looks filaments up in the vendor, then in OrcaFilamentLibrary', () => {
    const set = presetSet(tree, { ...ACME, filaments: ['Acme PLA', 'Generic PLA @System'] })
    expect(set.machine.name).toBe('Acme One 0.4 nozzle')
    expect(set.process.initial_layer_print_height).toBe('0.25')
    expect(set.filaments.map((f) => f.name)).toEqual(['Acme PLA', 'Generic PLA @System'])
    expect(() => presetSet(tree, { ...ACME, process: 'Nope' })).toThrow(/no process preset "Nope" in Acme$/)
    expect(() => presetSet(tree, { ...ACME, filaments: ['Nope'] })).toThrow(/no filament preset "Nope" in Acme or OrcaFilamentLibrary/)
    expect(presetSet(tree, { machine: 'Acme One 0.4 nozzle', process: '0.20mm Standard @Acme', filaments: ['Acme PLA'] }).filaments[0]!.nozzle_temperature).toEqual(['215'])
  })

  test('settingNumber and settingList read Orca values', () => {
    const p = { a: '0.2', b: ['0.1', '0.3'], c: '15%', d: 'x;y; z', e: 7, f: [], g: '' }
    expect(settingNumber(p, 'a')).toBe(0.2)
    expect(settingNumber(p, 'b')).toBe(0.1)
    expect(settingNumber(p, 'b', 1)).toBe(0.3)
    expect(settingNumber(p, 'b', 5)).toBe(0.3)
    expect(settingNumber(p, 'c')).toBeUndefined()
    expect(settingNumber(p, 'e')).toBe(7)
    expect(settingNumber(p, 'g')).toBeUndefined()
    expect(settingNumber(p, 'missing')).toBeUndefined()
    expect(settingList(p, 'b')).toEqual(['0.1', '0.3'])
    expect(settingList(p, 'd')).toEqual(['x', 'y', 'z'])
    expect(settingList(p, 'f')).toEqual([])
    expect(settingList(p, 'missing')).toEqual([])
  })
})

describe('preset sets', () => {
  const acme = (): PresetSet => presetSet(tree, ACME)

  test('a matching set passes; a filament not listed for the machine is only a note', () => {
    expect(checkPresetSet(acme())).toEqual([])
    const set = acme()
    set.filaments[0] = { ...set.filaments[0]!, compatible_printers: ['Other Printer'] }
    expect(checkPresetSet(set)).toEqual(['filament 1 "Acme PLA" does not list "Acme One 0.4 nozzle" in its compatible_printers (Orca\'s command line does not check it)'])
  })

  test('a process that relies on its condition alone: the engine takes it with a note, the stock command line refuses it', () => {
    const s = acme()
    s.process = flattenPreset(tree, 'process', '0.20mm Condition @Acme')
    expect(checkPresetSet(s)).toEqual([
      'process "0.20mm Condition @Acme" lists no compatible_printers; the engine checks its condition "printer_model==\\"Acme One\\"" against "Acme One 0.4 nozzle" (the stock command line would refuse it)',
    ])
    expect(checkPresetSet(s, 'engine')).toEqual(checkPresetSet(s))
    expect(() => checkPresetSet(s, 'stock')).toThrow(/lists no compatible_printers \(it relies on the condition "printer_model==\\"Acme One\\""\).*-17/)
    // No list and no condition: Orca takes the process for every printer.
    const open = acme()
    open.process = { ...open.process, compatible_printers: [], compatible_printers_condition: '' }
    expect(checkPresetSet(open)).toEqual([])
    expect(() => checkPresetSet(open, 'stock')).toThrow(/lists no compatible_printers;.*-17/)
    expect(() => writePresetSet(join(root, 'stock-cond'), s, 'stock')).toThrow(/-17/)
  })

  test('refusals Orca\'s command line would make, made early with a plain message', () => {
    const cases: [(s: PresetSet) => void, RegExp][] = [
      [(s) => (s.process = { ...s.process, compatible_printers: ['Someone Else'] }), /is not for machine "Acme One 0\.4 nozzle": its compatible_printers are "Someone Else"/],
      [(s) => (s.machine = { ...s.machine, from: 'Vendor' as 'system' }), /"from" must be "system" or "User", got "Vendor"/],
      [(s) => (s.filaments = []), /at least one filament/],
      [(s) => (s.process = { ...s.process, type: 'machine' }), /process "0\.20mm Standard @Acme" is a machine preset/],
      [(s) => (s.machine = { ...s.machine, from: 'User' }), /names no system preset in "inherits"/],
    ]
    for (const [change, re] of cases) {
      const s = acme()
      change(s)
      expect(() => checkPresetSet(s)).toThrow(re)
      expect(() => checkPresetSet(s, 'stock')).toThrow(re)
    }
  })

  test('a user preset on a system one: from User, inherits the system name, which compatible_printers is matched against', () => {
    const set = acme()
    const mine = asUserPreset(set.machine, 'My Acme', { machine_pause_gcode: 'M0' })
    expect(mine).toMatchObject({ type: 'machine', name: 'My Acme', from: 'User', inherits: 'Acme One 0.4 nozzle', machine_pause_gcode: 'M0', nozzle_diameter: ['0.4'] })
    expect(systemName(mine)).toBe('Acme One 0.4 nozzle')
    expect(checkPresetSet({ ...set, machine: mine })).toEqual([])
    expect(() => asUserPreset(mine, 'Again')).toThrow(/is not a system preset/)
  })

  test('writePresetSet writes machine.json, process.json and filament_<n>.json with LF, and refuses a bad set first', () => {
    const dir = join(root, 'out')
    const set = acme()
    const files = writePresetSet(dir, set)
    expect(files).toEqual({ machine: join(dir, 'machine.json'), process: join(dir, 'process.json'), filaments: [join(dir, 'filament_1.json')], notes: [] })
    const text = readFileSync(files.machine, 'utf8')
    expect(text).toBe(serializePreset(set.machine))
    expect(text.includes('\r')).toBe(false)
    expect(JSON.parse(readFileSync(files.filaments[0]!, 'utf8'))).toEqual(set.filaments[0] as FlatPreset)
    expect(() => writePresetSet(join(root, 'bad'), { ...set, filaments: [] })).toThrow(/at least one filament/)
    expect(existsSync(join(root, 'bad'))).toBe(false)
  })
})

// The real profiles of the fetched Orca source, when this machine has them (skipped elsewhere).
const realProfiles = join(workPaths(workHome(process.env, currentPlatform())).src, 'resources', 'profiles')

describe.skipIf(!existsSync(realProfiles))('the fetched Orca profiles', () => {
  test('every bundle loads without notes, and the smoke presets flatten into sets Orca accepts', () => {
    expect(loadProfileTree(realProfiles).notes).toEqual([])
    const t = loadProfileTree(realProfiles, { vendors: ['Voron', 'BBL'] })
    const voron = presetSet(t, { vendor: 'Voron', machine: 'Voron 2.4 300 0.4 nozzle', process: '0.20mm Standard @Voron', filaments: ['Generic PLA @System'] })
    const bambu = presetSet(t, { vendor: 'BBL', machine: 'Bambu Lab X1 Carbon 0.4 nozzle', process: '0.20mm Standard @BBL X1C', filaments: ['Bambu PLA Basic @BBL X1C'] })
    for (const s of [voron, bambu]) {
      expect(checkPresetSet(s)).toEqual([])
      for (const p of [s.machine, s.process, ...s.filaments]) for (const k of DROPPED_KEYS) expect(k in p).toBe(false)
    }
    expect(voron.machine.gcode_flavor).toBe('klipper')
    expect(String(voron.machine.machine_pause_gcode).trim()).not.toBe('')
    expect(settingNumber(voron.process, 'layer_height')).toBe(0.2)
    expect(String(bambu.machine.machine_pause_gcode).trim()).not.toBe('')
  })
})
