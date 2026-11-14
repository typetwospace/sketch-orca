// The walls probe of phase O3: walls.sop1, PerimeterGenerator, classic and Arachne, BridgeDetector
// and the principal components, recorded by orca-o3-probe-walls for the records walls-1, walls-2
// and walls-3 replay. Its record kinds start with WL and have their sections on
// parity/sop1/walls.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton, 2026-11-13,
// in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('walls', 'orca-o3-probe-walls', { budget: PROBE_FILE_MAX_BYTES, file: 'walls.sop1', crtSymbols: false, kindPrefix: 'WL' })
