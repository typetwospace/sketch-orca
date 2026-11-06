// parity corpus: builds the corpus (corpus.ts, parity/CORPUS.md). It converts Orca's 21 OBJ test
// meshes to binary STL as Sketch writes model.stl and copies the 6 STL files and the 3MF as they are,
// from the engine's checkout (<work>/src/tests/data) into <work>/parity/corpus/meshes, works out the
// families each case exercises from its presets (<work>/src/resources/profiles), its overrides and
// Orca's defaults (parity/probes/configdef.json), and writes parity/corpus.json (and a copy beside
// the meshes). It needs the engine's checkout (`fetch`) and parity/probes (parity probe).

import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { smokePresetSets } from '../smoke.ts'
import { currentPlatform } from '../toolchain.ts'
import { buildMeshes, configDefaults, corpusCases, corpusDocument, corpusFile, ORCA_TEST_DATA, withFamilies } from './corpus.ts'
import type { ParityRun } from './dispatch.ts'

export const usage = `corpus             builds the parity corpus: Orca's 28 test meshes (the OBJ files as binary STL, as Sketch writes model.stl) into <work>/parity/corpus, and parity/corpus.json with every case (the smoke cases, each mesh as info, layers and slice, the Sketch-shaped jobs, the invalid examples and the refusals) and the families each exercises`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length > 0) throw new UsageError(`parity corpus takes no options, got ${args.join(' ')}`)
  const say = (m: string) => log.line(`parity corpus: ${m}`)
  const platform = currentPlatform()
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, platform)
  if (problem) throw new StepError(problem)
  const testData = join(ctx.work.src, ...ORCA_TEST_DATA.split('/'))
  if (!existsSync(testData)) throw new StepError(`no ${testData}; run fetch (the engine's checkout at the locked commit)`)
  const configdef = join(ctx.engine.engine, 'parity', 'probes', 'configdef.json')
  if (!existsSync(configdef)) throw new StepError(`no ${configdef}; run parity probe`)
  const meshes = buildMeshes(testData, paths.corpus)
  say(`${meshes.length} meshes into ${join(paths.corpus, 'meshes')}: ${meshes.filter((m) => m.kind === 'obj').length} OBJ converted, ${meshes.filter((m) => m.kind !== 'obj').length} as they are`)
  const protocolDir = join(ctx.engine.engine, 'protocol')
  const examples = readdirSync(join(protocolDir, 'examples', 'invalid'))
  const sets = smokePresetSets(join(ctx.work.src, 'resources', 'profiles'))
  const defaults = configDefaults(JSON.parse(readFileSync(configdef, 'utf8')))
  const cases = withFamilies(corpusCases(protocolDir, examples), sets, defaults)
  const doc = corpusDocument({ orcaCommit: ctx.lock.commit, meshes, cases })
  const text = JSON.stringify(doc, null, 2) + '\n'
  writeFileSync(corpusFile(ctx.engine.engine), text)
  writeFileSync(join(paths.corpus, 'corpus.json'), text)
  const count = (k: string) => cases.filter((c) => c.kind === k).length
  say(`${cases.length} cases: ${count('smoke')} smoke, ${count('job')} jobs, ${count('example')} invalid examples, ${count('refusal')} refusals; ${doc.families.length} families`)
  const unexercised = doc.families.filter((f) => !cases.some((c) => c.families.includes(f.name))).map((f) => f.name)
  if (unexercised.length > 0) say(`no case exercises yet: ${unexercised.join(', ')}`)
  say(`written: ${corpusFile(ctx.engine.engine)}`)
  return 0
}
