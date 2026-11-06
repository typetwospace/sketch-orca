// parity certify [--check] [--program <exe>]: runs every corpus case with sketch-orca-rs, the Rust
// translation's program (by default target/debug/sketch-orca-rs.exe, what `bun run rs:build`
// makes; refused when a crate's source, a Cargo file or the toolchain pin is newer than it),
// compares every output with the goldens (certify.ts), and raises the certified flag in
// parity/parity.json of each family whose every run passes; it never lowers one. It writes
// <work>/parity/certify.json. --check runs the same into <work>/parity/runs/certify-check, writes
// <work>/parity/certify-check.json and no parity.json, and fails on a regression. Exit 1 when a
// family parity.json certifies does not pass. It needs parity corpus and parity record.

import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { sha256Bytes } from '../files.ts'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { smokePresetSets, smokeTimeoutMs } from '../smoke.ts'
import { currentPlatform } from '../toolchain.ts'
import {
  CERTIFY_FORMAT,
  certifyFiles,
  classIndex,
  compareCase,
  envelopeIndex,
  familyVerdicts,
  raiseCertified,
  readJsonIf,
  RUST_PROGRAM,
  rustProgramPath,
  rustSources,
  staleSources,
  type CaseVerdict,
  type CertifyReport,
  type Side,
} from './certify.ts'
import { corpusFile, corpusMeshProblems, corpusSmokeCases, readCorpus } from './corpus.ts'
import type { ParityRun } from './dispatch.ts'
import type { EnvelopeReport } from './envelope.ts'
import { readMasks } from './masks.ts'
import { GOLDENS_FORMAT, parityFile, readGoldens, readParity, recordFiles, writeJsonAtomic, type RecordReport } from './record.ts'
import { runEngineCases } from './same.ts'

export const usage = `certify [--check] [--program <exe>]   runs every corpus case with ${RUST_PROGRAM} (target/debug, from bun run rs:build) and compares its outputs with the goldens; raises the certified families in parity/parity.json, never lowers them; --check writes no parity.json and fails on a regression`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  let check = false
  let program: string | undefined
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--check') check = true
    else if (args[i] === '--program' && i + 1 < args.length) program = resolve(args[++i]!)
    else throw new UsageError(`parity certify takes --check and --program <exe>, got ${args.join(' ')}`)
  }
  const say = (m: string) => log.line(`parity certify: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)

  const exe = program ?? rustProgramPath(ctx.engine.engine, platform)
  if (!existsSync(exe)) throw new StepError(`no ${exe}; run bun run rs:build`)
  if (!program) {
    const stale = staleSources(exe, rustSources(ctx.engine.engine))
    if (stale.length > 0) throw new StepError(`${exe} is older than ${stale.slice(0, 5).join(', ')}${stale.length > 5 ? ` and ${stale.length - 5} more` : ''}; run bun run rs:build`)
  }

  const file = corpusFile(ctx.engine.engine)
  const doc = readCorpus(file)
  const corpusSha256 = sha256Bytes(readFileSync(file))
  const index = readGoldens(paths.goldens)
  if (index.format !== GOLDENS_FORMAT) throw new StepError(`${paths.goldens} holds ${JSON.stringify(index.format)}, not ${GOLDENS_FORMAT}; run parity record`)
  if (index.corpusSha256 !== corpusSha256) throw new StepError(`the goldens in ${paths.goldens} were recorded from another parity/corpus.json; run parity record`)
  const meshProblems = corpusMeshProblems(doc, paths.corpus)
  if (meshProblems.length > 0) throw new StepError(`the corpus in ${paths.corpus} is not the one parity/corpus.json names (${meshProblems.slice(0, 5).join('; ')}); run parity corpus`)
  const pf = parityFile(ctx.engine.engine)
  const parity = readParity(pf)
  if (!parity) throw new StepError(`no ${pf}; run parity record`)
  if (parity.corpus.sha256 !== corpusSha256) throw new StepError(`${pf} was written from another parity/corpus.json; run parity record`)

  const record = readJsonIf<RecordReport>(recordFiles(paths).report)
  if (!record || record.corpusSha256 !== corpusSha256) throw new StepError(`no record of this corpus's output classes in ${recordFiles(paths).report}; run parity record`)
  const envelope = readJsonIf<EnvelopeReport>(join(paths.envelope, 'envelope.json'))
  const classOf = classIndex(record.outputs)
  const envelopeOf = envelopeIndex(envelope)
  const { masks } = readMasks()

  const cases = corpusSmokeCases(doc, { corpusDir: paths.corpus, protocolDir: join(ctx.engine.engine, 'protocol') })
  const files = certifyFiles(paths, check)
  say(`${cases.length} cases with ${exe}, in ${files.root}`)
  const records = await runEngineCases(
    { label: RUST_PROGRAM, exe, resources: index.resources, libraryPath: dirname(exe) },
    {
      root: files.root,
      store: files.store,
      cases,
      sets: smokePresetSets(join(index.resources, 'profiles')),
      lock: ctx.lock,
      ...(existsSync(ctx.work.src) ? { orcaSource: ctx.work.src } : {}),
      baseEnv: process.env,
      platform,
      timeoutMs: smokeTimeoutMs(),
      log: (m) => log.detail(m),
    },
  )

  const golden: Side = { store: join(paths.goldens, 'cases'), root: index.root, resources: index.resources, work: index.work }
  const rust: Side = { store: files.store, root: files.root, resources: index.resources, work: ctx.work.home }
  const verdicts = new Map<string, CaseVerdict>()
  for (const c of cases) {
    verdicts.set(
      c.id,
      compareCase({
        caseId: c.id,
        golden: { side: golden, record: index.cases.find((x) => x.id === c.id) },
        rust: { side: rust, record: records.find((x) => x.id === c.id) },
        masks,
        families: parity.families,
        classOf: (r, out) => classOf(c.id, r, out),
        envelopeOf: (r, out) => envelopeOf(c.id, r, out),
      }),
    )
  }
  const families = familyVerdicts(parity, verdicts)
  const { parity: raisedParity, raised, regressions } = raiseCertified(parity, families)
  const report: CertifyReport = {
    format: CERTIFY_FORMAT,
    createdAt: new Date().toISOString(),
    check,
    program: { path: exe, sha256: sha256Bytes(readFileSync(exe)) },
    corpusSha256,
    goldens: paths.goldens,
    summary: { families: families.length, passed: families.filter((f) => f.passed).length, certified: raisedParity.families.filter((f) => f.certified).length, raised, regressions },
    families,
  }
  writeJsonAtomic(files.report, report)
  for (const f of families) {
    const how = f.passed ? (f.declined > 0 ? `passes (${f.declined} of ${f.runs} runs declined past intake)` : `passes (${f.runs} runs)`) : `does not pass: ${f.problems.slice(0, 2).join(' | ')}`
    say(`${f.certified ? 'certified' : '         '} ${f.name}: ${how}`)
  }
  if (!check && raised.length > 0) {
    writeJsonAtomic(pf, raisedParity)
    say(`raised in ${pf}: ${raised.join(', ')}`)
  }
  say(`written: ${files.report}`)
  if (regressions.length > 0) {
    say(`REGRESSION: parity.json certifies ${regressions.join(', ')}, which ${regressions.length === 1 ? 'does' : 'do'} not pass now`)
    return 1
  }
  return 0
}
