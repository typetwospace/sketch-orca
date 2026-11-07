// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-exact: the exact-arithmetic records of `arrange.sop1`; orca-arrange
//! replays the rest (`parity/SOP1.md`, "Probe files"), from the committed subset
//! (`parity/probes/`), or from the full set when `SKETCH_ORCA_PROBES_DEEP` names its folder
//! (`<work>/parity/probes`).
//!
//! The o2-arrange task records there and replays the records here. Until then the file is a stub
//! (HEAD and PRUN only), and this test fails as soon as it holds a record nothing replays (added
//! 2026-11-07).

use orca_probe::load_probe;

#[test]
fn arrange_probe_holds_no_record_without_a_replay() {
    assert_eq!(orca_exact::NAME, "orca-exact");
    let file = load_probe("arrange").unwrap_or_else(|e| panic!("{e}"));
    assert!(
        file.records.is_empty(),
        "{}: {} records, and orca-exact replays none yet: replay them in tests/probes.rs",
        file.path.display(),
        file.records.len()
    );
}
