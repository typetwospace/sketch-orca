#!/usr/bin/env bun
// The static print configs of orca-config (src/print_config.rs), generated from OrcaSlicer's
// src/libslic3r/PrintConfig.hpp in the work folder (<work>/src/src/libslic3r/PrintConfig.hpp):
// every enum the header declares, and every class its PRINT_CONFIG_CLASS_DEFINE,
// PRINT_CONFIG_CLASS_DERIVED_DEFINE and PRINT_CONFIG_CLASS_DERIVED_DEFINE0 macros make
// (PrintObjectConfig, PrintRegionConfig, MachineEnvelopeConfig, GCodeConfig, PrintConfig,
// FullPrintConfig and the SLA classes), with their fields in the header's order. The order decides
// a class's hash (each field's hash, and each parent's, combined in that order), so the classes are
// never typed in by hand. tests/static_header.rs reads the header again and fails while the
// generated file differs from it. The enums and classes are OrcaSlicer's (AGPL-3.0), which the
// generated file says in its header. Written by orca-o3-front-1, 2026-11-13.
//
//   bun crates/orca-config/gen/print-config.ts            writes src/print_config.rs
//   bun crates/orca-config/gen/print-config.ts --check    exits 1 while the file differs
//   --header <path>                                       reads another PrintConfig.hpp
//
// The work folder is SKETCH_ORCA_HOME, else %LOCALAPPDATA%\SketchDev\orca-engine on Windows and
// ~/.local/share/SketchDev/orca-engine elsewhere (scripts/lib/paths.ts, workHome).

import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const CRATE = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const OUT = join(CRATE, 'src', 'print_config.rs')

/** The work folder, as scripts/lib/paths.ts's workHome finds it. */
function workHome(): string {
  if (process.env.SKETCH_ORCA_HOME) return resolve(process.env.SKETCH_ORCA_HOME)
  if (process.platform === 'win32') return join(process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'), 'SketchDev', 'orca-engine')
  return join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'SketchDev', 'orca-engine')
}

/** The C++ text with comments and preprocessor lines blanked (string and character literals kept). */
export function stripCpp(text: string): string {
  const src = text.replace(/\r\n/g, '\n')
  let out = ''
  let i = 0
  let lineStart = true
  while (i < src.length) {
    const c = src[i]!
    if (lineStart) {
      let j = i
      while (j < src.length && (src[j] === ' ' || src[j] === '\t')) j++
      if (src[j] === '#') {
        // A directive, with its continuation lines.
        while (j < src.length) {
          const nl = src.indexOf('\n', j)
          const end = nl < 0 ? src.length : nl
          const cont = src.slice(j, end).trimEnd().endsWith('\\')
          j = nl < 0 ? src.length : nl + 1
          out += '\n'
          if (!cont) break
        }
        i = j
        continue
      }
    }
    if (c === '/' && src[i + 1] === '/') {
      while (i < src.length && src[i] !== '\n') i++
      continue
    }
    if (c === '/' && src[i + 1] === '*') {
      i += 2
      while (i < src.length && !(src[i] === '*' && src[i + 1] === '/')) {
        if (src[i] === '\n') out += '\n'
        i++
      }
      i += 2
      continue
    }
    if (c === '"' || c === "'") {
      let j = i + 1
      while (j < src.length && src[j] !== c) j += src[j] === '\\' ? 2 : 1
      out += src.slice(i, j + 1)
      i = j + 1
      lineStart = false
      continue
    }
    out += c
    lineStart = c === '\n'
    i++
  }
  return out
}

export interface EnumDecl {
  name: string
  bytes: 1 | 4
  variants: { name: string; value: number }[]
  aliases: { name: string; of: string }[]
}

/** Every `enum [class] Name [: type] { ... };` of the stripped text, with each enumerator's value. */
export function parseEnums(text: string): EnumDecl[] {
  const out: EnumDecl[] = []
  const re = /\benum\s+(?:class\s+)?(\w+)\s*(?::\s*([\w ]+?))?\s*\{([^}]*)\}\s*;/g
  for (const m of text.matchAll(re)) {
    const name = m[1]!
    const under = (m[2] ?? 'int').trim().replace(/\s+/g, ' ')
    if (under !== 'int' && under !== 'unsigned char') throw new Error(`enum ${name}: underlying type ${under}`)
    const variants: EnumDecl['variants'] = []
    const aliases: EnumDecl['aliases'] = []
    const known = new Map<string, number>()
    let next = 0
    for (const raw of m[3]!.split(',')) {
      const item = raw.trim()
      if (item === '') continue
      const eq = item.indexOf('=')
      const vname = (eq < 0 ? item : item.slice(0, eq)).trim()
      let value = next
      if (eq >= 0) {
        const expr = item.slice(eq + 1).trim()
        if (/^-?\d+$/.test(expr)) value = Number(expr)
        else if (known.has(expr)) value = known.get(expr)!
        else throw new Error(`enum ${name}: ${vname} = ${expr}`)
      }
      if (!/^\w+$/.test(vname)) throw new Error(`enum ${name}: enumerator ${vname}`)
      const of = variants.find((v) => v.value === value)
      if (of) aliases.push({ name: vname, of: of.name })
      else variants.push({ name: vname, value })
      known.set(vname, value)
      next = value + 1
    }
    out.push({ name, bytes: under === 'unsigned char' ? 1 : 4, variants, aliases })
  }
  return out
}

export interface ClassDecl {
  name: string
  parents: string[]
  fields: { type: string; key: string }[]
}

/** The arguments of a macro call at `open` (the index of its `(`), split at top-level commas. */
function macroArgs(text: string, open: number): string[] {
  const args: string[] = []
  let depth = 0
  let start = open + 1
  for (let i = open; i < text.length; i++) {
    const c = text[i]
    if (c === '(') depth++
    else if (c === ')') {
      depth--
      if (depth === 0) {
        args.push(text.slice(start, i).trim())
        return args
      }
    } else if (c === ',' && depth === 1) {
      args.push(text.slice(start, i).trim())
      start = i + 1
    }
  }
  throw new Error('a macro call that does not close')
}

/** The `((Type, key))` items of a parameter sequence. */
function parseSeq(seq: string): ClassDecl['fields'] {
  const fields: ClassDecl['fields'] = []
  const rest = seq.replace(/\(\(\s*([^,()]+?)\s*,\s*(\w+)\s*\)\)/g, (_all, type: string, key: string) => {
    fields.push({ type: type.replace(/\s+/g, ''), key })
    return ''
  })
  if (rest.trim() !== '') throw new Error(`text left in a parameter sequence: ${rest.trim().slice(0, 80)}`)
  return fields
}

/** Every static class the header's PRINT_CONFIG_CLASS_* macros define, in the header's order. */
export function parseClasses(text: string): ClassDecl[] {
  const out: ClassDecl[] = []
  const re = /\b(PRINT_CONFIG_CLASS_DEFINE|PRINT_CONFIG_CLASS_DERIVED_DEFINE0|PRINT_CONFIG_CLASS_DERIVED_DEFINE)\s*\(/g
  for (const m of text.matchAll(re)) {
    const args = macroArgs(text, m.index! + m[0].length - 1)
    if (m[1] === 'PRINT_CONFIG_CLASS_DEFINE') {
      if (args.length !== 2) throw new Error(`${m[1]} with ${args.length} arguments`)
      out.push({ name: args[0]!, parents: [], fields: parseSeq(args[1]!) })
    } else {
      const want = m[1] === 'PRINT_CONFIG_CLASS_DERIVED_DEFINE0' ? 2 : 3
      if (args.length !== want) throw new Error(`${m[1]} with ${args.length} arguments`)
      const parents = args[1]!.replace(/^\(|\)$/g, '').split(',').map((s) => s.trim())
      out.push({ name: args[0]!, parents, fields: want === 3 ? parseSeq(args[2]!) : [] })
    }
  }
  return out
}

/** Each C++ option type of a static field: the Rust type, its ConfigOptionType and nullable. */
const OPTION_TYPES: Record<string, [string, string, boolean]> = {
  ConfigOptionFloat: ['ConfigOptionFloat', 'Float', false],
  ConfigOptionFloats: ['ConfigOptionFloats', 'Floats', false],
  ConfigOptionFloatsNullable: ['ConfigOptionFloatsNullable', 'Floats', true],
  ConfigOptionInt: ['ConfigOptionInt', 'Int', false],
  ConfigOptionInts: ['ConfigOptionInts', 'Ints', false],
  ConfigOptionIntsNullable: ['ConfigOptionIntsNullable', 'Ints', true],
  ConfigOptionString: ['ConfigOptionString', 'String', false],
  ConfigOptionStrings: ['ConfigOptionStrings', 'Strings', false],
  ConfigOptionPercent: ['ConfigOptionPercent', 'Percent', false],
  ConfigOptionPercents: ['ConfigOptionPercents', 'Percents', false],
  ConfigOptionPercentsNullable: ['ConfigOptionPercentsNullable', 'Percents', true],
  ConfigOptionFloatOrPercent: ['ConfigOptionFloatOrPercent', 'FloatOrPercent', false],
  ConfigOptionPoint: ['ConfigOptionPoint', 'Point', false],
  ConfigOptionPoints: ['ConfigOptionPoints', 'Points', false],
  ConfigOptionBool: ['ConfigOptionBool', 'Bool', false],
  ConfigOptionBools: ['ConfigOptionBools', 'Bools', false],
  ConfigOptionBoolsNullable: ['ConfigOptionBoolsNullable', 'Bools', true],
  ConfigOptionEnumsGeneric: ['ConfigOptionEnumsGeneric', 'Enums', false],
  ConfigOptionEnumsGenericNullable: ['ConfigOptionEnumsGenericNullable', 'Enums', true],
  ConfigOptionPointsGroups: ['ConfigOptionPointsGroups', 'PointsGroups', false],
}

/** The Rust type of a C++ option type (ConfigOptionEnum<T> takes the enum T). */
function rustType(type: string, enums: Map<string, EnumDecl>): string {
  const e = /^ConfigOptionEnum<(\w+)>$/.exec(type)
  if (e) {
    if (e[1] !== 'PrinterTechnology' && !enums.has(e[1]!)) throw new Error(`ConfigOptionEnum<${e[1]}>: no such enum in the header`)
    return `ConfigOptionEnum<${e[1]}>`
  }
  const t = OPTION_TYPES[type]
  if (!t) throw new Error(`an option type the generator does not know: ${type}`)
  return t[0]
}

/** A parent's field name: the class's name less `Config`, in snake case (GCodeConfig: gcode). */
export function parentField(cls: string): string {
  const base = cls.replace(/Config$/, '')
  return base
    .replace(/^SLA/, 'Sla')
    .replace(/^GCode/, 'Gcode')
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .toLowerCase()
}

const KEYWORDS = new Set(['as', 'break', 'const', 'continue', 'crate', 'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return', 'self', 'static', 'struct', 'super', 'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while', 'async', 'await', 'dyn', 'abstract', 'become', 'box', 'do', 'final', 'macro', 'override', 'priv', 'typeof', 'unsized', 'virtual', 'yield', 'try'])
const ident = (key: string) => (KEYWORDS.has(key) ? `r#${key}` : key)

/** The flattened fields of a class: its parents' (each flattened) in order, then its own, each with its path. */
function flatten(cls: ClassDecl, byName: Map<string, ClassDecl>): { key: string; type: string; path: string }[] {
  const out: { key: string; type: string; path: string }[] = []
  for (const p of cls.parents) {
    const pc = byName.get(p)
    if (!pc) throw new Error(`${cls.name}: no parent ${p}`)
    for (const f of flatten(pc, byName)) out.push({ ...f, path: `${parentField(p)}.${f.path}` })
  }
  for (const f of cls.fields) out.push({ key: f.key, type: f.type, path: ident(f.key) })
  return out
}

const q = (s: string) => JSON.stringify(s)
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0)

export function generate(header: string, headerName: string): string {
  const text = stripCpp(header)
  const enums = parseEnums(text)
  const enumByName = new Map(enums.map((e) => [e.name, e]))
  if (enumByName.size !== enums.length) throw new Error('an enum declared twice')
  const classes = parseClasses(text)
  const byName = new Map(classes.map((c) => [c.name, c]))
  if (classes.length === 0) throw new Error('no static class found')

  // The first field of each enum type, whose definition holds the enum's names.
  const keysOption = new Map<string, string>()
  for (const c of classes)
    for (const f of c.fields) {
      const e = /^ConfigOptionEnum<(\w+)>$/.exec(f.type)
      if (e && !keysOption.has(e[1]!)) keysOption.set(e[1]!, f.key)
    }

  const L: string[] = []
  L.push(
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
    '// Carries OrcaSlicer code: the enums and the static print config classes below (their fields, in',
    '// order, and the hash and operator== the PRINT_CONFIG_CLASS_* macros give them) are generated from',
    '// src/libslic3r/PrintConfig.hpp of OrcaSlicer v2.4.2, commit 8500fcd (lines 33-430 and 917-1880);',
    "// their copyright is the OrcaSlicer contributors', under the AGPL-3.0. Generated on 2026-11-13",
    '// (orca-o3-front-1) and whenever the header changes.',
    '//',
    `// Generated by \`bun crates/orca-config/gen/print-config.ts\` from ${headerName}. Do not edit it:`,
    '// tests/static_header.rs fails while it differs from the header.',
    '',
    "//! OrcaSlicer's static print configs ([`PrintObjectConfig`], [`PrintRegionConfig`],",
    '//! [`MachineEnvelopeConfig`], [`GCodeConfig`], [`PrintConfig`], [`FullPrintConfig`] and the SLA',
    "//! classes) and the enums of PrintConfig.hpp. A class's fields are its options in the header's",
    "//! order; a class with parents holds each parent as a field of its own (`print.gcode`, as",
    "//! `static_cast<const GCodeConfig&>(print)`), first, as the C++ lays them out. What a class does",
    '//! (defaults, keys, options by key, the hash) is [`StaticPrintConfig`]',
    '//! (`crate::static_config`).',
    '',
    'use std::sync::OnceLock;',
    '',
    'use crate::PrinterTechnology;',
    'use crate::option::{',
    '    ConfigEnum, ConfigOption, ConfigOptionBool, ConfigOptionBools, ConfigOptionBoolsNullable,',
    '    ConfigOptionEnum, ConfigOptionEnumsGeneric, ConfigOptionEnumsGenericNullable, ConfigOptionFloat,',
    '    ConfigOptionFloatOrPercent, ConfigOptionFloats, ConfigOptionFloatsNullable, ConfigOptionInt,',
    '    ConfigOptionInts, ConfigOptionIntsNullable, ConfigOptionPercent, ConfigOptionPercents,',
    '    ConfigOptionPercentsNullable, ConfigOptionPoint, ConfigOptionPoints, ConfigOptionPointsGroups,',
    '    ConfigOptionString, ConfigOptionStrings, hash_combine,',
    '};',
    'use crate::static_config::{StaticCache, StaticPrintConfig};',
    '',
  )

  // ---- enums
  for (const e of enums) {
    const zero = e.variants.find((v) => v.value === 0)
    L.push(`/// \`enum ${e.name}${e.bytes === 1 ? ' : unsigned char' : ''}\` (PrintConfig.hpp).`)
    L.push(`#[derive(Clone, Copy, Debug, ${zero ? 'Default, ' : ''}PartialEq, Eq, PartialOrd, Ord)]`)
    L.push(`#[repr(${e.bytes === 1 ? 'u8' : 'i32'})]`)
    L.push(`pub enum ${e.name} {`)
    for (const v of e.variants) {
      if (v === zero) L.push('    #[default]')
      L.push(`    ${v.name} = ${v.value},`)
    }
    L.push('}', '')
    if (e.aliases.length > 0) {
      L.push(`impl ${e.name} {`)
      for (const a of e.aliases) L.push(`    /// \`${a.name}\`, the same value as \`${a.of}\`.`, `    pub const ${a.name}: ${e.name} = ${e.name}::${a.of};`)
      L.push('}', '')
    }
    L.push(`impl ConfigEnum for ${e.name} {`)
    L.push(`    const NAME: &'static str = ${q(e.name)};`)
    L.push(`    const BYTES: usize = ${e.bytes};`)
    const ko = keysOption.get(e.name)
    L.push(`    const KEYS_OPTION: Option<&'static str> = ${ko ? `Some(${q(ko)})` : 'None'};`)
    L.push('    fn from_int(v: i32) -> Option<Self> {')
    L.push('        Some(match v {')
    for (const v of e.variants) L.push(`            ${v.value} => ${e.name}::${v.name},`)
    L.push('            _ => return None,')
    L.push('        })')
    L.push('    }')
    L.push('    fn to_int(self) -> i32 {')
    L.push('        self as i32')
    L.push('    }')
    L.push('}', '')
  }

  // ---- classes
  for (const c of classes) {
    const flat = flatten(c, byName)
    const seen = new Set<string>()
    for (const f of flat) {
      if (seen.has(f.key)) throw new Error(`${c.name}: ${f.key} twice`)
      seen.add(f.key)
    }
    const macro = c.parents.length === 0 ? 'PRINT_CONFIG_CLASS_DEFINE' : c.fields.length === 0 ? 'PRINT_CONFIG_CLASS_DERIVED_DEFINE0' : 'PRINT_CONFIG_CLASS_DERIVED_DEFINE'
    L.push(`/// \`${c.name}\` (PrintConfig.hpp, ${macro}${c.parents.length > 0 ? ` of ${c.parents.join(', ')}` : ''}): ${flat.length} option${flat.length === 1 ? '' : 's'}.`)
    L.push('#[derive(Clone, Debug, PartialEq)]')
    L.push(`pub struct ${c.name} {`)
    for (const p of c.parents) L.push(`    /// The \`${p}\` part.`, `    pub ${parentField(p)}: ${p},`)
    for (const f of c.fields) L.push(`    pub ${ident(f.key)}: ${rustType(f.type, enumByName)},`)
    L.push('}', '')
    L.push(`impl Default for ${c.name} {`)
    L.push('    /// `CLASS_NAME()`: a copy of the defaults.')
    L.push('    fn default() -> Self {')
    L.push('        Self::defaults().clone()')
    L.push('    }')
    L.push('}', '')
    L.push(`impl StaticPrintConfig for ${c.name} {`)
    L.push(`    const NAME: &'static str = ${q(c.name)};`)
    L.push(`    const FIELDS: &'static [&'static str] = &[`)
    for (const f of flat) L.push(`        ${q(f.key)},`)
    L.push('    ];')
    L.push(`    const BY_KEY: &'static [(&'static str, u16)] = &[`)
    const sorted = flat.map((f, i) => [f.key, i] as const).sort((a, b) => cmp(a[0], b[0]))
    for (const [k, i] of sorted) L.push(`        (${q(k)}, ${i}),`)
    L.push('    ];')
    L.push('    fn constructed() -> Self {')
    L.push('        Self {')
    for (const p of c.parents) L.push(`            ${parentField(p)}: ${p}::constructed(),`)
    for (const f of c.fields) L.push(`            ${ident(f.key)}: Default::default(),`)
    L.push('        }')
    L.push('    }')
    L.push('    fn cache() -> &\'static StaticCache<Self> {')
    L.push(`        static CACHE: OnceLock<StaticCache<${c.name}>> = OnceLock::new();`)
    L.push('        CACHE.get_or_init(StaticCache::new)')
    L.push('    }')
    L.push('    fn opt(&self, i: usize) -> &dyn ConfigOption {')
    L.push('        match i {')
    flat.forEach((f, i) => L.push(`            ${i} => &self.${f.path},`))
    L.push(`            _ => panic!("${c.name} has no option {i}"),`)
    L.push('        }')
    L.push('    }')
    L.push('    fn opt_mut(&mut self, i: usize) -> &mut dyn ConfigOption {')
    L.push('        match i {')
    flat.forEach((f, i) => L.push(`            ${i} => &mut self.${f.path},`))
    L.push(`            _ => panic!("${c.name} has no option {i}"),`)
    L.push('        }')
    L.push('    }')
    L.push('    fn hash(&self) -> u64 {')
    L.push('        let mut seed = 0u64;')
    for (const p of c.parents) L.push(`        hash_combine(&mut seed, StaticPrintConfig::hash(&self.${parentField(p)}));`)
    for (const f of c.fields) L.push(`        hash_combine(&mut seed, ConfigOption::hash(&self.${ident(f.key)}));`)
    L.push('        seed')
    L.push('    }')
    L.push('}', '')
  }

  // ---- what the header test compares
  L.push('/// A static class as the header declares it: its parents and its own fields (type, key).')
  L.push('#[derive(Clone, Copy, Debug, PartialEq, Eq)]')
  L.push('pub struct ClassDecl {')
  L.push("    pub name: &'static str,")
  L.push("    pub parents: &'static [&'static str],")
  L.push("    pub fields: &'static [(&'static str, &'static str)],")
  L.push('}', '')
  L.push('/// The classes, in the header\'s order.')
  L.push('pub const CLASSES: &[ClassDecl] = &[')
  for (const c of classes) {
    L.push('    ClassDecl {')
    L.push(`        name: ${q(c.name)},`)
    L.push(`        parents: &[${c.parents.map(q).join(', ')}],`)
    L.push('        fields: &[')
    for (const f of c.fields) L.push(`            (${q(f.type)}, ${q(f.key)}),`)
    L.push('        ],')
    L.push('    },')
  }
  L.push('];', '')
  L.push('/// An enum as the header declares it: its name, its size in bytes, its enumerators and values.')
  L.push('#[derive(Clone, Copy, Debug, PartialEq, Eq)]')
  L.push('pub struct EnumDecl {')
  L.push("    pub name: &'static str,")
  L.push('    pub bytes: usize,')
  L.push("    pub values: &'static [(&'static str, i32)],")
  L.push('}', '')
  L.push('/// The enums, in the header\'s order (an alias after the enumerator whose value it takes).')
  L.push('pub const ENUMS: &[EnumDecl] = &[')
  for (const e of enums) {
    const all = [...e.variants.map((v) => [v.name, v.value] as const)]
    for (const a of e.aliases) all.push([a.name, e.variants.find((v) => v.name === a.of)!.value])
    L.push('    EnumDecl {')
    L.push(`        name: ${q(e.name)},`)
    L.push(`        bytes: ${e.bytes},`)
    L.push(`        values: &[${all.map(([n, v]) => `(${q(n)}, ${v})`).join(', ')}],`)
    L.push('    },')
  }
  L.push('];', '')
  return L.join('\n')
}

function main(): number {
  const args = process.argv.slice(2)
  const check = args.includes('--check')
  const hi = args.indexOf('--header')
  const header = hi >= 0 ? resolve(args[hi + 1] ?? '') : join(workHome(), 'src', 'src', 'libslic3r', 'PrintConfig.hpp')
  if (!existsSync(header)) {
    console.error(`print-config.ts: ${header} does not exist (the work folder's OrcaSlicer source)`)
    return 2
  }
  const text = generate(readFileSync(header, 'utf8'), 'src/libslic3r/PrintConfig.hpp')
  if (check) {
    const now = existsSync(OUT) ? readFileSync(OUT, 'utf8') : ''
    if (now !== text) {
      console.error(`print-config.ts: ${OUT} differs from what ${header} gives`)
      return 1
    }
    console.log(`print-config.ts: ${OUT} is what ${header} gives`)
    return 0
  }
  writeFileSync(OUT, text)
  console.log(`print-config.ts: wrote ${OUT}`)
  return 0
}

if (import.meta.main) process.exit(main())
