// parity reference: downloads the published sketch-orca 0.1.0 engine and unpacks it to
// <work>/parity/reference/0.1.0 (reference.ts). Nothing is sent to GitHub but the requests
// themselves: no token, as the release and its assets are public.

import { githubGetter } from '../github.ts'
import { parityPathProblem, parityPaths } from '../paths.ts'
import { StepError, UsageError } from '../run.ts'
import { currentPlatform } from '../toolchain.ts'
import type { ParityRun } from './dispatch.ts'
import { DEFAULT_REFERENCE, fetchReference, REFERENCES } from './reference.ts'

export const usage = `reference          downloads the published sketch-orca ${DEFAULT_REFERENCE} engine zip (not the source zip), checks it against the release's SHA256SUMS and unpacks it to <work>/parity/reference/${DEFAULT_REFERENCE}`

export const locked = true

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  if (args.length > 0) throw new UsageError(`parity reference takes no options, got ${args.join(' ')}`)
  const paths = parityPaths(ctx.work)
  const problem = parityPathProblem(paths, ctx.work.home, currentPlatform())
  if (problem) throw new StepError(problem)
  const r = await fetchReference({ pin: REFERENCES[DEFAULT_REFERENCE]!, paths, get: githubGetter(''), log: (m) => log.line(`parity reference: ${m}`) })
  log.line(`parity reference: done: ${r.dir}`)
  return 0
}
