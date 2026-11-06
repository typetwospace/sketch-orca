// `bun scripts/orca.ts parity <name> [options]`: the parity harness's commands, one module each,
// scripts/lib/parity/cmd-<name>.ts, found by name at run time. A new command is a new module: no
// list in orca.ts or here names them, and the usage text lists the modules there are.
//
// Each module exports
//   usage   one line: "<name> [options]   what it does"
//   locked  true when it writes in the work folder: it then holds <work>/run.lock.d while it runs,
//           as orca.ts's LOCKED_COMMANDS do, so no other pipeline or parity run writes there at once
//   run     (ParityRun) => Promise<number>, the exit code (0 when it did its work)
// Names are lower case: a letter, then letters, digits and '-' (PARITY_NAME_RE), so a name can never
// reach outside this folder. Each command logs to <work>/logs/parity-<name>.log.

import { readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import type { Context } from '../../orca.ts'
import { Log, UsageError } from '../run.ts'

export const PARITY_NAME_RE = /^[a-z][a-z0-9-]*$/

/** This folder: where the cmd-<name>.ts modules are. */
export const PARITY_DIR = dirname(fileURLToPath(import.meta.url))

export interface ParityRun {
  ctx: Context
  /** The options after the command's name. */
  args: string[]
  /** <work>/logs/parity-<name>.log */
  log: Log
}

export interface ParityCommand {
  usage: string
  locked: boolean
  run(run: ParityRun): Promise<number>
}

/** The commands in `dir`: the names of its cmd-<name>.ts modules, sorted. */
export function listParityCommands(dir: string = PARITY_DIR): string[] {
  return readdirSync(dir)
    .map((f) => /^cmd-(.+)\.ts$/.exec(f)?.[1])
    .filter((n): n is string => n !== undefined && PARITY_NAME_RE.test(n))
    .sort()
}

/** Checks what a cmd-<name>.ts module exports; throws naming what is missing. */
export function checkParityModule(mod: unknown, name: string): ParityCommand {
  const m = (mod ?? {}) as Record<string, unknown>
  const problems: string[] = []
  if (typeof m.usage !== 'string' || m.usage.trim() === '') problems.push('usage (text)')
  if (typeof m.locked !== 'boolean') problems.push('locked (true or false)')
  if (typeof m.run !== 'function') problems.push('run (a function)')
  if (problems.length > 0) throw new Error(`parity command ${name}: cmd-${name}.ts does not export ${problems.join(', ')}`)
  return { usage: m.usage as string, locked: m.locked as boolean, run: m.run as ParityCommand['run'] }
}

/** Loads the command `name` from `dir`; a name that is not one of its commands is a usage error. */
export async function loadParityCommand(name: string, dir: string = PARITY_DIR): Promise<ParityCommand> {
  if (!PARITY_NAME_RE.test(name)) throw new UsageError(`parity: ${JSON.stringify(name)} is not a command name (a lower-case letter, then letters, digits and -)\n${parityUsage(dir)}`)
  if (!listParityCommands(dir).includes(name)) throw new UsageError(`parity: no command ${name}\n${parityUsage(dir)}`)
  return checkParityModule(await import(pathToFileURL(join(dir, `cmd-${name}.ts`)).href), name)
}

/** The usage text: the commands found in `dir`, each with its usage line. */
export function parityUsage(dir: string = PARITY_DIR, usages?: Map<string, string>): string {
  const names = listParityCommands(dir)
  const lines = [`usage: bun ${join('scripts', 'orca.ts')} parity <${names.join('|') || 'command'}> [options]`]
  for (const n of names) lines.push(`  ${usages?.get(n) ?? n}`)
  return lines.join('\n')
}

/** The usage text with each command's own usage line (loads every module). */
export async function fullParityUsage(dir: string = PARITY_DIR): Promise<string> {
  const usages = new Map<string, string>()
  for (const n of listParityCommands(dir)) {
    try {
      usages.set(n, checkParityModule(await import(pathToFileURL(join(dir, `cmd-${n}.ts`)).href), n).usage)
    } catch (err) {
      usages.set(n, `${n}   (cannot be loaded: ${(err as Error).message})`)
    }
  }
  return parityUsage(dir, usages)
}

export interface DispatchOptions {
  dir?: string
  /** Takes <work>/run.lock.d for `label` and returns how to give it back (runlock.ts takeRunLock). */
  takeLock: (home: string, label: string) => () => void
  /** Where usage text goes (console.log by default). */
  print?: (text: string) => void
  /** The log of a command (default <work>/logs/parity-<name>.log). */
  logFor?: (ctx: Context, name: string) => Log
}

/**
 * Runs `parity <name> [options]`: `argv` is what follows "parity". No name, or --help, prints the
 * usage (exit 64, or 0 for --help); a locked command holds the run lock while it runs, and gives
 * it back however it ends.
 */
export async function runParity(ctx: Context, argv: string[], o: DispatchOptions): Promise<number> {
  const dir = o.dir ?? PARITY_DIR
  const [name, ...args] = argv
  if (name === undefined || name === '--help' || name === '-h') {
    const text = await fullParityUsage(dir)
    if (name === undefined) throw new UsageError(`parity: no command given\n${text}`)
    ;(o.print ?? console.log)(text)
    return 0
  }
  const cmd = await loadParityCommand(name, dir)
  const log = o.logFor ? o.logFor(ctx, name) : new Log(join(ctx.work.logs, `parity-${name}.log`))
  const release = cmd.locked ? o.takeLock(ctx.work.home, `parity ${name}`) : undefined
  try {
    return await cmd.run({ ctx, args, log })
  } finally {
    release?.()
  }
}
