// parity same: the published reference and the engine staged from this checkout, each on every
// smoke case's jobs, every output compared (same.ts); writes <work>/parity/same.json. It needs
// `parity reference` first, and a dist/engine that this checkout's build, smoke and stage made.
// Exit 0 when same.json is written, whatever it found: a difference is a finding, not a failure.

import { existsSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { SMOKE_CASES, smokePresetSets, smokeTimeoutMs } from '../smoke.ts'
import { currentPlatform } from '../toolchain.ts'
import type { ParityRun } from './dispatch.ts'
import { parityEngineEnv } from './engineEnv.ts'
import { readMasks } from './masks.ts'
import { DEFAULT_REFERENCE, keptReference, REFERENCES } from './reference.ts'
import { compareCases, engineInfo, engineUnderTest, headEngineProblemsFor, runEngineCases, sameFiles, sameReport } from './same.ts'

export const usage = `same               runs the reference (${DEFAULT_REFERENCE}) and the engine staged from this checkout on every smoke case's jobs, in the environment a client gives the engine, and writes <work>/parity/same.json: each output identical, identical after parity/masks.json, or different`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length > 0) throw new UsageError(`parity same takes no options, got ${args.join(' ')}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)
  const say = (m: string) => log.line(`parity same: ${m}`)
  const kept = keptReference(paths, REFERENCES[DEFAULT_REFERENCE]!)
  if (!kept) throw new StepError(`the reference ${DEFAULT_REFERENCE} is not unpacked and checked in ${paths.reference}; run parity reference first`)
  const reference = engineUnderTest('reference', kept.dir)
  const head = engineUnderTest('head', join(ctx.work.dist, 'engine'))
  const headProblems = headEngineProblemsFor(head.manifest, ctx.engine, ctx.lock)
  if (headProblems.length > 0) throw new StepError(`${head.dir} is not the engine this checkout stages: ${headProblems.join('; ')}; run build, smoke and stage`)
  for (const e of [reference, head]) if (e.manifest.platform !== platform) throw new StepError(`the ${e.label} engine is for ${e.manifest.platform}, this is ${platform}`)
  const { masks, sha256 } = readMasks()
  const sets = smokePresetSets(join(head.resources, 'profiles'))
  const files = sameFiles(paths)
  const common = {
    root: files.root,
    cases: SMOKE_CASES,
    sets,
    lock: ctx.lock,
    ...(existsSync(ctx.work.src) ? { orcaSource: ctx.work.src } : {}),
    baseEnv: process.env,
    platform,
    timeoutMs: smokeTimeoutMs(),
    log: say,
  }
  say(`reference ${reference.manifest.engine.version} in ${reference.dir}; head ${head.manifest.engine.version} in ${head.dir}; ${SMOKE_CASES.length} cases each, in ${files.root}`)
  const refCases = await runEngineCases(reference, { ...common, store: files.stores.reference })
  const headCases = await runEngineCases(head, { ...common, store: files.stores.head })
  const report = sameReport({
    cases: compareCases(refCases, headCases, files.stores, masks),
    engines: { reference: engineInfo(reference), head: engineInfo(head) },
    masks: { file: 'parity/masks.json', sha256 },
    env: Object.keys(parityEngineEnv(head, process.env, platform)).sort(),
  })
  const tmp = `${files.report}.tmp`
  writeFileSync(tmp, JSON.stringify(report, null, 2) + '\n')
  renameSync(tmp, files.report)
  const s = report.summary
  say(`${s.cases} cases, ${s.runs} engine runs, ${s.outputs} outputs: ${s.identical} identical, ${s.masked} identical after the masks, ${s.different} different`)
  for (const d of report.different) say(`  different: ${d}`)
  for (const c of report.cases) for (const label of ['reference', 'head'] as const) if (!c.smoke[label].ok) say(`  the ${label} engine failed the smoke checks of ${c.id}: ${c.smoke[label].problems.slice(0, 3).join('; ')}`)
  say(`written: ${files.report}`)
  return 0
}
