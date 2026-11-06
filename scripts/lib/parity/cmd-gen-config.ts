// parity gen-config [--check]: writes crates/orca-config/src/def_generated.rs, the Rust
// translation's configuration definitions, from the configdef probe (parity/probes/configdef.json,
// `parity probe`), with gen-config.ts. With --check it writes nothing and exits 1 while the
// committed file differs from what the probe gives (or is missing), 0 when it is the same. It needs
// only the repository: no work folder, no engine.

import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { StepError, UsageError } from '../run.ts'
import type { ParityRun } from './dispatch.ts'
import { CONFIGDEF_PROBE, configDefsRust, DEF_GENERATED, readConfigDefProbe } from './gen-config.ts'

export const usage = `gen-config [--check]   writes crates/orca-config/src/def_generated.rs (Orca's configuration definitions for the Rust translation) from parity/probes/configdef.json; --check exits 1 while the committed file differs`

export const locked = false

export async function run({ ctx, args, log }: ParityRun): Promise<number> {
  const check = args.length === 1 && args[0] === '--check'
  if (args.length > 0 && !check) throw new UsageError(`parity gen-config takes [--check], got ${args.join(' ')}`)
  const say = (m: string) => log.line(`parity gen-config: ${m}`)
  const root = ctx.engine.engine
  const probe = join(root, ...CONFIGDEF_PROBE.split('/'))
  if (!existsSync(probe)) throw new StepError(`no ${probe}; run parity probe`)
  const text = configDefsRust(readConfigDefProbe(root))
  const out = join(root, ...DEF_GENERATED.split('/'))
  const now = existsSync(out) ? readFileSync(out, 'utf8') : null
  if (check) {
    if (now === text) {
      say(`${DEF_GENERATED} is what ${CONFIGDEF_PROBE} gives`)
      return 0
    }
    say(now === null ? `no ${DEF_GENERATED}; run parity gen-config` : `${DEF_GENERATED} differs from what ${CONFIGDEF_PROBE} gives; run parity gen-config`)
    return 1
  }
  if (now === text) say(`${DEF_GENERATED} is already what ${CONFIGDEF_PROBE} gives`)
  else {
    writeFileSync(out, text)
    say(`written: ${out}`)
  }
  return 0
}
