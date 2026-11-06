// parity envelope: records the shipped engine's own run-to-run variance (envelope.ts) for each
// output parity record classed R, from the engine's recorded runs, into
// <work>/parity/envelope/envelope.json. It needs parity record. Exit 0; an empty envelope (no R
// output) is a finding too.

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import type { ParityRun } from './dispatch.ts'
import { ENVELOPE_FORMAT, outputEnvelope, type EnvelopeReport } from './envelope.ts'
import { engineLabel, outputBytes, recordFiles, writeJsonAtomic, type RecordReport } from './record.ts'
import type { CaseRecord } from './same.ts'

export const usage = `envelope           records the shipped engine's own run-to-run variance for each output parity record classed R (which lines vary, and by how much each number on them), into <work>/parity/envelope/envelope.json`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length > 0) throw new UsageError(`parity envelope takes no options, got ${args.join(' ')}`)
  const say = (m: string) => log.line(`parity envelope: ${m}`)
  const paths = parityPaths(ctx.work)
  const files = recordFiles(paths)
  if (!existsSync(files.report)) throw new StepError(`no ${files.report}; run parity record`)
  const report = JSON.parse(readFileSync(files.report, 'utf8')) as RecordReport
  const passes = new Map<string, CaseRecord[]>()
  for (let n = 1; n <= report.engineRuns; n++) {
    const label = engineLabel(n)
    const f = join(files.store(label), 'pass.json')
    if (!existsSync(f)) throw new StepError(`no ${f}; run parity record`)
    passes.set(label, (JSON.parse(readFileSync(f, 'utf8')) as { cases: CaseRecord[] }).cases)
  }
  const r = report.outputs.filter((o) => o.class === 'R' || o.flags.includes('R'))
  const outputs = r.map((o) =>
    outputEnvelope(
      o,
      [...passes.entries()].map(([label, cases]) => outputBytes(files.store(label), o.case, cases.find((c) => c.id === o.case)?.runs[o.run], o.output)),
    ),
  )
  const env: EnvelopeReport = { format: ENVELOPE_FORMAT, createdAt: new Date().toISOString(), record: { file: files.report, createdAt: report.createdAt }, outputs }
  const out = join(paths.envelope, 'envelope.json')
  writeJsonAtomic(out, env)
  say(`${outputs.length} R outputs of ${report.outputs.length}`)
  for (const o of outputs.slice(0, 40)) say(`  ${o.case}/${o.run}/${o.output}: ${o.kind}${o.kind === 'text' ? `, ${o.varyingLines} lines vary` : ''}`)
  say(`written: ${out}`)
  return 0
}
