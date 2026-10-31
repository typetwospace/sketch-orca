import { describe, expect, test } from 'bun:test'
import { boxWithPocket, cone, cube, decodeBinaryStl, encodeBinaryStl, meshBounds, meshIssues, meshVolume, triangleNormal, uvSphere, type Mesh } from '../scripts/lib/models.ts'

function expectClosed(mesh: Mesh): void {
  expect(meshIssues(mesh)).toEqual({ badEdges: 0, degenerate: 0 })
}

describe('models', () => {
  test('a cube is closed, outward-facing and exactly 20 mm on a side', () => {
    const m = cube(20)
    expect(m.triangles).toHaveLength(12)
    expectClosed(m)
    expect(meshVolume(m)).toBeCloseTo(8000, 9)
    expect(meshBounds(m)).toEqual({ min: [0, 0, 0], max: [20, 20, 20] })
    // Every normal points away from the centre.
    for (const t of m.triangles) {
      const n = triangleNormal(t)
      const c = [0, 1, 2].map((i) => (t[0][i]! + t[1][i]! + t[2][i]!) / 3 - 10)
      expect(n[0] * c[0]! + n[1] * c[1]! + n[2] * c[2]!).toBeGreaterThan(0)
    }
  })

  test('a box with a closed pocket: the pocket faces inward and takes its volume away', () => {
    const m = boxWithPocket([20, 20, 10], { min: [5, 5, 3], max: [15, 15, 6] })
    expect(m.triangles).toHaveLength(24)
    expectClosed(m)
    expect(meshVolume(m)).toBeCloseTo(20 * 20 * 10 - 10 * 10 * 3, 9)
    expect(meshBounds(m)).toEqual({ min: [0, 0, 0], max: [20, 20, 10] })
  })

  test('a pocket must lie strictly inside the box', () => {
    expect(() => boxWithPocket([20, 20, 10], { min: [0, 5, 3], max: [15, 15, 6] })).toThrow(/strictly inside the box \(axis x\)/)
    expect(() => boxWithPocket([20, 20, 10], { min: [5, 5, 3], max: [15, 15, 10] })).toThrow(/axis z/)
    expect(() => boxWithPocket([20, 20, 10], { min: [5, 9, 3], max: [15, 8, 6] })).toThrow(/axis y/)
  })

  test('a UV sphere is closed, stands on z = 0 and is just inside the true sphere', () => {
    const r = 10
    const m = uvSphere(r, 64, 32)
    expect(m.triangles).toHaveLength(2 * 64 + 2 * 64 * 30)
    expectClosed(m)
    const b = meshBounds(m)
    expect(b.min[2]).toBeCloseTo(0, 12)
    expect(b.max[2]).toBeCloseTo(2 * r, 12)
    for (let i = 0; i < 2; i++) {
      expect(b.min[i]!).toBeGreaterThanOrEqual(-r - 1e-12)
      expect(b.max[i]!).toBeLessThanOrEqual(r + 1e-12)
    }
    const exact = (4 / 3) * Math.PI * r ** 3
    const v = meshVolume(m)
    expect(v).toBeLessThan(exact)
    expect(v).toBeGreaterThan(exact * 0.98)
  })

  test('a cone is a closed pyramid over a regular polygon', () => {
    const n = 64
    const m = cone(10, 20, n)
    expect(m.triangles).toHaveLength(2 * n)
    expectClosed(m)
    const base = (n / 2) * 10 * 10 * Math.sin((2 * Math.PI) / n)
    expect(meshVolume(m)).toBeCloseTo((base * 20) / 3, 9)
    expect(meshBounds(m).max[2]).toBe(20)
  })

  test('too few segments are refused', () => {
    expect(() => uvSphere(10, 2, 8)).toThrow()
    expect(() => uvSphere(10, 8, 1)).toThrow()
    expect(() => cone(10, 10, 2)).toThrow()
  })

  test('binary STL: header, size, normals and a round trip of 32-bit coordinates', () => {
    const m = boxWithPocket([20, 20, 10], { min: [5, 5, 3], max: [15, 15, 6] })
    const buf = encodeBinaryStl(m, 'box with pocket')
    expect(buf.length).toBe(84 + 50 * m.triangles.length)
    expect(buf.toString('latin1', 0, 5)).not.toBe('solid')
    expect(buf.toString('latin1', 0, 80).trimEnd()).toBe('sketch-orca smoke model: box with pocket')
    expect(buf.readUInt32LE(80)).toBe(24)
    const n0 = triangleNormal(m.triangles[0]!)
    expect([buf.readFloatLE(84), buf.readFloatLE(88), buf.readFloatLE(92)]).toEqual(n0.map((x) => Math.fround(x)))
    const back = decodeBinaryStl(buf)
    expect(back.triangles).toHaveLength(m.triangles.length)
    back.triangles.forEach((t, i) => {
      for (let k = 0; k < 3; k++) expect(t[k]).toEqual(m.triangles[i]![k]!.map((x) => Math.fround(x)) as [number, number, number])
    })
    expectClosed(back)

    const sphere = uvSphere(10, 32, 16)
    const sb = decodeBinaryStl(encodeBinaryStl(sphere, 'sphere'))
    sb.triangles.forEach((t, i) => {
      for (let k = 0; k < 3; k++) for (let a = 0; a < 3; a++) expect(Math.abs(t[k]![a]! - sphere.triangles[i]![k]![a]!)).toBeLessThan(1e-5)
    })
  })

  test('a long name is cut to the 80-byte header', () => {
    const buf = encodeBinaryStl(cube(1), 'x'.repeat(200))
    expect(buf.readUInt32LE(80)).toBe(12)
    expect(buf.toString('latin1', 0, 80)).toBe(`sketch-orca smoke model: ${'x'.repeat(200)}`.slice(0, 80))
  })

  test('decodeBinaryStl refuses a file whose size does not match its triangle count', () => {
    const buf = encodeBinaryStl(cube(1), 'cube')
    expect(() => decodeBinaryStl(buf.subarray(0, buf.length - 1))).toThrow(/12 triangles need/)
    expect(() => decodeBinaryStl(Buffer.alloc(10))).toThrow(/too short/)
  })

  test('meshIssues finds an open edge and a degenerate triangle', () => {
    const m = cube(10)
    const open: Mesh = { triangles: m.triangles.slice(1) }
    expect(meshIssues(open).badEdges).toBeGreaterThan(0)
    const flipped: Mesh = { triangles: [...m.triangles.slice(1), [m.triangles[0]![0], m.triangles[0]![2], m.triangles[0]![1]]] }
    expect(meshIssues(flipped).badEdges).toBeGreaterThan(0)
    const degenerate: Mesh = { triangles: [[[0, 0, 0], [0, 0, 0], [1, 0, 0]]] }
    expect(meshIssues(degenerate).degenerate).toBe(1)
  })
})
