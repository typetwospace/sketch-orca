// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/ExtrusionLine.hpp (287-292) and
// ExtrusionLine.cpp (289-303) of OrcaSlicer v2.4.2, with libslic3r.h's Slic3r::append of two
// vectors, translated on 2026-11-14 (orca-o3-walls-1). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and Ultimaker B.V.,
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `extrusion_paths_append` of variable-width lines: a ClipperLib_Z path (the width in z) or an
//! Arachne line made a thick polyline (`Arachne::to_thick_polyline`), then the paths of
//! `thick_polyline_to_multi_path` with a tolerance of 0.05 mm and a merge tolerance of
//! `SCALED_EPSILON`, appended.
//!
//! The paths are moved in as `Slic3r::append` moves a vector: into an empty list the list itself,
//! else one by one at the end. Each path comes out with `inset_idx` -1, as every path of domain A
//! has it. A flow too narrow throws in the C++ (`Flow::with_width`, `mm3_per_mm`): these return
//! its [`FlowError`], `dst` unchanged by that line. A path of fewer than two points, which the C++
//! reads past, stops in `orca-arachne`'s `to_thick_polyline`.

use orca_arachne::extrusion_line::{ExtrusionLine, to_thick_polyline};
use orca_geom::clipper_z;
use orca_geom::point::{SCALED_EPSILON, scaled_f32};
use orca_layer::flow::{Flow, FlowError};
use orca_layer::path::ExtrusionPaths;
use orca_layer::role::ExtrusionRole;
use orca_layer::variable_width::thick_polyline_to_multi_path;

/// `Slic3r::append(dst, std::move(src))` of two path lists: `src` itself into an empty `dst`, else
/// its paths moved to the end. It belongs in `orca-layer`, whose `cpp_vec` has no move of a whole
/// vector; translated here (walls-1).
pub fn append_paths(dst: &mut ExtrusionPaths, mut src: ExtrusionPaths) {
    if dst.is_empty() {
        *dst = src;
    } else {
        orca_layer::cpp_vec::reserve(dst, dst.len() + src.len());
        for mut p in src.drain(..) {
            // A move construction into the new slot: -1.
            p.inset_idx = -1;
            dst.push(p);
        }
    }
}

/// `extrusion_paths_append(dst, extrusion_paths, role, flow)` of ClipperLib_Z paths.
///
/// # Errors
/// What `thick_polyline_to_multi_path` returns for a flow too narrow; the paths of the lines
/// before it stay appended.
#[allow(clippy::cast_possible_truncation)]
pub fn extrusion_paths_append_zpaths(
    dst: &mut ExtrusionPaths,
    extrusion_paths: &[clipper_z::Path],
    role: ExtrusionRole,
    flow: &Flow,
) -> Result<(), FlowError> {
    for extrusion_path in extrusion_paths {
        let thick_polyline = to_thick_polyline(extrusion_path);
        let multi = thick_polyline_to_multi_path(
            &thick_polyline,
            role,
            flow,
            scaled_f32(0.05),
            SCALED_EPSILON as f32,
        )?;
        append_paths(dst, multi.paths);
    }
    Ok(())
}

/// `extrusion_paths_append(dst, extrusion, role, flow)` of an Arachne line.
///
/// # Errors
/// What `thick_polyline_to_multi_path` returns for a flow too narrow.
#[allow(clippy::cast_possible_truncation)]
pub fn extrusion_paths_append_line(
    dst: &mut ExtrusionPaths,
    extrusion: &ExtrusionLine,
    role: ExtrusionRole,
    flow: &Flow,
) -> Result<(), FlowError> {
    let thick_polyline = to_thick_polyline(&extrusion.junctions);
    let multi = thick_polyline_to_multi_path(
        &thick_polyline,
        role,
        flow,
        scaled_f32(0.05),
        SCALED_EPSILON as f32,
    )?;
    append_paths(dst, multi.paths);
    Ok(())
}
