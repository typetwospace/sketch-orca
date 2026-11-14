// The arachne probe of phase O3: arachne.sop1, the beading strategies, the skeletal trapezoidation
// and WallToolPaths, recorded by orca-o3-probe-arachne for the records arachne-1, arachne-2,
// arachne-3 and arachne-4 replay. Its record kinds start with AN and have their sections on
// parity/sop1/arachne.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton, 2026-11-13,
// in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('arachne', 'orca-o3-probe-arachne', { budget: PROBE_FILE_MAX_BYTES, file: 'arachne.sop1', crtSymbols: false, kindPrefix: 'AN' })
