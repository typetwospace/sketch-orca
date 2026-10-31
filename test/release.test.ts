import { describe, expect, test } from 'bun:test'
import { runSteps, stepArgs, type ReleaseStep } from '../scripts/lib/release.ts'

function steps(codes: Record<string, number | Error>, ran: string[]): ReleaseStep[] {
  return Object.entries(codes).map(([name, code]) => ({
    name,
    run: async () => {
      ran.push(name)
      if (code instanceof Error) throw code
      return code
    },
  }))
}

describe('release', () => {
  test('runs every step in order when each succeeds', async () => {
    const ran: string[] = []
    const lines: string[] = []
    const out = await runSteps(steps({ check: 0, update: 0, fetch: 0, build: 0, smoke: 0, stage: 0, source: 0 }, ran), (l) => lines.push(l), () => 0)
    expect(out).toEqual({ code: 0, done: ['check', 'update', 'fetch', 'build', 'smoke', 'stage', 'source'] })
    expect(ran).toEqual(out.done)
    expect(lines.at(-1)).toBe('release: all 7 steps done (check, update, fetch, build, smoke, stage, source)')
  })

  test('stops at the first step with a non-zero exit code and runs nothing after it', async () => {
    const ran: string[] = []
    const lines: string[] = []
    const out = await runSteps(steps({ fetch: 0, build: 0, smoke: 1, stage: 0, source: 0 }, ran), (l) => lines.push(l))
    expect(out).toEqual({ code: 1, done: ['fetch', 'build'], failed: 'smoke' })
    expect(ran).toEqual(['fetch', 'build', 'smoke'])
    expect(lines).toContain('release: stopped at smoke (exit 1)')
  })

  test('a step that throws stops the release with its message and the failure code 3; a check that finds a moved tag keeps its code', async () => {
    const ran: string[] = []
    const out = await runSteps(steps({ fetch: 0, build: new Error('cl.exe is not on the build PATH'), smoke: 0 }, ran), () => {})
    expect(out).toEqual({ code: 3, done: ['fetch'], failed: 'build', error: 'cl.exe is not on the build PATH' })
    expect(ran).toEqual(['fetch', 'build'])
    const moved = await runSteps(steps({ check: 2, update: 0 }, []), () => {})
    expect(moved).toEqual({ code: 2, done: [], failed: 'check' })
  })

  test('stepArgs passes on only the options a step takes', () => {
    const args = ['--offline', '--no-deps', '--allow-missing', '--without-smoke', '--case', 'version']
    expect(stepArgs('source', args)).toEqual(['--no-deps', '--allow-missing'])
    expect(stepArgs('stage', args)).toEqual([])
    expect(stepArgs('smoke', args)).toEqual([])
    expect(stepArgs('build', args)).toEqual([])
  })
})
