// parity probe [--only <name>] [--check]: builds the probe programs (parity/oracle/probe/,
// probe.ts) against the oracle's build tree and runs them: every probe of the registry
// (PROBE_REGISTRY), or with --only the one named, which then writes only its own file. Without
// --check it writes the full set to <work>/parity/probes and the committed subset to the
// repository's parity/probes. With --check it runs the same probes again into
// <work>/parity/runs/probe-check and requires byte-identical files in both sets. It needs parity
// oracle. Exit 0 when the files are written (or, with --check, the same), 1 when --check finds a
// difference or a committed file is over its budget (probe.ts, committedSizeProblems).

import { existsSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { checkToolchain, currentPlatform, findToolchain } from '../toolchain.ts'
import type { ParityRun } from './dispatch.ts'
import {
  buildProbe,
  committedSizeProblems,
  PROBE_COMMITTED_MAX_BYTES,
  PROBE_FILE_MAX_BYTES,
  PROBE_REGISTRY,
  probeBuildDir,
  probeCommittedDir,
  probeDifferences,
  probeLibraryDirs,
  probeSelection,
  runProbe,
} from './probe.ts'

export const usage = `probe [--only <name>] [--check]    builds the probe programs (parity/oracle/probe/) against the oracle's build tree and records what the engine's foundations and libraries do: the full set into <work>/parity/probes, the committed subset into parity/probes (crt, fmt, stl, tbb and configdef.json under ${PROBE_COMMITTED_MAX_BYTES / 1024 / 1024} MB together, every other file ${PROBE_FILE_MAX_BYTES / 1024} KB at most); --only builds and runs one probe (${PROBE_REGISTRY.map((e) => e.name).join(', ')}) and writes only its file; --check runs them again and requires the same bytes, exit 1 when they differ`

export const locked = true

/** The command line: --check, and --only <name> (or --only=<name>) once; anything else is a usage error. */
export function parseProbeArgs(args: readonly string[]): { check: boolean; only?: string } {
  let check = false
  let only: string | undefined
  for (let i = 0; i < args.length; ++i) {
    const a = args[i]!
    if (a === '--check') check = true
    else if (a === '--only' || a.startsWith('--only=')) {
      const name = a === '--only' ? args[++i] : a.slice('--only='.length)
      if (name === undefined || name === '' || name.startsWith('--')) throw new UsageError('parity probe --only needs the name of a probe')
      if (only !== undefined) throw new UsageError('parity probe takes --only once')
      only = name
    } else throw new UsageError(`parity probe takes only --only <name> and --check, got ${a}`)
  }
  return only === undefined ? { check } : { check, only }
}

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  const { check, only } = parseProbeArgs(args)
  const entries = probeSelection(only)
  const files = entries.map((e) => e.file)
  const say = (m: string) => log.line(`parity probe: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)
  const t = findToolchain()
  for (const n of checkToolchain(t)) log.line(`toolchain: ${n}`)
  const build = await buildProbe({ t, work: ctx.work, paths, engineDir: ctx.engine.engine, log, entries })
  say(`built ${entries.map((e) => build.exes[e.name]).join(', ')}`)
  const committed = probeCommittedDir(ctx.engine.engine)
  const crtSymbolsFile = join(probeBuildDir(paths), 'crt-symbols.txt')
  const libraryDirs = probeLibraryDirs(ctx.work, paths)
  if (!check) {
    await runProbe({ entries, exes: build.exes, crtSymbolsFile, committed, full: paths.probes, libraryDirs })
    say(`wrote ${files.join(', ')} into ${paths.probes} (full) and ${committed} (committed)`)
    const sizes = committedSizeProblems(committed, entries)
    for (const s of sizes) say(s)
    return sizes.length > 0 ? 1 : 0
  }
  for (const d of [committed, paths.probes]) for (const f of files) if (!existsSync(join(d, f))) throw new StepError(`no ${f} in ${d}; run parity probe${only ? ` --only ${only}` : ''} first`)
  const root = join(paths.runs, 'probe-check')
  mkdirSync(root, { recursive: true })
  const again = { committed: join(root, 'committed'), full: join(root, 'full') }
  await runProbe({ entries, exes: build.exes, crtSymbolsFile, committed: again.committed, full: again.full, libraryDirs })
  // A full check compares the folders whole, so a file the registry no longer writes shows too.
  const compared = only === undefined ? undefined : files
  const differences = [
    ...probeDifferences(committed, again.committed, compared).map((d) => `committed ${d}`),
    ...probeDifferences(paths.probes, again.full, compared).map((d) => `full ${d}`),
    ...committedSizeProblems(committed, entries),
  ]
  if (differences.length > 0) {
    for (const d of differences) say(`differs: ${d}`)
    say(`FAIL: a second run did not write the same probe files (it wrote them into ${root})`)
    return 1
  }
  say(`pass: a second run wrote the same bytes for ${files.length === 1 ? files[0] : `all ${files.length} files`} of both sets`)
  return 0
}
