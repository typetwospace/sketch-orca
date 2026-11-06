// parity goldens --check <folder>: whether the goldens hang on the folder they were made in. It
// copies the staged oracle and the corpus into a second work folder (<folder>/oracle/dist and
// <folder>/parity/corpus, hard links where the volume allows), runs the oracle there as parity record
// kept it (time and seed pinned, one thread), and compares every output with the goldens, each
// folder's own paths written as <case>, <cases>, <resources> and <work> (record.ts portableRoots).
// Any output that still differs is an H-site: it is listed in parity.json (hSites) for an oracle
// H-patch, and record.json's class of that output becomes H. A zip (a .3mf) is compared as its
// bytes, so a path inside one is an H-site too.

import { existsSync, readFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { copyTree } from '../files.ts'
import type { Platform } from '../env.ts'
import type { OrcaLock } from '../lock.ts'
import type { ParityPaths } from '../paths.ts'
import { StepError } from '../run.ts'
import type { SmokePresetSets } from '../smoke.ts'
import { portableBytes } from './classes.ts'
import { corpusSmokeCases, type CorpusDocument } from './corpus.ts'
import { oracleUnderTest, type OracleSourceInputs, type OracleUnderTest } from './oracle.ts'
import {
  oracleLabel,
  outputBytes,
  outputNames,
  parityDocument,
  portableRoots,
  readGoldens,
  recordFiles,
  runPasses,
  type HSite,
  type OutputRecord,
  type ParityDocument,
  type RecordReport,
} from './record.ts'
import type { EngineUnderTest, RunProcess } from './same.ts'

export const GOLDENS_CHECK_FORMAT = 'sketch-orca-parity-goldens-check/1'

export interface GoldensCheckReport {
  format: typeof GOLDENS_CHECK_FORMAT
  createdAt: string
  folder: string
  corpusSha256: string
  compared: number
  hSites: HSite[]
}

/** Copies the staged oracle and the corpus into the second folder (fresh), hard links where it can. */
export function copyIntoSecondFolder(from: Pick<ParityPaths, 'oracleDist' | 'corpus'>, to: Pick<ParityPaths, 'oracleDist' | 'corpus'>): { oracle: number; corpus: number } {
  for (const d of [to.oracleDist, to.corpus]) rmSync(d, { recursive: true, force: true })
  const oracle = copyTree(from.oracleDist, to.oracleDist, undefined, { link: true }).length
  const corpus = copyTree(from.corpus, to.corpus, undefined, { link: true }).length
  return { oracle, corpus }
}

/** The outputs of the goldens and of the second folder's pass that differ once each folder's paths are portable. */
export function folderDifferences(o: {
  golden: { store: string; cases: { id: string; runs: Parameters<typeof outputBytes>[2][] }[]; root: string; resources: string; work: string }
  check: { store: string; cases: { id: string; runs: Parameters<typeof outputBytes>[2][] }[]; root: string; resources: string; work: string }
}): { compared: number; sites: HSite[] } {
  const sites: HSite[] = []
  let compared = 0
  for (const g of o.golden.cases) {
    const c = o.check.cases.find((x) => x.id === g.id)
    const n = Math.max(g.runs.length, c?.runs.length ?? 0)
    for (let i = 0; i < n; i++) {
      const gr = g.runs[i]
      const cr = c?.runs[i]
      for (const name of outputNames([gr, cr])) {
        compared++
        const a = portableBytes(outputBytes(o.golden.store, g.id, gr, name), portableRoots({ root: o.golden.root, caseId: g.id, resources: o.golden.resources, work: o.golden.work }))
        const b = portableBytes(outputBytes(o.check.store, g.id, cr, name), portableRoots({ root: o.check.root, caseId: g.id, resources: o.check.resources, work: o.check.work }))
        if (a === undefined && b === undefined) continue
        if (a === undefined || b === undefined) sites.push({ case: g.id, run: i, output: name, detail: `written in the ${a === undefined ? 'second' : 'first'} folder only` })
        else if (!a.equals(b)) sites.push({ case: g.id, run: i, output: name, detail: 'differs between the folders' })
      }
    }
  }
  return { compared, sites }
}

/** record.json's outputs with H added where the folders differ (H comes first, so it is the class). */
export function withHSites(outputs: OutputRecord[], sites: HSite[]): OutputRecord[] {
  const key = (x: { case: string; run: number; output: string }) => `${x.case}\u0000${x.run}\u0000${x.output}`
  const h = new Map(sites.map((s) => [key(s), s]))
  const seen = new Set<string>()
  const out = outputs.map((o) => {
    const s = h.get(key(o))
    const flags = o.flags.filter((f) => f !== 'H')
    if (!s) return { ...o, class: flags[0] ?? 'B', flags: flags.length > 0 ? flags : ['B' as const] }
    seen.add(key(o))
    const rest = flags.filter((f) => f !== 'B')
    return { ...o, class: 'H' as const, flags: ['H' as const, ...rest], details: { ...o.details, H: s.detail } }
  })
  for (const s of sites) if (!seen.has(key(s))) out.push({ case: s.case, run: s.run, output: s.output, class: 'H', flags: ['H'], details: { H: s.detail } })
  return out
}

export interface GoldensCheckOptions {
  paths: ParityPaths
  checkPaths: ParityPaths
  work: string
  checkWork: string
  doc: CorpusDocument
  corpusSha256: string
  want: OracleSourceInputs
  head: EngineUnderTest
  protocolDir: string
  sets: SmokePresetSets
  lock: OrcaLock
  orcaSource?: string
  baseEnv: NodeJS.ProcessEnv
  platform: Platform
  timeoutMs: number
  run?: RunProcess
  log: (msg: string) => void
}

/** Copies, runs and compares (see the top of this file); the caller writes the reports. */
export async function checkGoldens(o: GoldensCheckOptions): Promise<{ report: GoldensCheckReport; oracle: OracleUnderTest }> {
  const index = readGoldens(o.paths.goldens)
  if (index.corpusSha256 !== o.corpusSha256) throw new StepError('the goldens were recorded from another parity/corpus.json; run parity record')
  if (!existsSync(o.paths.oracleDist)) throw new StepError(`no oracle in ${o.paths.oracleDist}; run parity oracle`)
  const copied = copyIntoSecondFolder(o.paths, o.checkPaths)
  o.log(`copied the oracle (${copied.oracle} files) and the corpus (${copied.corpus} files) into ${o.checkWork}`)
  const oracle = oracleUnderTest(o.checkPaths.oracleDist, o.want, 'oracle (second folder)')
  const files = recordFiles(o.checkPaths)
  const cases = corpusSmokeCases(o.doc, { corpusDir: o.checkPaths.corpus, protocolDir: o.protocolDir })
  const passes = await runPasses({
    doc: o.doc,
    corpusSha256: o.corpusSha256,
    cases,
    oracle,
    engine: o.head,
    engineExeSha256: '',
    threads: [1],
    engineRuns: 0,
    files,
    sets: o.sets,
    lock: o.lock,
    ...(o.orcaSource ? { orcaSource: o.orcaSource } : {}),
    baseEnv: o.baseEnv,
    platform: o.platform,
    timeoutMs: o.timeoutMs,
    ...(o.run ? { run: o.run } : {}),
    log: o.log,
  })
  const pass = passes.find((p) => p.label === oracleLabel(1))!
  const { compared, sites } = folderDifferences({
    golden: { store: join(o.paths.goldens, 'cases'), cases: index.cases, root: index.root, resources: index.resources, work: index.work },
    check: { store: files.store(pass.label), cases: pass.cases, root: files.root, resources: oracle.resources, work: o.checkWork },
  })
  return { report: { format: GOLDENS_CHECK_FORMAT, createdAt: new Date().toISOString(), folder: o.checkWork, corpusSha256: o.corpusSha256, compared, hSites: sites }, oracle }
}

/** parity.json again, with the H-sites and the classes they change. */
export function parityWithHSites(o: { doc: CorpusDocument; record: RecordReport; sites: HSite[]; corpusSha256: string; previous?: ParityDocument }): { parity: ParityDocument; outputs: OutputRecord[] } {
  const outputs = withHSites(o.record.outputs, o.sites)
  return { parity: parityDocument({ doc: o.doc, corpusSha256: o.corpusSha256, threads: o.record.threads, engineRuns: o.record.engineRuns, outputs, hSites: o.sites, ...(o.previous ? { previous: o.previous } : {}) }), outputs }
}

export function readRecord(path: string): RecordReport {
  if (!existsSync(path)) throw new StepError(`no ${path}; run parity record`)
  return JSON.parse(readFileSync(path, 'utf8')) as RecordReport
}
