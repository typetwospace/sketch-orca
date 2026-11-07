// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-xml: the records of `xml.sop1` (`parity/SOP1.md`, "Probe files").
//!
//! The probe registry (`bun run engine parity probe --only <name>`) writes the file; until the
//! o2-model task records there, it is a stub and this test has nothing to replay.

#[test]
fn xml_probe_has_nothing_to_replay_yet() {
    assert_eq!(orca_xml::NAME, "orca-xml");
}
