// Logging and child processes for the pipeline. Every step appends to its own log file; child
// output goes only to the log (the console gets one line per command). Children never get a
// window (windowsHide).

import { spawn, spawnSync } from 'node:child_process'
import { appendFileSync, createWriteStream, mkdirSync } from 'node:fs'
import { dirname } from 'node:path'
import { formatCommand } from './cmake.ts'

export class StepError extends Error {}

/** A command line the pipeline does not take: an unknown command or a bad option. */
export class UsageError extends StepError {}

/**
 * The pipeline's exit codes. check: 0 nothing to do, 1 a newer stable release is out, 2 the
 * locked tag moved; update and release also stop with 2 when the tag moved. Every command: 3 when
 * it fails (an error, failed smoke cases, another run holding the work folder), 64 for a command
 * line it does not take. No failure exits with 1 or 2, so scripts can branch on check's codes.
 */
export const EXIT = { ok: 0, newer: 1, tagMoved: 2, failed: 3, usage: 64 } as const

export class Log {
  readonly path: string
  readonly quiet: boolean
  constructor(path: string, quiet = false) {
    this.path = path
    this.quiet = quiet
    mkdirSync(dirname(path), { recursive: true })
  }
  line(msg: string): void {
    appendFileSync(this.path, `[${new Date().toISOString()}] ${msg}\n`)
    if (!this.quiet) console.log(msg)
  }
  /** Only to the file. */
  detail(msg: string): void {
    appendFileSync(this.path, `[${new Date().toISOString()}] ${msg}\n`)
  }
}

export interface RunOptions {
  cwd?: string
  env?: Record<string, string>
  log: Log
  /** Exit codes that count as success (default [0]). */
  ok?: number[]
}

/** Runs a command to completion with stdout and stderr appended to the log; throws StepError on failure. */
export async function runLogged(exe: string, args: string[], opts: RunOptions): Promise<void> {
  opts.log.line(`$ ${formatCommand(exe, args)}`)
  const stream = createWriteStream(opts.log.path, { flags: 'a' })
  const code = await new Promise<number>((resolve, reject) => {
    const child = spawn(exe, args, {
      cwd: opts.cwd,
      env: opts.env ?? (process.env as Record<string, string>),
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    })
    child.stdout.pipe(stream, { end: false })
    child.stderr.pipe(stream, { end: false })
    child.on('error', reject)
    child.on('close', (c) => resolve(c ?? -1))
  })
  await new Promise<void>((resolve) => stream.end(resolve))
  if (!(opts.ok ?? [0]).includes(code)) {
    throw new StepError(`${formatCommand(exe, args)} exited with ${code} (log: ${opts.log.path})`)
  }
}

export interface Captured {
  code: number
  stdout: string
  stderr: string
}

export interface CaptureOptions {
  cwd?: string
  env?: Record<string, string>
  input?: string
  /** Pass arguments to the program as written (cmd.exe /s /c on Windows). */
  verbatim?: boolean
  /** How the program writes its output: UTF-8 by default; utf16le for cmd.exe /u. */
  encoding?: 'utf8' | 'utf16le'
}

/** Runs a short command and captures its output (for git queries, vcvars, version checks). */
export function capture(exe: string, args: string[], opts: CaptureOptions = {}): Captured {
  const r = spawnSync(exe, args, {
    cwd: opts.cwd,
    env: opts.env ?? (process.env as Record<string, string>),
    input: opts.input,
    encoding: opts.encoding ?? 'utf8',
    windowsHide: true,
    windowsVerbatimArguments: opts.verbatim ?? false,
    maxBuffer: 256 * 1024 * 1024,
  })
  if (r.error) throw r.error
  const text = (s: string | null) => (s ?? '').replace(/^\uFEFF/, '')
  return { code: r.status ?? -1, stdout: text(r.stdout), stderr: text(r.stderr) }
}

export function captureOk(exe: string, args: string[], opts: CaptureOptions = {}): string {
  const r = capture(exe, args, opts)
  if (r.code !== 0) throw new StepError(`${formatCommand(exe, args)} exited with ${r.code}: ${r.stderr.trim() || r.stdout.trim()}`)
  return r.stdout
}
