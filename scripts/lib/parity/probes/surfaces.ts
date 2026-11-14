// The surfaces probe of phase O3: surfaces.sop1, MutablePolygon, region expansion and the surface
// steps of PrintObject, recorded by orca-o3-probe-surfaces for the records surfaces-1, surfaces-2
// and surfaces-3 replay. Its record kinds start with SF and have their sections on
// parity/sop1/surfaces.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton,
// 2026-11-13, in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('surfaces', 'orca-o3-probe-surfaces', { budget: PROBE_FILE_MAX_BYTES, file: 'surfaces.sop1', crtSymbols: false, kindPrefix: 'SF' })
