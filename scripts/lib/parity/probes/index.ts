// The index probe of phase O3: index.sop1, the AABB trees, the KD tree, the mutable priority queue
// and EdgeGrid, recorded by orca-o3-probe-index for the records index-1 and index-2 replay. Its
// record kinds start with IX and have their sections on parity/sop1/index.md (probe.ts,
// probeKindProblems). Registered by orca-o3-skeleton, 2026-11-13, in a file of its own so that the
// probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('index', 'orca-o3-probe-index', { budget: PROBE_FILE_MAX_BYTES, file: 'index.sop1', crtSymbols: false, kindPrefix: 'IX' })
