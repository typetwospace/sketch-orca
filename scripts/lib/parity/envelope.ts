// The self-variance envelope: for each output parity record classed R (the engine as it ships
// differs between its own runs where no mask applies), what varies across its five runs: which
// lines, and for each number on such a line, by how much. It bounds what the Rust translation may
// differ by where the C++ engine itself is not repeatable. A text output is compared line by line;
// a line whose runs have the same text around their numbers gives each number's least and greatest
// value and their spread; any other line is recorded as text that varies. A binary output gives its
// sizes and how many bytes differ from the first run's.

import type { OutputRecord } from './record.ts'

export const ENVELOPE_FORMAT = 'sketch-orca-parity-envelope/1'

/** Lines recorded per output; the count of varying lines is always whole. */
export const ENVELOPE_MAX_LINES = 200

const NUMBER_RE = /[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g

export interface NumberSpread {
  /** The number's place on the line, from 0. */
  field: number
  min: number
  max: number
  spread: number
}

export interface VaryingLine {
  /** 1-based, in the first run. */
  line: number
  distinct: number
  /** Present when every run's line has the same text around its numbers. */
  numbers?: NumberSpread[]
  /** The first two texts, shortened, when the text itself varies. */
  texts?: string[]
}

export interface OutputEnvelope {
  case: string
  run: number
  output: string
  runs: number
  kind: 'text' | 'binary' | 'missing'
  /** Lines in each run (text). */
  lineCounts?: number[]
  varyingLines?: number
  lines?: VaryingLine[]
  /** Sizes in each run and bytes that differ from the first (binary). */
  sizes?: number[]
  differingBytes?: number[]
}

export interface EnvelopeReport {
  format: typeof ENVELOPE_FORMAT
  createdAt: string
  record: { file: string; createdAt: string }
  outputs: OutputEnvelope[]
}

function isText(b: Buffer): boolean {
  return !(b.length >= 4 && b.readUInt32LE(0) === 0x04034b50) && !b.includes(0)
}

/** The numbers of a line and the text around them (the numbers written as #). */
export function lineShape(line: string): { skeleton: string; numbers: number[] } {
  const numbers: number[] = []
  const skeleton = line.replace(NUMBER_RE, (m) => {
    numbers.push(Number(m))
    return '#'
  })
  return { skeleton, numbers }
}

/** What varies across the runs of one output (see the top of this file). */
export function outputEnvelope(o: Pick<OutputRecord, 'case' | 'run' | 'output'>, runs: (Buffer | undefined)[]): OutputEnvelope {
  const base = { case: o.case, run: o.run, output: o.output, runs: runs.length }
  if (runs.some((r) => r === undefined)) return { ...base, kind: 'missing', sizes: runs.map((r) => r?.length ?? -1) }
  const bufs = runs as Buffer[]
  if (!bufs.every(isText)) {
    const first = bufs[0]!
    return {
      ...base,
      kind: 'binary',
      sizes: bufs.map((b) => b.length),
      differingBytes: bufs.map((b) => {
        let n = Math.abs(b.length - first.length)
        for (let i = 0; i < Math.min(b.length, first.length); i++) if (b[i] !== first[i]) n++
        return n
      }),
    }
  }
  const texts = bufs.map((b) => b.toString('utf8').split(/\r?\n/))
  const count = Math.max(...texts.map((t) => t.length))
  const lines: VaryingLine[] = []
  let varying = 0
  for (let i = 0; i < count; i++) {
    const variants = texts.map((t) => t[i] ?? '')
    const distinct = new Set(variants).size
    if (distinct === 1) continue
    varying++
    if (lines.length >= ENVELOPE_MAX_LINES) continue
    const shapes = variants.map(lineShape)
    const sameShape = shapes.every((s) => s.skeleton === shapes[0]!.skeleton && s.numbers.length === shapes[0]!.numbers.length)
    if (sameShape) {
      const numbers: NumberSpread[] = []
      for (let f = 0; f < shapes[0]!.numbers.length; f++) {
        const vals = shapes.map((s) => s.numbers[f]!)
        const min = vals.reduce((a, b) => (b < a ? b : a))
        const max = vals.reduce((a, b) => (b > a ? b : a))
        if (min !== max) numbers.push({ field: f, min, max, spread: max - min })
      }
      lines.push({ line: i + 1, distinct, numbers })
    } else {
      lines.push({ line: i + 1, distinct, texts: [...new Set(variants)].slice(0, 2).map((t) => t.slice(0, 200)) })
    }
  }
  return { ...base, kind: 'text', lineCounts: texts.map((t) => t.length), varyingLines: varying, lines }
}
