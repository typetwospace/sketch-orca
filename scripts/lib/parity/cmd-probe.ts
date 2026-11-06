// parity probe [--check]: builds the probe program (parity/oracle/probe/, probe.ts) against the
// oracle's build tree and runs it. Without --check it writes the full set to <work>/parity/probes
// and the committed subset to the repository's parity/probes. With --check it runs the probe again
// into <work>/parity/runs/probe-check and requires byte-identical files in both sets. It needs
// parity oracle. Exit 0 when the files are written (or, with --check, the same), 1 when --check
// finds a difference or the committed set is over its size limit.

import { existsSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { checkToolchain, currentPlatform, findToolchain } from '../toolchain.ts'
import type { ParityRun } from './dispatch.ts'
import { buildProbe, committedSizeProblem, PROBE_COMMITTED_MAX_BYTES, PROBE_FILES, probeBuildDir, probeCommittedDir, probeDifferences, probeLibraryDirs, runProbe } from './probe.ts'

export const usage = `probe [--check]    builds the probe program (parity/oracle/probe/) against the oracle's build tree and records what the engine's foundations do: the full set into <work>/parity/probes, the committed subset (under ${PROBE_COMMITTED_MAX_BYTES / 1024 / 1024} MB) into parity/probes; --check runs it again and requires the same bytes, exit 1 when they differ`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  const check = args.includes('--check')
  const rest = args.filter((a) => a !== '--check')
  if (rest.length > 0) throw new UsageError(`parity probe takes only --check, got ${rest.join(' ')}`)
  const say = (m: string) => log.line(`parity probe: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)
  const t = findToolchain()
  for (const n of checkToolchain(t)) log.line(`toolchain: ${n}`)
  const build = await buildProbe({ t, work: ctx.work, paths, engineDir: ctx.engine.engine, log })
  say(`built ${build.exe}`)
  const committed = probeCommittedDir(ctx.engine.engine)
  const crtSymbolsFile = join(probeBuildDir(paths), 'crt-symbols.txt')
  const libraryDirs = probeLibraryDirs(ctx.work, paths)
  if (!check) {
    await runProbe({ exe: build.exe, crtSymbolsFile, committed, full: paths.probes, libraryDirs })
    say(`wrote ${PROBE_FILES.join(', ')} into ${paths.probes} (full) and ${committed} (committed)`)
    const size = committedSizeProblem(committed)
    if (size) {
      say(size)
      return 1
    }
    return 0
  }
  for (const d of [committed, paths.probes]) if (!existsSync(d)) throw new StepError(`no probe files in ${d}; run parity probe first`)
  const root = join(paths.runs, 'probe-check')
  mkdirSync(root, { recursive: true })
  const again = { committed: join(root, 'committed'), full: join(root, 'full') }
  await runProbe({ exe: build.exe, crtSymbolsFile, committed: again.committed, full: again.full, libraryDirs })
  const differences = [...probeDifferences(committed, again.committed).map((d) => `committed ${d}`), ...probeDifferences(paths.probes, again.full).map((d) => `full ${d}`)]
  const size = committedSizeProblem(committed)
  if (size) differences.push(size)
  if (differences.length > 0) {
    for (const d of differences) say(`differs: ${d}`)
    say(`FAIL: a second run did not write the same probe files (it wrote them into ${root})`)
    return 1
  }
  say(`pass: a second run wrote the same bytes for all ${PROBE_FILES.length} files of both sets`)
  return 0
}
