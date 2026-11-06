// parity oracle: builds the oracle (oracle.ts) and stages it in <work>/oracle/dist. It refuses to
// start with less than ORACLE_MIN_FREE_BYTES free on the work folder's drive or with a work folder
// longer than WINDOWS_WORK_PATH_MAX, and builds only on the engine's finished dependency build
// (run build first), which it never rebuilds. Exit 0 when the oracle is staged.

import { listPatches } from '../checkout.ts'
import { formatCommand } from '../cmake.ts'
import { StepError, UsageError } from '../run.ts'
import { parityPaths } from '../paths.ts'
import { checkToolchain, currentPlatform, findToolchain } from '../toolchain.ts'
import type { ParityRun } from './dispatch.ts'
import { buildOracle, freeBytes, ORACLE_EXE, oraclePatches, oracleRefusals, prepareOracleSource, stagedResourceCounts, stageOracle } from './oracle.ts'

export const usage = 'oracle             builds the oracle, the engine with the parity knobs of parity/oracle/patches (parity/oracle/README.md), on the engine\'s dependency build, and stages it in <work>/oracle/dist; it never ships'

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length > 0) throw new UsageError(`parity oracle takes no options, got ${args.join(' ')}`)
  const say = (m: string) => log.line(`parity oracle: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const free = freeBytes(ctx.work.home)
  const refusals = oracleRefusals({ home: ctx.work.home, platform, free })
  if (refusals.length > 0) throw new StepError(refusals.join('; '))
  say(`${(free / 1024 ** 3).toFixed(1)} GB free on the drive of ${ctx.work.home}`)
  const patches = listPatches(ctx.engine.patches)
  const oracle = oraclePatches(ctx.engine.engine)
  say(`${patches.length} engine patches, ${oracle.length} oracle patches: ${oracle.map((p) => p.name).join(', ')}`)
  const t = findToolchain()
  for (const n of checkToolchain(t)) log.line(`toolchain: ${n}`)
  log.line(`toolchain: cmake ${t.cmake}, ninja ${t.ninja}, git ${t.git}`)
  const source = await prepareOracleSource({ git: t.git, lock: ctx.lock, work: ctx.work, paths, patches, oracle, cli: ctx.engine.cli, log })
  const build = await buildOracle({ t, lock: ctx.lock, work: ctx.work, paths, engineDir: ctx.engine.engine, patches, source, log })
  say(`built ${build.exe}; linker map ${build.map}`)
  log.detail(`parity oracle: configured with ${formatCommand(t.cmake, build.configure)}`)
  const manifest = stageOracle({ paths, work: ctx.work, lock: ctx.lock, platform, source, build })
  const counts = stagedResourceCounts(paths.oracleDist)
  say(`staged ${paths.oracleDist}: bin/${ORACLE_EXE} with its map and ${manifest.runtime.length} libraries, resources ${Object.entries(counts).map(([k, v]) => `${k} (${v} files)`).join(', ')}`)
  return 0
}
