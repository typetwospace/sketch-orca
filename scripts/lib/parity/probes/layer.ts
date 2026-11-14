// The layer probe of phase O3: layer.sop1, the static print configs, extrusion entities, Flow,
// variable width, Surface, Layer and its regions, recorded by orca-o3-probe-layer for the records
// front-1, layer-1, layer-2 and layer-3 replay. Its record kinds start with LY and have their
// sections on parity/sop1/layer.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton,
// 2026-11-13, in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('layer', 'orca-o3-probe-layer', { budget: PROBE_FILE_MAX_BYTES, file: 'layer.sop1', crtSymbols: false, kindPrefix: 'LY' })
