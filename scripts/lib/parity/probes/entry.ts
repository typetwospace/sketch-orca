// One probe of the registry (probe.ts, PROBE_REGISTRY): the entry's shape and probe(), which
// makes one. They live here, not in probe.ts, so that a probe's own file (probes/<name>.ts, one per
// probe from phase O3, orca-o3-skeleton 2026-11-13) can import probe() while probe.ts imports that
// file: probe.ts importing a file that imports probe() back from probe.ts would meet probe() before
// it is initialised. probe() is a function declaration for the same reason (it is hoisted).

/** The committed size limit of each later probe file, on its own. */
export const PROBE_FILE_MAX_BYTES = 512 * 1024

/** One probe: a program of its own that writes one file into each set. */
export interface ProbeEntry {
  /** The name --only takes: the program is sketch-orca-probe-<name>, its source <name>.cpp. */
  readonly name: string
  /** The file it writes into each set. */
  readonly file: string
  /** Its committed budget: shared with the other foundation files, or a limit of its own in bytes. */
  readonly budget: 'foundations' | number
  /** Whether it takes the oracle map's CRT maths functions (--crt-symbols). */
  readonly crtSymbols: boolean
  /** The task that records into it: O1 for the foundations; an O2 task's file is a stub until it runs. */
  readonly owner: string
  /**
   * Phase O3's probes only: the two letters every record kind of its file starts with, so that
   * probe units recording side by side cannot take the same kind (probe.ts, probeKindProblems).
   * The O1 and O2 entries leave it out.
   */
  readonly kindPrefix?: string
}

/** A probe's entry: its file is <name>.sop1 with 512 KB of its own and no CRT symbols, unless told otherwise. */
export function probe(name: string, owner: string, o: Partial<Pick<ProbeEntry, 'file' | 'budget' | 'crtSymbols' | 'kindPrefix'>> = {}): ProbeEntry {
  return {
    name,
    file: o.file ?? `${name}.sop1`,
    budget: o.budget ?? PROBE_FILE_MAX_BYTES,
    crtSymbols: o.crtSymbols ?? false,
    owner,
    ...(o.kindPrefix === undefined ? {} : { kindPrefix: o.kindPrefix }),
  }
}
