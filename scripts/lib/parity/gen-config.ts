// The Rust translation's configuration definitions, generated from the configdef probe
// (parity/probes/configdef.json, parity/oracle/README.md "The probe program"): every option of
// Orca's print_config_def with every field the probe records, the preset sections' key lists, the
// keys handle_legacy names and the probe's values, as crates/orca-config/src/def_generated.rs. The
// text depends on the probe alone, byte for byte, so `parity gen-config --check` can tell whether
// the committed file is the one the probe gives. The definitions are OrcaSlicer's
// (src/libslic3r/PrintConfig.cpp, AGPL-3.0), which the generated file says in its header.

import { readFileSync } from 'node:fs'
import { join } from 'node:path'

export const CONFIGDEF_FORMAT = 'sketch-orca-probe-configdef/1'

/** Below the repository: the probe the definitions come from, and the file they go to. */
export const CONFIGDEF_PROBE = 'parity/probes/configdef.json'
export const DEF_GENERATED = 'crates/orca-config/src/def_generated.rs'

export interface ProbeOption {
  key: string
  optKey: string
  type: number
  nullable: boolean
  printerTechnology: number
  category: string
  label: string
  fullLabel: string
  tooltip: string
  sidetext: string
  cli: string
  cliParams: string
  ratioOver: string
  multiline: boolean
  fullWidth: boolean
  isCode: boolean
  readonly: boolean
  height: number
  width: number
  minBits: number
  maxBits: number
  min: number
  max: number
  maxLiteral: number
  mode: number
  guiType: number
  guiFlags: string
  aliases: string[]
  shortcut: string[]
  enumValues: string[]
  enumLabels: string[]
  enumKeysMap: Record<string, number> | null
  serializationKeyOrdinal: number
  defaultType: number | null
  default: string | null
  defaultThrow?: string
}

export interface ConfigDefProbe {
  format: string
  orcaVersion: string
  count: number
  options: ProbeOption[]
  printOptions: string[]
  filamentOptions: string[]
  printerOptions: string[]
  legacyKeys: string[]
  probeValues: string[]
  legacy: [string, string, string, string][]
}

/** ConfigOptionType's values, as the Rust OptionType names them. */
const TYPES: Record<number, string> = {
  0: 'None',
  1: 'Float',
  0x4001: 'Floats',
  2: 'Int',
  0x4002: 'Ints',
  3: 'String',
  0x4003: 'Strings',
  4: 'Percent',
  0x4004: 'Percents',
  5: 'FloatOrPercent',
  0x4005: 'FloatsOrPercents',
  6: 'Point',
  0x4006: 'Points',
  7: 'Point3',
  8: 'Bool',
  0x4008: 'Bools',
  9: 'Enum',
  0x4009: 'Enums',
  0x400a: 'PointsGroups',
  0x400b: 'IntsGroups',
}

const MODES = ['Simple', 'Advanced', 'Expert', 'Develop']
const GUI_TYPES = ['Undefined', 'IEnumOpen', 'FEnumOpen', 'Color', 'SelectOpen', 'Slider', 'Legend', 'OneString']
const TECHNOLOGIES = ['Fff', 'Sla', 'Unknown', 'Any']

/** ConfigOptionDef's defaults (Config.hpp), which OptionDef::EMPTY holds: a field equal to one is left out. */
const FLT_MAX_BITS = 0x7f7fffff
const NEG_FLT_MAX_BITS = 0xff7fffff

/** A Rust string literal of `s`: quotes, backslashes and control characters escaped, the rest as it is. */
export function rustString(s: string): string {
  let out = '"'
  for (const ch of s) {
    const c = ch.codePointAt(0)!
    if (ch === '"') out += '\\"'
    else if (ch === '\\') out += '\\\\'
    else if (ch === '\n') out += '\\n'
    else if (ch === '\r') out += '\\r'
    else if (ch === '\t') out += '\\t'
    else if (c < 0x20 || c === 0x7f) out += `\\u{${c.toString(16)}}`
    else out += ch
  }
  return out + '"'
}

/** A Rust f64 literal that reads back as `x`. */
export function rustF64(x: number): string {
  if (!Number.isFinite(x)) throw new Error(`not a finite number: ${x}`)
  if (Object.is(x, -0)) return '-0.0'
  const s = String(x)
  return /[.e]/.test(s) ? s : `${s}.0`
}

function hex32(n: number): string {
  return `0x${(n >>> 0).toString(16).padStart(8, '0')}`
}

function strings(list: string[]): string {
  return `&[${list.map(rustString).join(', ')}]`
}

function requireOf<T>(v: T | undefined, what: string): T {
  if (v === undefined) throw new Error(`the configdef probe has ${what}, which the generator does not know`)
  return v
}

function optionRust(o: ProbeOption): string {
  if (o.defaultThrow !== undefined) throw new Error(`${o.key}: the probe could not serialise its default (${o.defaultThrow})`)
  const f: string[] = [`key: ${rustString(o.key)}`, `opt_key: ${rustString(o.optKey)}`]
  f.push(`ty: OptionType::${requireOf(TYPES[o.type], `type ${o.type} (${o.key})`)}`)
  if (o.nullable) f.push('nullable: true')
  if (o.printerTechnology !== 0) f.push(`printer_technology: PrinterTechnology::${requireOf(TECHNOLOGIES[o.printerTechnology], `printer technology ${o.printerTechnology}`)}`)
  const text: [keyof ProbeOption, string][] = [
    ['category', 'category'],
    ['label', 'label'],
    ['fullLabel', 'full_label'],
    ['tooltip', 'tooltip'],
    ['sidetext', 'sidetext'],
    ['cli', 'cli'],
    ['cliParams', 'cli_params'],
    ['ratioOver', 'ratio_over'],
  ]
  for (const [k, field] of text) if (o[k] !== '') f.push(`${field}: ${rustString(o[k] as string)}`)
  const flags: [keyof ProbeOption, string][] = [
    ['multiline', 'multiline'],
    ['fullWidth', 'full_width'],
    ['isCode', 'is_code'],
    ['readonly', 'readonly'],
  ]
  for (const [k, field] of flags) if (o[k] === true) f.push(`${field}: true`)
  if (o.height !== -1) f.push(`height: ${o.height}`)
  if (o.width !== -1) f.push(`width: ${o.width}`)
  if (o.minBits !== NEG_FLT_MAX_BITS) f.push(`min: f32::from_bits(${hex32(o.minBits)})`)
  if (o.maxBits !== FLT_MAX_BITS) f.push(`max: f32::from_bits(${hex32(o.maxBits)})`)
  if (o.maxLiteral !== 1) f.push(`max_literal: ${rustF64(o.maxLiteral)}`)
  if (o.mode !== 0) f.push(`mode: Mode::${requireOf(MODES[o.mode], `mode ${o.mode}`)}`)
  if (o.guiType !== 0) f.push(`gui_type: GuiType::${requireOf(GUI_TYPES[o.guiType], `GUI type ${o.guiType}`)}`)
  if (o.guiFlags !== '') f.push(`gui_flags: ${rustString(o.guiFlags)}`)
  const lists: [keyof ProbeOption, string][] = [
    ['aliases', 'aliases'],
    ['shortcut', 'shortcut'],
    ['enumValues', 'enum_values'],
    ['enumLabels', 'enum_labels'],
  ]
  for (const [k, field] of lists) if ((o[k] as string[]).length > 0) f.push(`${field}: ${strings(o[k] as string[])}`)
  if (o.enumKeysMap !== null) {
    const pairs = Object.entries(o.enumKeysMap).map(([k, v]) => `(${rustString(k)}, ${v})`)
    f.push(`enum_keys_map: Some(&[${pairs.join(', ')}])`)
  }
  f.push(`serialization_key_ordinal: ${o.serializationKeyOrdinal}`)
  if (o.defaultType !== null) f.push(`default_type: Some(OptionType::${requireOf(TYPES[o.defaultType], `default type ${o.defaultType}`)})`)
  if (o.default !== null) f.push(`default: Some(${rustString(o.default)})`)
  return `    OptionDef {\n${f.map((x) => `        ${x},\n`).join('')}        ..OptionDef::EMPTY\n    },\n`
}

function listRust(name: string, doc: string, list: string[]): string {
  return `/// ${doc}\npub static ${name}: &[&str] = &[\n${list.map((k) => `    ${rustString(k)},\n`).join('')}];\n`
}

/** The text of def_generated.rs for the probe `doc`. */
export function configDefsRust(doc: ConfigDefProbe): string {
  if (doc.format !== CONFIGDEF_FORMAT) throw new Error(`the configdef probe is ${JSON.stringify(doc.format)}, not ${CONFIGDEF_FORMAT}`)
  if (doc.options.length !== doc.count) throw new Error(`the configdef probe says ${doc.count} options and has ${doc.options.length}`)
  const keys = doc.options.map((o) => o.key)
  for (let i = 1; i < keys.length; i++) {
    if (!(Buffer.compare(Buffer.from(keys[i - 1]!), Buffer.from(keys[i]!)) < 0)) throw new Error(`the configdef probe's options are not in print_config_def's order (std::map, by key): ${keys[i - 1]} before ${keys[i]}`)
  }
  const head = [
    '// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)',
    '// SPDX-License-Identifier: AGPL-3.0-only',
    '//',
    '// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:',
    '// you can redistribute it and/or modify it under the terms of the GNU Affero General Public',
    '// License, version 3, as published by the Free Software Foundation. It is distributed in the hope',
    '// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of',
    '// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License',
    '// (LICENSE) for more details.',
    '//',
    '// Carries OrcaSlicer code: the definitions below (keys, labels, tooltips, limits, enums, defaults)',
    "// are OrcaSlicer's own, from src/libslic3r/PrintConfig.cpp (PrintConfigDef) and",
    '// src/libslic3r/Preset.cpp (the preset key lists) of OrcaSlicer v2.4.2, commit 8500fcd, as the',
    '// configdef probe read them from the built library; their copyright is the OrcaSlicer',
    '// contributors\', under the AGPL-3.0. Generated on 2026-11-06 and whenever the probe changes.',
    '//',
    `// Generated by \`bun scripts/orca.ts parity gen-config\` from ${CONFIGDEF_PROBE} (${doc.format},`,
    `// OrcaSlicer ${doc.orcaVersion}). Do not edit it: \`parity gen-config --check\` fails while it differs`,
    '// from what the probe gives.',
    '',
    'use crate::{GuiType, Mode, OptionDef, OptionType, PrinterTechnology};',
    '',
    `/// print_config_def.options: all ${doc.count} options, in its order (by key).`,
    'pub static OPTIONS: &[OptionDef] = &[',
  ].join('\n')
  let out = `${head}\n`
  for (const o of doc.options) out += optionRust(o)
  out += '];\n\n'
  out += listRust('PRINT_OPTIONS', 'Preset::print_options(), in its order.', doc.printOptions) + '\n'
  out += listRust('FILAMENT_OPTIONS', 'Preset::filament_options(), in its order.', doc.filamentOptions) + '\n'
  out += listRust('PRINTER_OPTIONS', 'Preset::printer_options(), in its order.', doc.printerOptions) + '\n'
  out += listRust('LEGACY_KEYS', "The keys PrintConfigDef::handle_legacy names (cli/CMakeLists.txt's legacy_keys.inc), in its order.", doc.legacyKeys) + '\n'
  out += listRust('PROBE_VALUES', 'The values the probe read every key with through handle_legacy.', doc.probeValues)
  return out
}

/** Reads the probe below the repository `root`. */
export function readConfigDefProbe(root: string): ConfigDefProbe {
  return JSON.parse(readFileSync(join(root, ...CONFIGDEF_PROBE.split('/')), 'utf8')) as ConfigDefProbe
}
