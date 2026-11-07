// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-geom: `geom.sop1`'s records but the Eigen ones, which orca-eigen replays
//! (`parity/SOP1.md`, "Probe files"), from the committed subset (`parity/probes/`), or from the
//! full set when `SKETCH_ORCA_PROBES_DEEP` names its folder (`<work>/parity/probes`).
//!
//! The o2-geom task records there and replays the records here. Until the Clipper and geometry
//! records exist the file holds only orca-eigen's, and this test fails as soon as it holds a
//! record of another kind that nothing replays (added 2026-11-07).

use orca_probe::load_probe;

/// The Eigen kinds of `geom.sop1`, which orca-eigen's `tests/probes.rs` replays.
const EIGEN_KINDS: [&str; 5] = ["EVEC", "EMAT", "ETRF", "EROT", "ESVD"];

#[test]
fn geom_probe_holds_no_record_without_a_replay() {
    assert_eq!(orca_geom::NAME, "orca-geom");
    let file = load_probe("geom").unwrap_or_else(|e| panic!("{e}"));
    let others: Vec<&str> = file
        .records
        .iter()
        .map(|r| r.kind())
        .filter(|k| !EIGEN_KINDS.contains(k))
        .collect();
    assert!(
        others.is_empty(),
        "{}: {} records orca-geom replays none of yet ({:?}...): replay them in tests/probes.rs",
        file.path.display(),
        others.len(),
        &others[..others.len().min(4)]
    );
}
