// The fill probe of phase O3: fill.sop1, the infill of domain A (FillBase, the rectilinear family,
// crosshatch, the concentric patterns, new_from_type and the infill rotation), recorded by
// orca-o3-probe-fill (2026-11-13; parity/oracle/probe/fill.cpp and fill/) for the records fill-1,
// fill-2, fill-3 and fill-4 replay. Its record kinds start with FL and have their sections on
// parity/sop1/fill.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton, 2026-11-13, in
// a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('fill', 'orca-o3-probe-fill', { budget: PROBE_FILE_MAX_BYTES, file: 'fill.sop1', crtSymbols: false, kindPrefix: 'FL' })
