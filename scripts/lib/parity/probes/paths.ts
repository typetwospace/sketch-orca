// The paths probe of phase O3: paths.sop1, Clipper with Z, Circle, ArcFitter, the 2D and 3D helpers
// of orca-geom, and ShortestPath's chaining, recorded by orca-o3-probe-paths for the records
// geom-1, geom-2 and paths-1 replay. Its record kinds start with PS and have their sections on
// parity/sop1/paths.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton, 2026-11-13,
// in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('paths', 'orca-o3-probe-paths', { budget: PROBE_FILE_MAX_BYTES, file: 'paths.sop1', crtSymbols: false, kindPrefix: 'PS' })
