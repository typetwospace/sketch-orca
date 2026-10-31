// Reading the G-code Orca writes, for the smoke checks: layer changes, pauses and custom G-code
// blocks (Orca's reserved tags), the extruding moves of each layer, and the settings of its config
// block.
//
// Orca writes two tag styles (GCodeProcessor::Reserved_Tags): Bambu printers get
// "; CHANGE_LAYER", "; Z_HEIGHT: z", "; LAYER_HEIGHT: h", "; PAUSE_PRINTING", "; CUSTOM_GCODE";
// every other printer ";LAYER_CHANGE", ";Z:z", ";HEIGHT:h", ";PAUSE_PRINT", ";CUSTOM_GCODE".
// A layer's tags come first, then its Z move and layer-change G-code, then a custom G-code item
// for that print Z (a pause: the tag and the machine's pause G-code), then its extrusions.

export type TagStyle = 'bbl' | 'plain'

export interface GcodeLayer {
  /** 0-based, in file order. */
  index: number
  z: number
  height: number | undefined
  /** 0-based line of the layer-change tag. */
  line: number
}

export interface GcodeScan {
  style: TagStyle | undefined
  layers: GcodeLayer[]
  /** 0-based lines of pause tags. */
  pauses: number[]
  /** 0-based lines of custom G-code tags. */
  customs: number[]
  lines: string[]
}

const TAGS = {
  plain: { layer: ';LAYER_CHANGE', z: /^;Z:\s*(-?[\d.]+(?:e-?\d+)?)\s*$/i, height: /^;HEIGHT:\s*([\d.]+(?:e-?\d+)?)\s*$/i, pause: ';PAUSE_PRINT', custom: ';CUSTOM_GCODE' },
  bbl: { layer: '; CHANGE_LAYER', z: /^; Z_HEIGHT:\s*(-?[\d.]+(?:e-?\d+)?)\s*$/i, height: /^; LAYER_HEIGHT:\s*([\d.]+(?:e-?\d+)?)\s*$/i, pause: '; PAUSE_PRINTING', custom: '; CUSTOM_GCODE' },
} as const

export function splitLines(text: string): string[] {
  return text.split('\n').map((l) => (l.endsWith('\r') ? l.slice(0, -1) : l))
}

/** Finds the layer changes, pauses and custom G-code blocks. The Z of a layer is read from the tags right after its layer-change tag. */
export function scanGcode(text: string): GcodeScan {
  const lines = splitLines(text)
  const scan: GcodeScan = { style: undefined, layers: [], pauses: [], customs: [], lines }
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!.trimEnd()
    let style: TagStyle | undefined
    if (line === TAGS.plain.layer) style = 'plain'
    else if (line === TAGS.bbl.layer) style = 'bbl'
    if (style) {
      scan.style ??= style
      const tags = TAGS[style]
      let z: number | undefined
      let height: number | undefined
      for (let j = i + 1; j < Math.min(lines.length, i + 6); j++) {
        const t = lines[j]!.trimEnd()
        // The tags of this layer only: the first of each, never the next layer's.
        if (t === TAGS.plain.layer || t === TAGS.bbl.layer) break
        const mz = z === undefined ? tags.z.exec(t) : null
        if (mz) z = Number(mz[1])
        const mh = height === undefined ? tags.height.exec(t) : null
        if (mh) height = Number(mh[1])
      }
      if (z === undefined) throw new Error(`G-code line ${i + 1}: a layer change without its Z tag`)
      scan.layers.push({ index: scan.layers.length, z, height, line: i })
      continue
    }
    if (line === TAGS.plain.pause || line === TAGS.bbl.pause) scan.pauses.push(i)
    else if (line === TAGS.plain.custom || line === TAGS.bbl.custom) scan.customs.push(i)
  }
  return scan
}

/** The layer a line belongs to: the last layer change at or before it (undefined before the first). */
export function layerAtLine(scan: GcodeScan, line: number): GcodeLayer | undefined {
  let lo = 0
  let hi = scan.layers.length - 1
  let found: GcodeLayer | undefined
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (scan.layers[mid]!.line <= line) {
      found = scan.layers[mid]
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  return found
}

/** The next `count` lines after `line` that are commands: no comments, blank lines or M73 progress lines. */
export function commandsAfter(lines: string[], line: number, count: number): string[] {
  const out: string[] = []
  for (let i = line + 1; i < lines.length && out.length < count; i++) {
    const cmd = lines[i]!.replace(/;.*$/, '').trim()
    if (cmd === '' || /^M73\b/i.test(cmd)) continue
    out.push(cmd)
  }
  return out
}

/**
 * A setting as the G-code's config block gives it: the line "; key = value" between
 * "; CONFIG_BLOCK_START" and "; CONFIG_BLOCK_END", where Orca writes the whole configuration it
 * sliced with. Undefined when the block or the key is not there.
 */
export function configValue(lines: string[], key: string): string | undefined {
  const prefix = `; ${key} =`
  let inside = false
  for (const raw of lines) {
    const line = raw.trimEnd()
    if (line === '; CONFIG_BLOCK_START') inside = true
    else if (line === '; CONFIG_BLOCK_END') inside = false
    else if (inside && line.startsWith(prefix) && (line.length === prefix.length || line[prefix.length] === ' ')) return line.slice(prefix.length + 1)
  }
  return undefined
}

export interface Extrusion {
  /** 0-based line of the move. */
  line: number
  /** Index into GcodeScan.layers, -1 before the first layer change. */
  layer: number
  x0: number
  y0: number
  x1: number
  y1: number
  /** The nozzle's Z at the end of the move, as the G-code moves it (print Z plus the printer's z_offset); 0 until the G-code sets Z. */
  z: number
  /** The feature the move prints, from the last ";TYPE:" (Bambu: "; FEATURE: ") tag before it; absent before the first. */
  role?: string
}

/** Orca's feature tags: ";TYPE:Outer wall", and "; FEATURE: Outer wall" for Bambu printers. */
const ROLE_TAG = /^;\s*(?:TYPE|FEATURE):\s*(.*?)\s*$/

function words(cmd: string): Map<string, number> {
  const out = new Map<string, number>()
  for (const m of cmd.matchAll(/([A-Za-z])\s*([-+]?(?:\d+\.?\d*|\.\d+))/g)) out.set(m[1]!.toUpperCase(), Number(m[2]))
  return out
}

/**
 * Every move that extrudes (E goes up while X or Y changes), with its layer and Z. Follows G90/G91,
 * M82/M83 and G92; arcs (G2/G3 with I/J) become chords of at most 10 degrees.
 */
export function extrusions(scan: GcodeScan): Extrusion[] {
  const out: Extrusion[] = []
  let absXYZ = true
  let absE = true
  let x = 0
  let y = 0
  let z = 0
  let e = 0
  let layer = -1
  let nextLayer = 0
  let role: string | undefined
  const push = (m: Extrusion) => out.push(role === undefined ? m : { ...m, role })
  for (let i = 0; i < scan.lines.length; i++) {
    while (nextLayer < scan.layers.length && scan.layers[nextLayer]!.line <= i) layer = nextLayer++
    const tag = ROLE_TAG.exec(scan.lines[i]!)
    if (tag) role = tag[1]
    const cmd = scan.lines[i]!.replace(/;.*$/, '').trim()
    if (cmd === '') continue
    const head = /^([GM])(\d+)(?:\.\d+)?/i.exec(cmd)
    if (!head) continue
    const code = `${head[1]!.toUpperCase()}${Number(head[2])}`
    const w = words(cmd.slice(head[0].length))
    switch (code) {
      case 'G90':
        absXYZ = true
        absE = true
        continue
      case 'G91':
        absXYZ = false
        absE = false
        continue
      case 'M82':
        absE = true
        continue
      case 'M83':
        absE = false
        continue
      case 'G92':
        if (w.has('X')) x = w.get('X')!
        if (w.has('Y')) y = w.get('Y')!
        if (w.has('Z')) z = w.get('Z')!
        if (w.has('E')) e = w.get('E')!
        continue
      case 'G0':
      case 'G1':
      case 'G2':
      case 'G3': {
        const nx = w.has('X') ? (absXYZ ? w.get('X')! : x + w.get('X')!) : x
        const ny = w.has('Y') ? (absXYZ ? w.get('Y')! : y + w.get('Y')!) : y
        const nz = w.has('Z') ? (absXYZ ? w.get('Z')! : z + w.get('Z')!) : z
        let de = 0
        if (w.has('E')) {
          de = absE ? w.get('E')! - e : w.get('E')!
          e = absE ? w.get('E')! : e + de
        }
        const moved = nx !== x || ny !== y || code === 'G2' || code === 'G3'
        if (de > 1e-9 && moved) {
          if (code === 'G2' || code === 'G3') {
            const cx = x + (w.get('I') ?? 0)
            const cy = y + (w.get('J') ?? 0)
            const r = Math.hypot(x - cx, y - cy)
            const a0 = Math.atan2(y - cy, x - cx)
            let a1 = Math.atan2(ny - cy, nx - cx)
            if (code === 'G3' && a1 <= a0) a1 += 2 * Math.PI
            if (code === 'G2' && a1 >= a0) a1 -= 2 * Math.PI
            const steps = Math.max(1, Math.ceil(Math.abs(a1 - a0) / (Math.PI / 18)))
            let px = x
            let py = y
            for (let s = 1; s <= steps; s++) {
              const a = a0 + ((a1 - a0) * s) / steps
              const qx = s === steps ? nx : cx + r * Math.cos(a)
              const qy = s === steps ? ny : cy + r * Math.sin(a)
              // A helical arc climbs evenly along its chords.
              push({ line: i, layer, x0: px, y0: py, x1: qx, y1: qy, z: s === steps ? nz : z + ((nz - z) * s) / steps })
              px = qx
              py = qy
            }
          } else {
            push({ line: i, layer, x0: x, y0: y, x1: nx, y1: ny, z: nz })
          }
        }
        x = nx
        y = ny
        z = nz
        continue
      }
      default:
        continue
    }
  }
  return out
}

/** The XY length of the extrusions of each feature, in mm (moves before the first feature tag are left out). */
export function featureLengths(moves: Extrusion[]): Map<string, number> {
  const out = new Map<string, number>()
  for (const m of moves) if (m.role !== undefined) out.set(m.role, (out.get(m.role) ?? 0) + Math.hypot(m.x1 - m.x0, m.y1 - m.y0))
  return out
}

export interface Rect {
  minX: number
  minY: number
  maxX: number
  maxY: number
}

/** Whether a segment passes through the inside of a rectangle (Liang-Barsky clipping). */
export function segmentHitsRect(x0: number, y0: number, x1: number, y1: number, r: Rect): boolean {
  let t0 = 0
  let t1 = 1
  const dx = x1 - x0
  const dy = y1 - y0
  const clip = (p: number, q: number): boolean => {
    if (p === 0) return q >= 0
    const t = q / p
    if (p < 0) {
      if (t > t1) return false
      if (t > t0) t0 = t
    } else {
      if (t < t0) return false
      if (t < t1) t1 = t
    }
    return true
  }
  return clip(-dx, x0 - r.minX) && clip(dx, r.maxX - x0) && clip(-dy, y0 - r.minY) && clip(dy, r.maxY - y0) && t0 <= t1
}

/** Bounding box of a layer's extrusions, or undefined when it has none. */
export function layerBounds(moves: Extrusion[], layer: number): Rect | undefined {
  let r: Rect | undefined
  for (const m of moves) {
    if (m.layer !== layer) continue
    r ??= { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity }
    r.minX = Math.min(r.minX, m.x0, m.x1)
    r.minY = Math.min(r.minY, m.y0, m.y1)
    r.maxX = Math.max(r.maxX, m.x0, m.x1)
    r.maxY = Math.max(r.maxY, m.y0, m.y1)
  }
  return r
}

/** Whether a layer extrudes anywhere inside `rect`. */
export function layerHitsRect(moves: Extrusion[], layer: number, rect: Rect): boolean {
  return moves.some((m) => m.layer === layer && segmentHitsRect(m.x0, m.y0, m.x1, m.y1, rect))
}

/** The first extrusion of a layer at or after `fromLine`, or undefined. */
export function firstExtrusionLine(moves: Extrusion[], layer: number, fromLine = 0): number | undefined {
  for (const m of moves) if (m.layer === layer && m.line >= fromLine) return m.line
  return undefined
}
