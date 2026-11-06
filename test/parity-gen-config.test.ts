// parity gen-config (scripts/lib/parity/gen-config.ts, cmd-gen-config.ts): the Rust definitions
// generated from the configdef probe. The committed crates/orca-config/src/def_generated.rs is what
// the committed probe gives, byte for byte; the text is a function of the probe alone; strings and
// numbers come out as Rust literals that read back as the probe's; and a probe the generator does
// not know is refused, never guessed at. Hermetic: it reads the repository and writes nothing.

import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { checkParityModule } from '../scripts/lib/parity/dispatch.ts'
import { configDefsRust, DEF_GENERATED, readConfigDefProbe, rustF64, rustString, type ConfigDefProbe, type ProbeOption } from '../scripts/lib/parity/gen-config.ts'
import * as cmd from '../scripts/lib/parity/cmd-gen-config.ts'

const ROOT = join(import.meta.dir, '..')

function option(over: Partial<ProbeOption>): ProbeOption {
  return {
    key: 'k',
    optKey: 'k',
    type: 1,
    nullable: false,
    printerTechnology: 0,
    category: '',
    label: '',
    fullLabel: '',
    tooltip: '',
    sidetext: '',
    cli: '',
    cliParams: '',
    ratioOver: '',
    multiline: false,
    fullWidth: false,
    isCode: false,
    readonly: false,
    height: -1,
    width: -1,
    minBits: 0xff7fffff,
    maxBits: 0x7f7fffff,
    min: -3.4028234663852886e38,
    max: 3.4028234663852886e38,
    maxLiteral: 1,
    mode: 0,
    guiType: 0,
    guiFlags: '',
    aliases: [],
    shortcut: [],
    enumValues: [],
    enumLabels: [],
    enumKeysMap: null,
    serializationKeyOrdinal: 0,
    defaultType: 1,
    default: '0',
    ...over,
  }
}

function probe(options: ProbeOption[]): ConfigDefProbe {
  return {
    format: 'sketch-orca-probe-configdef/1',
    orcaVersion: '2.4.2',
    count: options.length,
    options,
    printOptions: ['k'],
    filamentOptions: [],
    printerOptions: [],
    legacyKeys: ['old'],
    probeValues: ['', '1'],
    legacy: [],
  }
}

describe('gen-config', () => {
  test('the committed def_generated.rs is what the committed probe gives', () => {
    const text = configDefsRust(readConfigDefProbe(ROOT))
    expect(text === readFileSync(join(ROOT, ...DEF_GENERATED.split('/')), 'utf8')).toBe(true)
  })

  test('the text depends on the probe alone', () => {
    const p = readConfigDefProbe(ROOT)
    expect(configDefsRust(p)).toBe(configDefsRust(structuredClone(p)))
    expect(configDefsRust(p)).toContain('Carries OrcaSlicer code')
  })

  test('a field equal to ConfigOptionDef\'s default is left to OptionDef::EMPTY', () => {
    const text = configDefsRust(probe([option({})]))
    const block = text.slice(text.indexOf('    OptionDef {'), text.indexOf('..OptionDef::EMPTY'))
    expect(block).not.toContain('min:')
    expect(block).not.toContain('tooltip:')
    expect(block).toContain('ty: OptionType::Float')
    const full = configDefsRust(probe([option({ minBits: 0, maxBits: 0x42c80000, maxLiteral: 0.5, mode: 2, guiType: 3, nullable: true, type: 0x4001, enumKeysMap: { a: 0, b: 1 } })]))
    expect(full).toContain('min: f32::from_bits(0x00000000)')
    expect(full).toContain('max: f32::from_bits(0x42c80000)')
    expect(full).toContain('max_literal: 0.5')
    expect(full).toContain('mode: Mode::Expert')
    expect(full).toContain('gui_type: GuiType::Color')
    expect(full).toContain('nullable: true')
    expect(full).toContain('ty: OptionType::Floats')
    expect(full).toContain('enum_keys_map: Some(&[("a", 0), ("b", 1)])')
  })

  test('strings and numbers are Rust literals that read back as the probe\'s', () => {
    expect(rustString('a"b\\c\nd\re\tf\u0001\u007fé')).toBe('"a\\"b\\\\c\\nd\\re\\tf\\u{1}\\u{7f}é"')
    expect(rustF64(1)).toBe('1.0')
    expect(rustF64(-0)).toBe('-0.0')
    expect(rustF64(0.1)).toBe('0.1')
    expect(rustF64(1e300)).toBe('1e+300')
    expect(() => rustF64(Number.NaN)).toThrow()
  })

  test('a probe the generator does not know is refused', () => {
    expect(() => configDefsRust({ ...probe([option({})]), format: 'other/1' })).toThrow(/not sketch-orca-probe-configdef\/1/)
    expect(() => configDefsRust({ ...probe([option({})]), count: 2 })).toThrow(/says 2 options/)
    expect(() => configDefsRust(probe([option({ key: 'b' }), option({ key: 'a' })]))).toThrow(/not in print_config_def's order/)
    expect(() => configDefsRust(probe([option({ type: 99 })]))).toThrow(/type 99/)
    expect(() => configDefsRust(probe([option({ defaultThrow: 'x' })]))).toThrow(/could not serialise its default/)
  })

  test('the command is a parity command that needs no work folder', () => {
    const c = checkParityModule(cmd, 'gen-config')
    expect(c.locked).toBe(false)
    expect(c.usage).toStartWith('gen-config [--check]')
  })
})
