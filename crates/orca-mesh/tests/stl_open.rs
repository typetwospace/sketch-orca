// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The ASCII STL files the C++ engine cannot read: those whose `solid` line is longer than the
//! 255 bytes admesh's `char solid_name[256]` holds, which `fscanf(fp, " solid %[^\n]",
//! solid_name)` overruns. A file with CR line ends has no `\n`, so its whole text is that line
//! (OrcaSlicer's `20mmbox-CR.stl`). The C++ engine dies there with a stack overrun (exit code 9,
//! 0xC0000409: `parity/CORPUS.md`, "An engine fault: 20mmbox-CR.stl"); orca-mesh refuses the
//! file with `StlError::OverlongSolidLine` before it reads a facet (`parity/HAZARDS.md`,
//! orca-mesh). Added by o2-mesh (2026-11-07).

use orca_mesh::admesh::StlFile;
use orca_mesh::admesh::stlinit::{StlError, stl_open_bytes};
use orca_mesh::triangle_mesh::TriangleMesh;

/// A 20 mm cube as an ASCII STL named `name`, every line ended by `eol`.
fn ascii_cube(name: &str, eol: &str) -> Vec<u8> {
    const FACES: [[[u8; 3]; 3]; 12] = [
        [[0, 0, 0], [0, 1, 0], [1, 1, 0]],
        [[0, 0, 0], [1, 1, 0], [1, 0, 0]],
        [[0, 0, 1], [1, 0, 1], [1, 1, 1]],
        [[0, 0, 1], [1, 1, 1], [0, 1, 1]],
        [[0, 0, 0], [1, 0, 0], [1, 0, 1]],
        [[0, 0, 0], [1, 0, 1], [0, 0, 1]],
        [[0, 1, 0], [0, 1, 1], [1, 1, 1]],
        [[0, 1, 0], [1, 1, 1], [1, 1, 0]],
        [[0, 0, 0], [0, 0, 1], [0, 1, 1]],
        [[0, 0, 0], [0, 1, 1], [0, 1, 0]],
        [[1, 0, 0], [1, 1, 0], [1, 1, 1]],
        [[1, 0, 0], [1, 1, 1], [1, 0, 1]],
    ];
    let mut t = format!("solid {name}{eol}");
    for face in FACES {
        t += &format!("facet normal 0 0 0{eol}  outer loop{eol}");
        for v in face {
            t += &format!(
                "    vertex {} {} {}{eol}",
                u32::from(v[0]) * 20,
                u32::from(v[1]) * 20,
                u32::from(v[2]) * 20
            );
        }
        t += &format!("  endloop{eol}endfacet{eol}");
    }
    t += &format!("endsolid {name}{eol}");
    t.into_bytes()
}

#[test]
fn a_file_with_cr_line_ends_is_refused() {
    let text = ascii_cube("cube", "\r");
    assert!(text.len() > 256 && !text.contains(&b'\n'));
    let mut stl = StlFile::default();
    assert_eq!(
        stl_open_bytes(&mut stl, Some(&text), "cube-cr.stl", 80),
        Err(StlError::OverlongSolidLine { length: 256 })
    );
    let mut mesh = TriangleMesh::default();
    assert_eq!(
        mesh.read_stl_bytes(Some(&text), "cube-cr.stl", true, 80),
        Err(StlError::OverlongSolidLine { length: 256 })
    );
    assert!(mesh.empty(), "a refused file leaves the mesh as it was");
}

#[test]
fn the_same_file_with_lf_or_cr_lf_line_ends_reads() {
    for eol in ["\n", "\r\n"] {
        let text = ascii_cube("cube", eol);
        let mut mesh = TriangleMesh::default();
        mesh.read_stl_bytes(Some(&text), "cube.stl", true, 80)
            .unwrap_or_else(|e| panic!("{eol:?}: {e}"));
        assert_eq!(mesh.facets_count(), 12, "{eol:?}");
        assert_eq!(mesh.stats().number_of_parts, 1, "{eol:?}");
        assert_eq!(mesh.stats().open_edges, 0, "{eol:?}");
    }
}

#[test]
fn a_solid_line_of_255_bytes_reads_and_one_of_256_is_refused() {
    let name = "n".repeat(255);
    let mut mesh = TriangleMesh::default();
    mesh.read_stl_bytes(Some(&ascii_cube(&name, "\n")), "long.stl", true, 80)
        .unwrap_or_else(|e| panic!("255 bytes: {e}"));
    assert_eq!(mesh.facets_count(), 12);
    let name = "n".repeat(256);
    let mut stl = StlFile::default();
    assert_eq!(
        stl_open_bytes(&mut stl, Some(&ascii_cube(&name, "\n")), "longer.stl", 80),
        Err(StlError::OverlongSolidLine { length: 256 })
    );
}
