// The slice probe of phase O3: slice.sop1, the Model and mesh helpers the slicer calls,
// TriangleMeshSlicer, SlicingParameters and adaptive slicing, recorded by orca-o3-probe-slice for
// the records front-2, slice-1 and slice-2 replay. Its record kinds start with SL and have their
// sections on parity/sop1/slice.md (probe.ts, probeKindProblems). Registered by orca-o3-skeleton,
// 2026-11-13, in a file of its own so that the probe unit changes only this file of the registry.

import { PROBE_FILE_MAX_BYTES, probe } from './entry.ts'

export const entry = probe('slice', 'orca-o3-probe-slice', { budget: PROBE_FILE_MAX_BYTES, file: 'slice.sop1', crtSymbols: false, kindPrefix: 'SL' })
