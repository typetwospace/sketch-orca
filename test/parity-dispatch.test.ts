// parity <name>: the dispatcher finds scripts/lib/parity/cmd-<name>.ts modules by name, takes the
// run lock for the locked ones and lists the commands it finds. Hermetic: fake modules in a
// temporary folder, a fake lock and a fake work folder.

import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { Context } from '../scripts/orca.ts'
import { checkParityModule, fullParityUsage, listParityCommands, loadParityCommand, PARITY_DIR, PARITY_NAME_RE, parityUsage, runParity } from '../scripts/lib/parity/dispatch.ts'
import { workPaths } from '../scripts/lib/paths.ts'
import { UsageError } from '../scripts/lib/run.ts'

let dir: string
let work: string

beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), 'orca-parity-cmds-'))
  work = mkdtempSync(join(tmpdir(), 'orca-parity-work-'))
  // A locked command that records what it got, one that fails, an unlocked one, one with bad
  // exports, and files that are not commands.
  writeFileSync(
    join(dir, 'cmd-alpha.ts'),
    `export const usage = 'alpha [--x]   records its call'\nexport const locked = true\nexport async function run(r) { globalThis.__alpha = { args: r.args, home: r.ctx.work.home, log: r.log.path }; return 0 }\n`,
  )
  writeFileSync(join(dir, 'cmd-boom.ts'), `export const usage = 'boom   fails'\nexport const locked = true\nexport async function run() { throw new Error('boom failed') }\n`)
  writeFileSync(join(dir, 'cmd-plain-2.ts'), `export const usage = 'plain-2   writes nothing'\nexport const locked = false\nexport async function run() { return 7 }\n`)
  writeFileSync(join(dir, 'cmd-broken.ts'), `export const usage = ''\nexport const locked = 'yes'\n`)
  writeFileSync(join(dir, 'cmd-Upper.ts'), `export const usage = 'x'\nexport const locked = false\nexport async function run() { return 0 }\n`)
  writeFileSync(join(dir, 'helper.ts'), `export const x = 1\n`)
  writeFileSync(join(dir, 'cmd-notes.md'), 'not a module\n')
})

afterAll(() => {
  rmSync(dir, { recursive: true, force: true })
  rmSync(work, { recursive: true, force: true })
})

function ctx(): Context {
  return { lock: {} as Context['lock'], work: workPaths(work), engine: {} as Context['engine'], args: [] }
}

function fakeLock(): { calls: string[]; takeLock: (home: string, label: string) => () => void } {
  const calls: string[] = []
  return {
    calls,
    takeLock: (home, label) => {
      calls.push(`take ${label} in ${home === work ? '<work>' : home}`)
      return () => calls.push(`release ${label}`)
    },
  }
}

describe('parity dispatch', () => {
  test('command names: a lower-case letter, then letters, digits and -', () => {
    for (const ok of ['same', 'reference', 'a', 'probe-gate', 'o1-5']) expect(PARITY_NAME_RE.test(ok)).toBe(true)
    for (const bad of ['', 'Same', '1st', '-x', 'a_b', '../same', 'a/b', 'a.b', 'same ']) expect(PARITY_NAME_RE.test(bad)).toBe(false)
  })

  test('lists the cmd-<name>.ts modules of the folder, sorted, and nothing else', () => {
    expect(listParityCommands(dir)).toEqual(['alpha', 'boom', 'broken', 'plain-2'])
  })

  test('the usage text lists the commands found, each with its own usage line', async () => {
    expect(parityUsage(dir).split('\n')[0]).toContain('parity <alpha|boom|broken|plain-2> [options]')
    const full = await fullParityUsage(dir)
    expect(full).toContain('  alpha [--x]   records its call')
    expect(full).toContain('  plain-2   writes nothing')
    expect(full).toContain('broken   (cannot be loaded: parity command broken: cmd-broken.ts does not export usage (text), locked (true or false), run (a function))')
  })

  test('a module must export usage, locked and run', () => {
    expect(checkParityModule({ usage: 'u', locked: false, run: async () => 0 }, 'x').locked).toBe(false)
    expect(() => checkParityModule({ usage: 'u', locked: true }, 'x')).toThrow(/does not export run \(a function\)/)
    expect(() => checkParityModule(undefined, 'x')).toThrow(/usage \(text\), locked \(true or false\), run \(a function\)/)
  })

  test('a name that is not a command is a usage error naming the commands there are', async () => {
    await expect(loadParityCommand('nope', dir)).rejects.toThrow(/no command nope\nusage: .*parity <alpha\|boom\|broken\|plain-2>/)
    await expect(loadParityCommand('../alpha', dir)).rejects.toBeInstanceOf(UsageError)
    await expect(loadParityCommand('Upper', dir)).rejects.toThrow(/not a command name/)
    await expect(loadParityCommand('helper', dir)).rejects.toThrow(/no command helper/)
  })

  test('a locked command runs holding the run lock, with its options and its own log', async () => {
    const lock = fakeLock()
    expect(await runParity(ctx(), ['alpha', '--x', '1'], { dir, takeLock: lock.takeLock })).toBe(0)
    expect(lock.calls).toEqual(['take parity alpha in <work>', 'release parity alpha'])
    const seen = (globalThis as unknown as { __alpha: { args: string[]; home: string; log: string } }).__alpha
    expect(seen).toEqual({ args: ['--x', '1'], home: work, log: join(work, 'logs', 'parity-alpha.log') })
  })

  test('the lock is given back when a locked command fails', async () => {
    const lock = fakeLock()
    await expect(runParity(ctx(), ['boom'], { dir, takeLock: lock.takeLock })).rejects.toThrow('boom failed')
    expect(lock.calls).toEqual(['take parity boom in <work>', 'release parity boom'])
  })

  test('an unlocked command runs without the lock and its exit code passes through', async () => {
    const lock = fakeLock()
    expect(await runParity(ctx(), ['plain-2'], { dir, takeLock: lock.takeLock })).toBe(7)
    expect(lock.calls).toEqual([])
  })

  test('no command is a usage error; --help prints the usage and exits 0; a broken module takes no lock', async () => {
    const lock = fakeLock()
    await expect(runParity(ctx(), [], { dir, takeLock: lock.takeLock })).rejects.toThrow(/no command given\nusage:/)
    const printed: string[] = []
    expect(await runParity(ctx(), ['--help'], { dir, takeLock: lock.takeLock, print: (t) => printed.push(t) })).toBe(0)
    expect(printed[0]).toContain('alpha [--x]   records its call')
    await expect(runParity(ctx(), ['broken'], { dir, takeLock: lock.takeLock })).rejects.toThrow(/does not export/)
    expect(lock.calls).toEqual([])
  })

  test("the repository's commands: reference and same, both locked, each with a usage line", async () => {
    expect(listParityCommands(PARITY_DIR)).toEqual(expect.arrayContaining(['reference', 'same']))
    for (const name of ['reference', 'same']) {
      const cmd = await loadParityCommand(name, PARITY_DIR)
      expect({ name, locked: cmd.locked, starts: cmd.usage.startsWith(`${name} `) }).toEqual({ name, locked: true, starts: true })
    }
  })

  test('orca.ts dispatches parity and its usage names it', () => {
    const orca = readFileSync(join(import.meta.dir, '..', 'scripts', 'orca.ts'), 'utf8')
    expect(orca).toContain("parity: (ctx) => runParity(ctx, ctx.args, { takeLock: takeRunLock })")
    // Nothing was written outside the fake work folder's logs.
    expect(existsSync(join(work, 'run.lock.d'))).toBe(false)
  })
})
