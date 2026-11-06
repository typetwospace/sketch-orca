// The classes of an output (parity/CORPUS.md, "Classes"). `parity record` runs every corpus case
// with the oracle (time and seed pinned) at several thread counts P and with the engine as it ships
// five times; `parity goldens --check` runs the oracle again in a second work folder. Each output of
// each engine run (exit code, stdout, stderr, each file made, changed or removed) then gets a class:
//   H  it differs between the two folders (after each folder's own paths are written as <work>);
//   R  the engine differs between its own runs where no mask applies;
//   T  the oracle differs between thread counts (with the clock and seed pinned, nothing else moves);
//   N  the oracle differs from the engine where no mask applies, while neither moves by itself:
//      the oracle is not neutral there (parity neutral's gate, on the corpus);
//   E  the engine's runs, or the oracle and the engine, differ only in masked fields (masks.json);
//   B  byte for byte the same everywhere.
// An output takes the first class of H, R, T, N, E, B that holds, and `flags` lists all that hold.
// A family's classes are those of the outputs of the cases that exercise it (parity.json).

import { compareOutput, type Mask, type OutputStatus } from './masks.ts'

export const OUTPUT_CLASSES = ['H', 'R', 'T', 'N', 'E', 'B'] as const
export type OutputClass = (typeof OUTPUT_CLASSES)[number]

export interface OutputSides {
  /** The output's name as compareOutput's masks see it: stdout, stderr, exit, or the file's path. */
  name: string
  /** The oracle's bytes at each thread count, in the order of the thread counts (undefined: not written). */
  oracle: (Buffer | undefined)[]
  /** The engine's bytes in each of its runs. */
  engine: (Buffer | undefined)[]
  /** The oracle's bytes in the second folder (parity goldens --check), with paths made portable; absent when not run. */
  folder?: Buffer | undefined
  /** The oracle's bytes at the first thread count with the same paths made portable, for the folder comparison. */
  oraclePortable?: Buffer | undefined
  /** Whether `folder` was recorded (an output the second folder did not write is then a difference). */
  folderRan?: boolean
}

export interface Classified {
  class: OutputClass
  flags: OutputClass[]
  /** What the first difference of each flag was, by flag. */
  details: Partial<Record<OutputClass, string>>
}

function same(a: Buffer | undefined, b: Buffer | undefined): boolean {
  if (a === undefined || b === undefined) return a === b
  return a.equals(b)
}

/** The worst status across `others` compared against `first` (different over masked over identical). */
function worst(name: string, first: Buffer | undefined, others: (Buffer | undefined)[], masks: Mask[], labels: [string, string]): { status: OutputStatus; detail?: string } {
  let status: OutputStatus = 'identical'
  let detail: string | undefined
  for (const [i, o] of others.entries()) {
    const c = compareOutput(name, first, o, masks, [labels[0], `${labels[1]} ${i + 2}`])
    if (c.status === 'different') return { status: 'different', detail: c.detail ?? 'the bytes differ' }
    if (c.status === 'masked' && status === 'identical') {
      status = 'masked'
      detail = c.masks.join(', ')
    }
  }
  return { status, ...(detail ? { detail } : {}) }
}

/** The class of one output from its sides (see the top of this file). */
export function classify(s: OutputSides, masks: Mask[]): Classified {
  const flags = new Set<OutputClass>()
  const details: Classified['details'] = {}
  if (s.folderRan && !same(s.folder, s.oraclePortable)) {
    flags.add('H')
    details.H = s.folder === undefined || s.oraclePortable === undefined ? 'written in one folder only' : 'the bytes differ between the folders'
  }
  const [e0, ...eRest] = s.engine
  const engineSelf = worst(s.name, e0, eRest, masks, ['engine run', 'engine run'])
  if (engineSelf.status === 'different') {
    flags.add('R')
    details.R = engineSelf.detail!
  } else if (engineSelf.status === 'masked') {
    flags.add('E')
    details.E = engineSelf.detail!
  }
  const [o0, ...oRest] = s.oracle
  const t = oRest.findIndex((o) => !same(o0, o))
  if (t >= 0) {
    flags.add('T')
    details.T = `the oracle at the first thread count and at thread count number ${t + 2} differ`
  }
  if (!flags.has('R') && !flags.has('T') && s.engine.length > 0 && s.oracle.length > 0) {
    const c = compareOutput(s.name, o0, e0, masks, ['oracle', 'engine'])
    if (c.status === 'different') {
      flags.add('N')
      details.N = c.detail ?? 'the bytes differ'
    } else if (c.status === 'masked') {
      flags.add('E')
      details.E = details.E ?? c.masks.join(', ')
    }
  }
  const list = OUTPUT_CLASSES.filter((c) => flags.has(c))
  return { class: list[0] ?? 'B', flags: list.length > 0 ? list : ['B'], details }
}

/** The classes, from H to B, that a list of outputs' classes holds (each once, in that order). */
export function classesOf(classes: OutputClass[]): OutputClass[] {
  const set = new Set(classes)
  return OUTPUT_CLASSES.filter((c) => set.has(c))
}

/**
 * Text made portable between work folders: each of `roots` (a path and the name it is written as)
 * is replaced wherever it appears as written, with forward slashes, or JSON-escaped (backslashes
 * doubled). Longer roots first, so a case's folder goes before the folder that holds it.
 */
export function portableText(text: string, roots: [string, string][]): string {
  let out = text
  for (const [root, name] of [...roots].sort((a, b) => b[0].length - a[0].length)) {
    const forms = new Set([root, root.replace(/\\/g, '/'), root.replace(/\\/g, '\\\\'), root.replace(/\//g, '\\\\')])
    for (const f of [...forms].sort((a, b) => b.length - a.length)) if (f !== '') out = out.split(f).join(name)
  }
  return out
}

/**
 * portableText for bytes, read as UTF-8 byte for byte (each root is matched as its UTF-8 bytes):
 * text outputs only. A zip (a .3mf) or other binary output is left as it is, so a path inside one
 * makes the folders differ.
 */
export function portableBytes(b: Buffer | undefined, roots: [string, string][]): Buffer | undefined {
  if (b === undefined) return undefined
  if (b.length >= 4 && b.readUInt32LE(0) === 0x04034b50) return b
  if (b.includes(0)) return b
  const asBytes = (s: string) => Buffer.from(s, 'utf8').toString('latin1')
  return Buffer.from(
    portableText(
      b.toString('latin1'),
      roots.map(([r, n]) => [asBytes(r), asBytes(n)]),
    ),
    'latin1',
  )
}
