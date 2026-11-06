#!/usr/bin/env bun
// The cargo runner of the Rust translation (crates/): cargo with --locked, inside the pinned
// Visual Studio environment, with the pinned Rust. See ../README.md and ../BUILDING.md.
//
//   bun scripts/rust.ts <command> [cargo arguments] [-- arguments for the tool]
//
//   build            cargo build --locked
//   test             cargo test --locked
//   clippy           cargo clippy --locked --all-targets  (the lint flags, -D warnings, go after a --)
//   fmt              cargo fmt --all                      (--check only checks)
//
// The package scripts run them: bun run rs:build, rs:test, rs:clippy, rs:fmt:check. Exit codes:
// cargo's own when cargo ran; 3 when the runner refuses (another Visual Studio, MSVC, SDK or
// rustc than the pinned ones, or the cargo run was stopped after its time limit); 64 for a command
// line it does not take (run.ts, EXIT). The cargo run ends at the latest after 45 minutes
// (SKETCH_ORCA_RUST_TIMEOUT_MS sets another limit); it never opens a window.

import { spawn, spawnSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { findRustToolchain, parseRustArgs } from './lib/rust.ts'
import { EXIT, StepError, UsageError } from './lib/run.ts'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const DEFAULT_LIMIT_MS = 45 * 60 * 1000

/** Ends a process and the processes it started. */
function killTree(pid: number | undefined): void {
  if (pid === undefined) return
  spawnSync('taskkill.exe', ['/PID', String(pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' })
}

function limitMs(env: NodeJS.ProcessEnv): number {
  const v = env.SKETCH_ORCA_RUST_TIMEOUT_MS
  return v !== undefined && /^[1-9]\d*$/.test(v) ? Number(v) : DEFAULT_LIMIT_MS
}

async function main(): Promise<number> {
  const inv = parseRustArgs(process.argv.slice(2))
  const tool = findRustToolchain(ROOT)
  console.log(`${tool.notes.join('; ')}`)
  console.log(`$ cargo ${inv.cargoArgs.join(' ')}`)
  return await new Promise<number>((resolveCode, reject) => {
    const child = spawn(tool.cargo, inv.cargoArgs, { cwd: ROOT, env: tool.env, stdio: 'inherit', windowsHide: true })
    let timedOut = false
    const timer = setTimeout(() => {
      timedOut = true
      killTree(child.pid)
    }, limitMs(process.env))
    const stop = () => killTree(child.pid)
    process.once('SIGINT', stop)
    process.once('SIGTERM', stop)
    child.on('error', (e) => {
      clearTimeout(timer)
      reject(e)
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      process.off('SIGINT', stop)
      process.off('SIGTERM', stop)
      if (timedOut) reject(new StepError(`cargo ${inv.command} was stopped after its time limit`))
      else resolveCode(code ?? EXIT.failed)
    })
  })
}

main().then(
  (code) => process.exit(code),
  (e) => {
    if (e instanceof UsageError) {
      console.error(`rust.ts: ${e.message}`)
      process.exit(EXIT.usage)
    }
    console.error(`rust.ts: ${e instanceof Error ? e.message : String(e)}`)
    process.exit(EXIT.failed)
  },
)
