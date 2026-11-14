// parity certify [--check] [--program <exe>]: runs every corpus case with sketch-orca-rs, the Rust
// translation's program (by default target/debug/sketch-orca-rs.exe, what `bun run rs:build`
// makes; refused when a crate's source, a Cargo file or the toolchain pin is newer than it),
// compares every output with the goldens (certify.ts), and raises the certified flag in
// parity/parity.json of each family whose every run passes; it never lowers one. It writes
// <work>/parity/certify.json. --check runs the same into <work>/parity/runs/certify-check, writes
// <work>/parity/certify-check.json and no parity.json, and fails on a regression. A run the program
// declines passes in no family. Exit 1 when a family parity.json certifies does not pass, or when a
// run of a corpus job, example or refusal (DECLINE_HELD_KINDS) that reaches a family parity.json
// does not certify is not declined naming it, its C++ run having gone past what the program does
// itself (certify.ts declineGaps), the families held as this run raises them. The program runs with
// the golden pass's knobs and a fresh SKETCH_ORCA_ORACLE_DUMP folder per run, and the dumps it keeps
// (record.ts KEPT_DUMPS) are compared with the goldens'; a decline that reached a boundary passes the
// families before it (certify.ts BOUNDARY_FAMILIES). After a run the program declines, its case goes
// on from the golden C++ run's outputs (certify.ts goldenCarry), so a smoke case's later runs are
// made, from what the C++ program had, and judged; the report lists those declines as `carried`. It
// prints what the declines wrote to their streams (`decline streams: <n> declines; stderr ...;
// stdout ...`, certify.ts declineStreams) and, for each family, its answered runs (`pre-boundary
// holders of <family>: <k> answered runs`, the first five named, certify.ts preBoundaryHolders);
// neither changes the exit status. It needs parity corpus, and parity oracle, neutral and record run
// for this checkout's cli/ and patches (certify.ts oracleChainProblems).

import { existsSync, readFileSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { listPatches } from '../checkout.ts'
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
  DECLINE_HELD_KINDS,
  declineGaps,
  declineStreams,
  declineStreamsLine,
  envelopeIndex,
  familyVerdicts,
  goldenCarry,
  oracleChainProblems,
  preBoundaryHolders,
  preBoundaryLine,
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
import { neutralFiles, type NeutralReport } from './neutral.ts'
import { oraclePatches, oracleSourceInputs, oracleUnderTest } from './oracle.ts'
import { knobRunner } from './neutral.ts'
import { collectDumps, dumpsRoot, GOLDENS_FORMAT, keptDump, parityFile, readGoldens, readParity, recordFiles, writeJsonAtomic, type RecordReport } from './record.ts'
import { runEngineCases, runHidden } from './same.ts'

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
  // The goldens stand for the C++ engine of this checkout only when the chain after a cli/ change ran.
  const oracle = oracleUnderTest(paths.oracleDist, oracleSourceInputs(ctx.lock, listPatches(ctx.engine.patches), oraclePatches(ctx.engine.engine), ctx.engine.cli))
  const chain = oracleChainProblems({
    oracle: { exeSha256: oracle.manifest.exe.sha256, cli: oracle.manifest.cli },
    neutral: readJsonIf<NeutralReport>(neutralFiles(paths).report),
    goldens: index,
  })
  if (chain.length > 0) throw new StepError(`the goldens in ${paths.goldens} do not stand for this checkout's engine: ${chain.join('; ')}`)
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
  // The golden pass's knobs (time and seed pinned, one thread) and a fresh dump folder per run.
  const dumps = dumpsRoot(join(files.store, '..'), 'rust')
  rmSync(dumps, { recursive: true, force: true })
  say(`${cases.length} cases with ${exe}, in ${files.root}, with ${Object.entries(index.knobs).map(([k, v]) => `${k}=${v}`).join(' ') || 'no knobs'}`)
  const golden: Side = { store: join(paths.goldens, 'cases'), root: index.root, resources: index.resources, work: index.work }
  const rust: Side = { store: files.store, root: files.root, resources: index.resources, work: ctx.work.home }
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
      run: knobRunner(index.knobs, dumps, runHidden, (f) => keptDump(f)),
      // Past a declined run, the case goes on from the golden run's outputs.
      after: goldenCarry({ golden, goldens: index.cases, rust }),
      log: (m) => log.detail(m),
    },
  )
  collectDumps(dumps, files.store, records)
  const carried = records.flatMap((c) => c.runs.filter((r) => r.carried).map((r) => `${c.id}/${r.index}`))

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
  // The decline against the corpus: each run of a case that reaches an uncertified family is
  // declined, naming it (smoke cases aside: DECLINE_HELD_KINDS), the families as this run raised them.
  const held = doc.cases.filter((c) => (DECLINE_HELD_KINDS as readonly string[]).includes(c.kind))
  const declineGapList = held.flatMap((c) =>
    declineGaps({
      caseId: c.id,
      reaches: c.families,
      families: raisedParity.families,
      rust: { side: rust, record: records.find((x) => x.id === c.id) },
      golden: { side: golden, record: index.cases.find((x) => x.id === c.id) },
    }),
  )
  // What the declines wrote to their streams, and which answered runs hold each family's lines (J11).
  const streams = declineStreams(
    cases.map((c) => ({
      caseId: c.id,
      golden: { side: golden, record: index.cases.find((x) => x.id === c.id) },
      rust: { side: rust, record: records.find((x) => x.id === c.id) },
    })),
  )
  const holders = preBoundaryHolders(parity, verdicts)
  const report: CertifyReport = {
    format: CERTIFY_FORMAT,
    createdAt: new Date().toISOString(),
    check,
    program: { path: exe, sha256: sha256Bytes(readFileSync(exe)) },
    corpusSha256,
    goldens: paths.goldens,
    summary: { families: families.length, passed: families.filter((f) => f.passed).length, certified: raisedParity.families.filter((f) => f.certified).length, raised, regressions },
    families,
    declineGaps: declineGapList,
    carried,
    declineStreams: streams,
    preBoundaryHolders: holders,
  }
  writeJsonAtomic(files.report, report)
  for (const f of families) {
    const boundary = f.boundaryPasses > 0 ? `, ${f.boundaryPasses} by declines past its boundary` : ''
    const how = f.passed ? `passes (${f.runs} runs${boundary})` : `does not pass${f.declined > 0 ? ` (${f.declined} of ${f.runs} runs declined${boundary})` : ''}: ${f.problems.slice(0, 2).join(' | ')}`
    say(`${f.certified ? 'certified' : '         '} ${f.name}: ${how}`)
  }
  if (!check && raised.length > 0) {
    writeJsonAtomic(pf, raisedParity)
    say(`raised in ${pf}: ${raised.join(', ')}`)
  }
  say(`written: ${files.report}`)
  say(`${carried.length} declined runs were carried (the case went on from the golden run's outputs; a later run of such a case was made from what the C++ program had)`)
  say(declineStreamsLine(streams))
  for (const n of streams.notPrefix.slice(0, 5)) say(`  not a prefix: ${n}`)
  for (const h of holders) say(preBoundaryLine(h))
  if (declineGapList.length === 0) say(`the decline: every run of the ${held.length} corpus jobs, examples and refusals that reaches an uncertified family is declined, naming each`)
  else say(`DECLINE GAP: ${declineGapList.length} runs of corpus jobs, examples and refusals are let through for a family that is not certified: ${declineGapList.slice(0, 5).join(' | ')}`)
  if (regressions.length > 0) say(`REGRESSION: parity.json certifies ${regressions.join(', ')}, which ${regressions.length === 1 ? 'does' : 'do'} not pass now`)
  return regressions.length > 0 || declineGapList.length > 0 ? 1 : 0
}
