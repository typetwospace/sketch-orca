// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the walk of Qhull's facets in its_convex_hull of
// src/libslic3r/TriangleMesh.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The probe gate for orca-qhull: the records of `qhull.sop1` (`parity/SOP1.md`, QHUL), from the
//! committed subset (`parity/probes/`), or from the full set when `SKETCH_ORCA_PROBES_DEEP` names
//! its folder (`<work>/parity/probes`).
//!
//! Each record holds a point set and what libslic3r's `its_convex_hull` made of it with the C++
//! engine's Qhull: the vertices, the triangles and the count of their face properties, and the
//! text `~Qhull` wrote to `std::cerr`. The
//! test runs [`orca_qhull::convex_hull`] on the points and walks its facets as `its_convex_hull`
//! does. That walk is `orca-mesh`'s (`its::its_convex_hull`, whose own probe test replays these
//! records too); it is repeated here in a few lines because `orca-mesh` depends on this crate.
//! A run that stops on a branch the translation leaves out (`Error::Unported`) fails the test
//! even where the C++ failed too and the record is empty. Filled by o2-mesh (2026-11-07).

use orca_probe::{Mismatches, Reader, Record, load_probe};

/// One QHUL record.
struct Case {
    label: String,
    points: Vec<[f32; 3]>,
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[i32; 3]>,
    properties: u64,
    cerr: Vec<u8>,
}

fn points(r: &mut Reader<'_>) -> Vec<[f32; 3]> {
    r.vec(|r| Ok([r.f32()?, r.f32()?, r.f32()?]))
        .unwrap_or_else(|e| panic!("{e}"))
}

fn parse(rec: &Record) -> Case {
    let mut r = rec.reader();
    let label = r.str().unwrap_or_else(|e| panic!("{e}"));
    let pts = points(&mut r);
    let vertices = points(&mut r);
    let triangles = r
        .vec(|r| Ok([r.i32()?, r.i32()?, r.i32()?]))
        .unwrap_or_else(|e| panic!("{label}: {e}"));
    let properties = r.u64().unwrap_or_else(|e| panic!("{label}: {e}"));
    let cerr = r
        .bytes()
        .unwrap_or_else(|e| panic!("{label}: {e}"))
        .to_vec();
    r.finish(&label).unwrap_or_else(|e| panic!("{e}"));
    Case {
        label,
        points: pts,
        vertices,
        triangles,
        properties,
        cerr,
    }
}

/// What `its_convex_hull` gives for a point set.
struct Its {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[i32; 3]>,
    /// The text written to `std::cerr`.
    cerr: String,
    /// The error the run stopped on, which `its_convex_hull` catches.
    error: Option<orca_qhull::Error>,
}

/// What `its_convex_hull` returns and writes to `std::cerr`, as `TriangleMesh.cpp` walks Qhull's
/// facets: the vertices numbered as the facets first use them, each facet of three a triangle,
/// turned when `(v1 - v0) x (v2 - v1)` (in `float`) points against Qhull's normal (the dot
/// product in `double`).
fn its_convex_hull(pts: &[[f32; 3]]) -> Its {
    let mut dst_vertices: Vec<[f32; 3]> = Vec::new();
    let mut dst_facets: Vec<[i32; 3]> = Vec::new();
    if pts.is_empty() {
        return Its {
            vertices: dst_vertices,
            triangles: dst_facets,
            cerr: String::new(),
            error: None,
        };
    }
    let src: Vec<[f64; 3]> = pts
        .iter()
        .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
        .collect();
    let hull = match orca_qhull::convex_hull(&src) {
        Ok(hull) => hull,
        Err(error) => {
            return Its {
                vertices: dst_vertices,
                triangles: dst_facets,
                cerr: String::new(),
                error: Some(error),
            };
        }
    };
    let mut map: Vec<i32> = Vec::new();
    for facet in &hull.facets {
        let mut indices = [0i32; 3];
        let mut cnt = 0usize;
        for vertex in &facet.vertices {
            let id = vertex.id as usize;
            if id >= map.len() {
                map.resize((id + 1).next_power_of_two(), -1);
            }
            if map[id] == -1 {
                map[id] = i32::try_from(dst_vertices.len()).expect("fewer than 2^31 vertices");
                let p = src[vertex.point];
                #[allow(clippy::cast_possible_truncation)]
                dst_vertices.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            }
            if cnt < 3 {
                indices[cnt] = map[id];
            }
            let at = cnt;
            cnt += 1;
            if at == 3 {
                break;
            }
        }
        if cnt == 3 {
            let v = |k: usize| dst_vertices[usize::try_from(indices[k]).expect("an index")];
            let a = [v(1)[0] - v(0)[0], v(1)[1] - v(0)[1], v(1)[2] - v(0)[2]];
            let b = [v(2)[0] - v(1)[0], v(2)[1] - v(1)[1], v(2)[2] - v(1)[2]];
            let n = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            let n2 = facet.normal;
            let d = f64::from(n[0]) * n2[0] + f64::from(n[1]) * n2[1] + f64::from(n[2]) * n2[2];
            if d < 0.0 {
                indices.swap(1, 2);
            }
            dst_facets.push(indices);
        }
    }
    let cerr = if hull.message.is_empty() {
        String::new()
    } else {
        format!("\nQhull output at end\n{}", hull.message)
    };
    Its {
        vertices: dst_vertices,
        triangles: dst_facets,
        cerr,
        error: None,
    }
}

fn bits(v: &[[f32; 3]]) -> Vec<[u32; 3]> {
    v.iter()
        .map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
        .collect()
}

#[test]
fn convex_hulls_are_the_engines() {
    let file = load_probe("qhull").unwrap_or_else(|e| panic!("{e}"));
    let mut m = Mismatches::default();
    let mut cases = 0usize;
    for rec in &file.records {
        assert_eq!(
            rec.kind(),
            "QHUL",
            "{}: a record of kind {} that nothing replays",
            file.path.display(),
            rec.kind()
        );
        let case = parse(rec);
        cases += 1;
        let Its {
            vertices,
            triangles,
            cerr,
            error,
        } = its_convex_hull(&case.points);
        let label = &case.label;
        // A branch the translation leaves out stops the run with an error the C++ never raises,
        // which an empty record would hide.
        m.check(
            !matches!(error, Some(orca_qhull::Error::Unported(_))),
            || format!("{label}: {error:?}, a branch the translation leaves out"),
        );
        let (got, want) = (bits(&vertices), bits(&case.vertices));
        m.check(got == want, || {
            let at = got
                .iter()
                .zip(&want)
                .position(|(a, b)| a != b)
                .unwrap_or(Ord::min(got.len(), want.len()));
            format!(
                "{label}: {} vertices here, {} recorded, first difference at vertex {at}",
                got.len(),
                want.len()
            )
        });
        m.check(triangles == case.triangles, || {
            let at = triangles
                .iter()
                .zip(&case.triangles)
                .position(|(a, b)| a != b)
                .unwrap_or(Ord::min(triangles.len(), case.triangles.len()));
            format!(
                "{label}: {} triangles here, {} recorded, first difference at triangle {at}",
                triangles.len(),
                case.triangles.len()
            )
        });
        // indexed_triangle_set's two-argument constructor sizes the face properties as the
        // faces; the `return {}` after a throw leaves both empty.
        m.check(case.properties == triangles.len() as u64, || {
            format!(
                "{label}: {} face properties recorded for {} triangles",
                case.properties,
                triangles.len()
            )
        });
        m.check(cerr.as_bytes() == case.cerr.as_slice(), || {
            format!(
                "{label}: std::cerr {:?} here, {:?} recorded",
                cerr,
                String::from_utf8_lossy(&case.cerr)
            )
        });
    }
    assert!(cases > 0, "{}: no QHUL record", file.path.display());
    m.assert_none(&format!(
        "{} ({} set, {cases} hulls)",
        file.path.display(),
        file.set.name()
    ));
}
