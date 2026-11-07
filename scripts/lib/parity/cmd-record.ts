// parity record [--only <regex>]: runs every corpus case with the oracle (time and seed pinned) at
// thread counts 1, 2, 8 and the machine's concurrency, and with the engine staged from this checkout
// five times (record.ts); classifies every output (classes.ts); keeps the oracle's one-thread pass as
// the goldens (<work>/parity/goldens); writes the front end's goldens (parity/goldens/front/) and
// parity/parity.json; and writes <work>/parity/record.json. A pass whose program, knobs and corpus
// have not changed is kept, not run again. --only runs the cases whose id matches, into
// <work>/parity/runs/record-only, and writes neither goldens nor parity.json (for trying a case).
// It needs parity corpus, parity oracle, and build, smoke and stage of this checkout.

import { availableParallelism } from 'node:os'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { listPatches } from '../checkout.ts'
import { sha256Bytes } from '../files.ts'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { smokePresetSets, smokeTimeoutMs } from '../smoke.ts'
import { currentPlatform } from '../toolchain.ts'
import { corpusFile, corpusMeshProblems, corpusSmokeCases, readCorpus } from './corpus.ts'
import type { ParityRun } from './dispatch.ts'
import { readMasks } from './masks.ts'
import { checkPlatforms } from './neutral.ts'
import { oraclePatches, oracleSourceInputs, oracleUnderTest } from './oracle.ts'
import {
  classifyPasses,
  keepGoldens,
  oracleLabel,
  parityDocument,
  parityFile,
  readParity,
  RECORD_ENGINE_RUNS,
  RECORD_FORMAT,
  recordFiles,
  recordSummary,
  recordThreads,
  runPasses,
  writeFrontGoldens,
  writeJsonAtomic,
  type RecordReport,
} from './record.ts'
import { engineInfo, engineUnderTest, headEngineProblemsFor } from './same.ts'

export const usage = `record [--only <regex>]   runs every corpus case with the oracle (time and seed pinned) at 1, 2, 8 and the machine's thread count and with the staged engine ${RECORD_ENGINE_RUNS} times, classifies every output (B, T, R, N, E; H after goldens --check), keeps the oracle's one-thread outputs in <work>/parity/goldens, and writes parity/goldens/front/ and parity/parity.json`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  let only: RegExp | undefined
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--only' && i + 1 < args.length) only = new RegExp(args[++i]!)
    else throw new UsageError(`parity record takes only --only <regex>, got ${args.join(' ')}`)
  }
  const say = (m: string) => log.line(`parity record: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)
  const head = engineUnderTest('head', join(ctx.work.dist, 'engine'))
  const headProblems = headEngineProblemsFor(head.manifest, ctx.engine, ctx.lock)
  if (headProblems.length > 0) throw new StepError(`${head.dir} is not the engine this checkout stages: ${headProblems.join('; ')}; run build, smoke and stage`)
  const oracle = oracleUnderTest(paths.oracleDist, oracleSourceInputs(ctx.lock, listPatches(ctx.engine.patches), oraclePatches(ctx.engine.engine), ctx.engine.cli))
  checkPlatforms(head, oracle, platform)
  const file = corpusFile(ctx.engine.engine)
  const doc = readCorpus(file)
  const corpusSha256 = sha256Bytes(readFileSync(file))
  const meshProblems = corpusMeshProblems(doc, paths.corpus)
  if (meshProblems.length > 0) throw new StepError(`the corpus in ${paths.corpus} is not the one parity/corpus.json names (${meshProblems.slice(0, 5).join('; ')}); run parity corpus`)
  const protocolDir = join(ctx.engine.engine, 'protocol')
  const all = corpusSmokeCases(doc, { corpusDir: paths.corpus, protocolDir })
  const cases = only ? all.filter((c) => only!.test(c.id)) : all
  if (cases.length === 0) throw new StepError(`no corpus case matches ${only}`)
  const threads = recordThreads(availableParallelism())
  const files = recordFiles(paths, only ? 'record-only' : 'record')
  const { masks } = readMasks()
  say(`${cases.length} cases; the oracle at ${threads.join(', ')} threads with time and seed pinned, the engine ${RECORD_ENGINE_RUNS} times; in ${files.root}`)
  const passes = await runPasses({
    doc,
    corpusSha256,
    cases,
    oracle,
    engine: head,
    engineExeSha256: engineInfo(head).exeSha256,
    threads,
    engineRuns: RECORD_ENGINE_RUNS,
    files,
    sets: smokePresetSets(join(head.resources, 'profiles')),
    lock: ctx.lock,
    ...(existsSync(ctx.work.src) ? { orcaSource: ctx.work.src } : {}),
    baseEnv: process.env,
    platform,
    timeoutMs: smokeTimeoutMs(),
    log: say,
  })
  const outputs = classifyPasses(passes, files.store, masks)
  const report: RecordReport = {
    format: RECORD_FORMAT,
    createdAt: new Date().toISOString(),
    corpusSha256,
    smokeVersion: doc.smokeVersion,
    threads,
    engineRuns: RECORD_ENGINE_RUNS,
    passes: passes.map((p) => ({ label: p.label, knobs: p.knobs, failed: p.cases.filter((c) => !c.ok).map((c) => `${c.id}: ${c.problems.slice(0, 3).join('; ')}`) })),
    summary: recordSummary(outputs),
    outputs,
  }
  writeJsonAtomic(files.report, report)
  const s = report.summary
  say(`${s.outputs} outputs: ${s.B} B, ${s.E} E, ${s.N} N, ${s.T} T, ${s.R} R`)
  for (const p of report.passes) if (p.failed.length > 0) say(`${p.label}: ${p.failed.length} cases did not pass their own checks (recorded): ${p.failed.slice(0, 5).join(' | ')}`)
  for (const o of outputs.filter((x) => x.class === 'N' || x.class === 'T' || x.class === 'R').slice(0, 40)) say(`  ${o.class}: ${o.case}/${o.run}/${o.output}: ${o.details[o.class] ?? ''}`)
  say(`written: ${files.report}`)
  if (only) return 0
  const kept = passes.find((p) => p.label === oracleLabel(1))!
  const index = keepGoldens({ pass: kept, store: files.store(kept.label), goldens: files.goldens, corpusSha256, root: files.root, resources: oracle.resources, work: ctx.work.home, oracle: { exeSha256: oracle.manifest.exe.sha256, cli: oracle.manifest.cli } })
  say(`goldens: ${kept.label} kept in ${files.goldens}`)
  const front = writeFrontGoldens({ doc, index, goldens: files.goldens, engineDir: ctx.engine.engine })
  say(`front-end goldens: ${front.length} files in parity/goldens/front`)
  const pf = parityFile(ctx.engine.engine)
  const parity = parityDocument({ doc, corpusSha256, threads, engineRuns: RECORD_ENGINE_RUNS, outputs, hSites: [], previous: readParity(pf) })
  writeJsonAtomic(pf, parity)
  say(`written: ${pf} (${parity.families.length} families; H-sites come from parity goldens --check)`)
  return 0
}
