// The voronoi probe of phase O3: voronoi.sop1, Boost.Polygon's Voronoi builder, libslic3r's Voronoi
// utilities and the medial axis, recorded by orca-o3-probe-voronoi for the records voronoi-1,
// voronoi-2, voronoi-3 and voronoi-4 replay. Its record kinds start with VR and have their sections
// on parity/sop1/voronoi.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton,
// 2026-11-13, in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('voronoi', 'orca-o3-probe-voronoi', { budget: PROBE_FILE_MAX_BYTES, file: 'voronoi.sop1', crtSymbols: false, kindPrefix: 'VR' })
