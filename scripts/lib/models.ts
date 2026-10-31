// Test models for the smoke run, made in code and written as binary STL: a cube, a box with a
// closed internal pocket, a UV sphere and a cone. Every mesh is closed and outward-facing (a
// pocket's walls face into the pocket), which tests check with meshIssues and meshVolume.

import { writeFileSync } from 'node:fs'

export type Vec3 = [number, number, number]
export type Triangle = [Vec3, Vec3, Vec3]

export interface Mesh {
  triangles: Triangle[]
}

function sub(a: Vec3, b: Vec3): Vec3 {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

function dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/** Unit normal by the right-hand rule (zero for a degenerate triangle). */
export function triangleNormal(t: Triangle): Vec3 {
  const n = cross(sub(t[1], t[0]), sub(t[2], t[0]))
  const len = Math.hypot(n[0], n[1], n[2])
  return len === 0 ? [0, 0, 0] : [n[0] / len, n[1] / len, n[2] / len]
}

/** Two triangles of a quad given counter-clockwise as seen from the side its normal points to. */
function quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3): Triangle[] {
  return [
    [a, b, c],
    [a, c, d],
  ]
}

function flip(t: Triangle): Triangle {
  return [t[0], t[2], t[1]]
}

/** The 12 triangles of an axis-aligned box; `inward` turns them to face into the box (a cavity). */
export function boxTriangles(min: Vec3, max: Vec3, inward = false): Triangle[] {
  const p = (i: number, j: number, k: number): Vec3 => [i ? max[0] : min[0], j ? max[1] : min[1], k ? max[2] : min[2]]
  const t = [
    ...quad(p(0, 0, 0), p(0, 1, 0), p(1, 1, 0), p(1, 0, 0)), // -z
    ...quad(p(0, 0, 1), p(1, 0, 1), p(1, 1, 1), p(0, 1, 1)), // +z
    ...quad(p(0, 0, 0), p(1, 0, 0), p(1, 0, 1), p(0, 0, 1)), // -y
    ...quad(p(0, 1, 0), p(0, 1, 1), p(1, 1, 1), p(1, 1, 0)), // +y
    ...quad(p(0, 0, 0), p(0, 0, 1), p(0, 1, 1), p(0, 1, 0)), // -x
    ...quad(p(1, 0, 0), p(1, 1, 0), p(1, 1, 1), p(1, 0, 1)), // +x
  ]
  return inward ? t.map(flip) : t
}

/** A cube with one corner at the origin. */
export function cube(size: number): Mesh {
  return { triangles: boxTriangles([0, 0, 0], [size, size, size]) }
}

export interface Pocket {
  min: Vec3
  max: Vec3
}

/** A box from the origin to `size` with a closed cavity (`pocket`, strictly inside it). */
export function boxWithPocket(size: Vec3, pocket: Pocket): Mesh {
  for (let i = 0; i < 3; i++) {
    if (!(pocket.min[i]! > 0 && pocket.max[i]! < size[i]! && pocket.min[i]! < pocket.max[i]!)) {
      throw new Error(`the pocket must lie strictly inside the box (axis ${'xyz'[i]})`)
    }
  }
  return { triangles: [...boxTriangles([0, 0, 0], size), ...boxTriangles(pocket.min, pocket.max, true)] }
}

/** A UV sphere standing on z = 0 and centred on the z axis; `segments` around, `rings` from pole to pole. */
export function uvSphere(radius: number, segments = 64, rings = 32): Mesh {
  if (segments < 3 || rings < 2) throw new Error('a sphere needs at least 3 segments and 2 rings')
  const c: Vec3 = [0, 0, radius]
  const v = (ring: number, seg: number): Vec3 => {
    const theta = (Math.PI * ring) / rings
    const phi = (2 * Math.PI * (seg % segments)) / segments
    return [c[0] + radius * Math.sin(theta) * Math.cos(phi), c[1] + radius * Math.sin(theta) * Math.sin(phi), c[2] + radius * Math.cos(theta)]
  }
  const north: Vec3 = [c[0], c[1], c[2] + radius]
  const south: Vec3 = [c[0], c[1], c[2] - radius]
  const t: Triangle[] = []
  for (let j = 0; j < segments; j++) {
    t.push([north, v(1, j), v(1, j + 1)])
    for (let i = 1; i < rings - 1; i++) t.push(...quad(v(i, j), v(i + 1, j), v(i + 1, j + 1), v(i, j + 1)))
    t.push([south, v(rings - 1, j + 1), v(rings - 1, j)])
  }
  return { triangles: t }
}

/** A cone with its base disk on z = 0, centred on the z axis, apex at `height`. */
export function cone(radius: number, height: number, segments = 64): Mesh {
  if (segments < 3) throw new Error('a cone needs at least 3 segments')
  const rim = (j: number): Vec3 => {
    const phi = (2 * Math.PI * (j % segments)) / segments
    return [radius * Math.cos(phi), radius * Math.sin(phi), 0]
  }
  const centre: Vec3 = [0, 0, 0]
  const apex: Vec3 = [0, 0, height]
  const t: Triangle[] = []
  for (let j = 0; j < segments; j++) {
    t.push([centre, rim(j + 1), rim(j)])
    t.push([rim(j), rim(j + 1), apex])
  }
  return { triangles: t }
}

/** Binary STL: 80-byte header (not starting with "solid"), triangle count, then 50 bytes per triangle. */
export function encodeBinaryStl(mesh: Mesh, name: string): Buffer {
  const n = mesh.triangles.length
  const buf = Buffer.alloc(84 + 50 * n)
  const header = `sketch-orca smoke model: ${name}`.slice(0, 80).padEnd(80, ' ')
  buf.write(header, 0, 80, 'latin1')
  buf.writeUInt32LE(n, 80)
  let o = 84
  for (const t of mesh.triangles) {
    for (const x of [...triangleNormal(t), ...t[0], ...t[1], ...t[2]]) {
      buf.writeFloatLE(x, o)
      o += 4
    }
    buf.writeUInt16LE(0, o)
    o += 2
  }
  return buf
}

export function writeStl(path: string, mesh: Mesh, name: string): void {
  writeFileSync(path, encodeBinaryStl(mesh, name))
}

/** Reads a binary STL back (for tests). Coordinates come back as 32-bit floats. */
export function decodeBinaryStl(buf: Buffer): Mesh {
  if (buf.length < 84) throw new Error('not a binary STL: too short')
  const n = buf.readUInt32LE(80)
  if (buf.length !== 84 + 50 * n) throw new Error(`not a binary STL: ${n} triangles need ${84 + 50 * n} bytes, got ${buf.length}`)
  const triangles: Triangle[] = []
  for (let i = 0; i < n; i++) {
    const o = 84 + 50 * i + 12
    const v = (k: number): Vec3 => [buf.readFloatLE(o + 12 * k), buf.readFloatLE(o + 12 * k + 4), buf.readFloatLE(o + 12 * k + 8)]
    triangles.push([v(0), v(1), v(2)])
  }
  return { triangles }
}

/** Signed volume (divergence theorem): positive for a closed outward-facing mesh. */
export function meshVolume(mesh: Mesh): number {
  let v = 0
  for (const t of mesh.triangles) v += dot(t[0], cross(t[1], t[2]))
  return v / 6
}

export function meshBounds(mesh: Mesh): { min: Vec3; max: Vec3 } {
  const min: Vec3 = [Infinity, Infinity, Infinity]
  const max: Vec3 = [-Infinity, -Infinity, -Infinity]
  for (const t of mesh.triangles) {
    for (const p of t) {
      for (let i = 0; i < 3; i++) {
        min[i] = Math.min(min[i]!, p[i]!)
        max[i] = Math.max(max[i]!, p[i]!)
      }
    }
  }
  return { min, max }
}

/**
 * Edges that break a closed, consistently oriented surface: each directed edge must appear once
 * and its reverse once. Returns the count of bad edges (0 for a good mesh) and of degenerate triangles.
 */
export function meshIssues(mesh: Mesh): { badEdges: number; degenerate: number } {
  const key = (p: Vec3) => `${p[0]},${p[1]},${p[2]}`
  const directed = new Map<string, number>()
  let degenerate = 0
  for (const t of mesh.triangles) {
    const k = t.map(key)
    if (k[0] === k[1] || k[1] === k[2] || k[0] === k[2]) degenerate++
    for (let i = 0; i < 3; i++) {
      const e = `${k[i]}|${k[(i + 1) % 3]}`
      directed.set(e, (directed.get(e) ?? 0) + 1)
    }
  }
  let badEdges = 0
  for (const [e, count] of directed) {
    const [a, b] = e.split('|')
    if (count !== 1 || directed.get(`${b}|${a}`) !== 1) badEdges++
  }
  return { badEdges, degenerate }
}
