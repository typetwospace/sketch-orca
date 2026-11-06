// parity goldens --check <folder>: copies the staged oracle and the corpus to a second work folder,
// records the oracle there again as parity record kept it, and lists every output that differs from
// the goldens once each folder's paths are portable as an H-site (goldens.ts): in parity.json, in
// record.json's classes, and in <work>/parity/goldens-check.json. It needs parity record. Exit 0 when
// it ran (H-sites are findings, recorded for oracle H-patches, not a failure of the check).

import { existsSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { listPatches } from '../checkout.ts'
import { sha256Bytes } from '../files.ts'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { smokePresetSets, smokeTimeoutMs } from '../smoke.ts'
import { currentPlatform } from '../toolchain.ts'
import { corpusFile, readCorpus } from './corpus.ts'
import type { ParityRun } from './dispatch.ts'
import { checkGoldens, parityWithHSites, readRecord } from './goldens.ts'
import { oraclePatches, oracleSourceInputs } from './oracle.ts'
import { parityFile, readParity, recordFiles, recordSummary, writeJsonAtomic } from './record.ts'
import { engineUnderTest } from './same.ts'

export const usage = `goldens --check <folder>   copies the oracle and the corpus to a second work folder, records the oracle there again, and lists every output that differs from the goldens (paths made portable) as an H-site in parity.json`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length !== 2 || args[0] !== '--check') throw new UsageError(`parity goldens takes --check <folder>, got ${args.join(' ') || 'nothing'}`)
  const say = (m: string) => log.line(`parity goldens: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const checkWork = resolve(args[1]!)
  if (resolve(checkWork).toLowerCase() === resolve(ctx.work.home).toLowerCase()) throw new StepError('the second folder is the work folder itself')
  const checkPaths = parityPaths({ home: checkWork })
  for (const [p, h] of [
    [paths, ctx.work.home],
    [checkPaths, checkWork],
  ] as const) {
    const problem = parityPathProblem(p, h, platform)
    if (problem) throw new StepError(problem)
  }
  const file = corpusFile(ctx.engine.engine)
  const doc = readCorpus(file)
  const corpusSha256 = sha256Bytes(readFileSync(file))
  const files = recordFiles(paths)
  const record = readRecord(files.report)
  if (record.corpusSha256 !== corpusSha256) throw new StepError(`${files.report} was recorded from another parity/corpus.json; run parity record`)
  const head = engineUnderTest('head', join(ctx.work.dist, 'engine'))
  const { report } = await checkGoldens({
    paths,
    checkPaths,
    work: ctx.work.home,
    checkWork,
    doc,
    corpusSha256,
    want: oracleSourceInputs(ctx.lock, listPatches(ctx.engine.patches), oraclePatches(ctx.engine.engine), ctx.engine.cli),
    head,
    protocolDir: join(ctx.engine.engine, 'protocol'),
    sets: smokePresetSets(join(head.resources, 'profiles')),
    lock: ctx.lock,
    ...(existsSync(ctx.work.src) ? { orcaSource: ctx.work.src } : {}),
    baseEnv: process.env,
    platform,
    timeoutMs: smokeTimeoutMs(),
    log: say,
  })
  const out = join(paths.parity, 'goldens-check.json')
  writeJsonAtomic(out, report)
  const pf = parityFile(ctx.engine.engine)
  const { parity, outputs } = parityWithHSites({ doc, record, sites: report.hSites, corpusSha256, previous: readParity(pf) })
  writeJsonAtomic(files.report, { ...record, outputs, summary: recordSummary(outputs) })
  writeJsonAtomic(pf, parity)
  say(`${report.compared} outputs compared between ${ctx.work.home} and ${checkWork}: ${report.hSites.length} H-sites`)
  for (const s of report.hSites.slice(0, 40)) say(`  H: ${s.case}/${s.run}/${s.output}: ${s.detail}`)
  say(`written: ${out}, ${pf}, ${files.report}`)
  return 0
}
