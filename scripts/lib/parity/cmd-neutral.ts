// parity neutral: the neutrality gate (neutral.ts). It runs every smoke case's jobs with the engine
// staged from this checkout and with the staged oracle, unset, pinned and at two thread counts, and
// writes <work>/parity/neutral.json. It needs `parity oracle` and a dist/engine that this
// checkout's build, smoke and stage made. Exit 0 when (a) and (b) pass, 1 when either fails; (c)
// is recorded either way.

import { existsSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { listPatches } from '../checkout.ts'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { SMOKE_CASES, smokePresetSets, smokeTimeoutMs } from '../smoke.ts'
import { currentPlatform } from '../toolchain.ts'
import type { ParityRun } from './dispatch.ts'
import { readMasks } from './masks.ts'
import { checkPlatforms, NEUTRAL_THREADS, neutralFiles, runNeutral } from './neutral.ts'
import { ORACLE_KNOBS, oraclePatches, oracleSourceInputs, oracleUnderTest } from './oracle.ts'
import { engineInfo, engineUnderTest, headEngineProblemsFor } from './same.ts'

export const usage = `neutral            the neutrality gate: with no knob set the oracle's outputs equal the staged engine's after parity/masks.json, with ${ORACLE_KNOBS.time} and ${ORACLE_KNOBS.seed} set two oracle runs are byte for byte the same, and with ${ORACLE_KNOBS.threads}=${NEUTRAL_THREADS.join(' and =')} what differs is recorded; writes <work>/parity/neutral.json, exit 1 when either of the first two fails`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length > 0) throw new UsageError(`parity neutral takes no options, got ${args.join(' ')}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)
  const say = (m: string) => log.line(`parity neutral: ${m}`)
  const head = engineUnderTest('head', join(ctx.work.dist, 'engine'))
  const headProblems = headEngineProblemsFor(head.manifest, ctx.engine, ctx.lock)
  if (headProblems.length > 0) throw new StepError(`${head.dir} is not the engine this checkout stages: ${headProblems.join('; ')}; run build, smoke and stage`)
  const want = oracleSourceInputs(ctx.lock, listPatches(ctx.engine.patches), oraclePatches(ctx.engine.engine), ctx.engine.cli)
  const oracle = oracleUnderTest(paths.oracleDist, want)
  checkPlatforms(head, oracle, platform)
  const { masks, sha256 } = readMasks()
  const files = neutralFiles(paths)
  say(`head ${head.manifest.engine.version} in ${head.dir}; oracle in ${oracle.dir}; ${SMOKE_CASES.length} cases, in ${files.root}`)
  const report = await runNeutral({
    head,
    headInfo: engineInfo(head),
    oracle,
    paths,
    cases: SMOKE_CASES,
    sets: smokePresetSets(join(head.resources, 'profiles')),
    lock: ctx.lock,
    ...(existsSync(ctx.work.src) ? { orcaSource: ctx.work.src } : {}),
    baseEnv: process.env,
    platform,
    timeoutMs: smokeTimeoutMs(),
    masks,
    masksFile: { file: 'parity/masks.json', sha256 },
    log: say,
  })
  const tmp = `${files.report}.tmp`
  writeFileSync(tmp, JSON.stringify(report, null, 2) + '\n')
  renameSync(tmp, files.report)
  const line = (name: string, s: { outputs: number; identical: number; masked: number; different: number }) => `${name}: ${s.outputs} outputs, ${s.identical} identical, ${s.masked} identical after the masks, ${s.different} different`
  say(`(a) ${report.unset.passed ? 'pass' : 'FAIL'}: ${line('head against the oracle, no knob set', report.unset.summary)}`)
  for (const d of report.unset.different) say(`  different: ${d}`)
  say(`(b) ${report.pinned.passed ? 'pass' : 'FAIL'}: ${line('two pinned oracle runs, no masks', report.pinned.summary)}`)
  for (const d of report.pinned.different) say(`  different: ${d}`)
  const t = report.threads
  say(`(c) ${t.finished ? 'every run finished' : `UNFINISHED: ${t.unfinished.join(', ')}`}; ${line(`${ORACLE_KNOBS.threads}=${NEUTRAL_THREADS[0]} against =${NEUTRAL_THREADS[1]}`, t.summary)}; dumps: ${t.dumps.compared} compared, ${t.dumps.identical} identical, ${t.dumps.different.length} different, ${t.dumps.only.length} in one only, ${t.dumps.unreadable.length} unreadable`)
  for (const a of t.arena) say(`  ${ORACLE_KNOBS.threads}=${a.threads}: ${a.dumps} RUN records; max_concurrency() ${a.reportsP ? 'reports' : 'does NOT report'} ${a.threads}; max_allowed_parallelism ${a.oneThread ? 'is 1' : 'is NOT 1'} (${a.seen.map((s) => `${s.maxConcurrency}/${s.maxAllowedParallelism}/FMA3 ${s.fma3Enable} x${s.dumps}`).join(', ')})`)
  say(`  ${t.candidates.length} T candidates`)
  for (const c of t.candidates.slice(0, 40)) say(`  T candidate: ${c}`)
  say(`written: ${files.report}`)
  return report.passed ? 0 : 1
}
