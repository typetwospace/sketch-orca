// `release`: the steps in order, stopping at the first that fails. Each step skips by itself what
// is already done and verified, so running release again only does what changed.

import { EXIT } from './run.ts'

export interface ReleaseStep {
  name: string
  /** Exit code: 0 done (or already done), anything else stops the release. */
  run(): Promise<number>
}

export interface ReleaseOutcome {
  code: number
  /** Steps that finished, in order. */
  done: string[]
  /** The step that stopped the release. */
  failed?: string
  /** Why, when it threw. */
  error?: string
}

/** Runs the steps in order; a non-zero exit code or an error stops the run there. */
export async function runSteps(steps: ReleaseStep[], line: (msg: string) => void, now: () => number = Date.now): Promise<ReleaseOutcome> {
  const done: string[] = []
  for (const step of steps) {
    const started = now()
    line(`release: ${step.name}`)
    let code: number
    try {
      code = await step.run()
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      line(`release: stopped at ${step.name}: ${error}`)
      return { code: EXIT.failed, done, failed: step.name, error }
    }
    if (code !== 0) {
      line(`release: stopped at ${step.name} (exit ${code})`)
      return { code, done, failed: step.name }
    }
    done.push(step.name)
    line(`release: ${step.name} done (${((now() - started) / 1000).toFixed(1)} s)`)
  }
  line(`release: all ${steps.length} steps done (${done.join(', ')})`)
  return { code: 0, done }
}

/** The command-line options each step of a release takes from `release`'s own. */
export function stepArgs(step: string, args: string[]): string[] {
  const pass: Record<string, string[]> = { source: ['--no-deps', '--allow-missing'] }
  const allowed = pass[step] ?? []
  return args.filter((a) => allowed.includes(a))
}
