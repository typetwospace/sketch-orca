// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/SlicingAdaptive.hpp lines 1-42 and
// src/libslic3r/SlicingAdaptive.cpp lines 1-216, of OrcaSlicer v2.4.2, translated on 2026-11-14
// by orca-o3-slice-2. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the
// PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci), based on
// the work of Florens Wasserfall (@platsch), under the GNU Affero General Public License, version
// 3 (AGPL-3.0; LICENSE).

//! Adaptive slicing (`SlicingAdaptive`), based on the work of Florens Wasserfall and his paper
//! "Adaptive Slicing for the FDM Process Revisited" (CASE-2017): the faces of an object by the
//! height they span, and the layer height their slope allows for a quality between 0 (the minimum
//! layer height) and 1 (the maximum).
//!
//! - [`SlicingAdaptive::prepare`] takes the object's model parts (`raw_mesh`) through its first
//!   instance's matrix, and keeps for each face its z span (`std::min` and `std::max` of the three
//!   `z`), the absolute `z` of its unit normal (the cosine to the Z axis, `fabsf`) and the length of
//!   its `x` and `y` (the sine, `sqrtf` of `float` squares); the faces are then sorted by z span
//!   (orca-stl's sort, the C++'s introsort, with the `std::pair` order written out).
//! - [`SlicingAdaptive::next_layer_height`] mixes `float` and `double` as the C++ does: the surface
//!   deviation is the `double` `lerp` of the layer heights narrowed to `float`, the touching test
//!   `zspan.second < print_z + EPSILON` is made in `double`, and a face's height from its slope is
//!   `min(deviation / 0.184f, float(1.44 * deviation * sqrtf(sin / cos)))` (Vojtech's error
//!   triangle with the clamp at 90 degrees; `FLT_MAX` where the cosine is not above `1e-5`).
//! - [`SlicingAdaptive::horizontal_facet_distance`] is the distance to the next horizontal face
//!   above `z` within one maximum layer height, else to the object's top.
//!
//! The C++'s `ADAPTIVE_LAYER_HEIGHT_DEBUG` trace records are only compiled without `NDEBUG`, so the
//! release engine writes none, and neither does this port; its assert is left out too. An object
//! without an instance stops with [`OBJECT_WITHOUT_INSTANCE`] where the C++ dereferences
//! `instances.front()` of an empty vector.

use orca_crt::{fabsf, sqrtf};
use orca_geom::point::EPSILON;
use orca_index::stop::not_ported;
use orca_mesh::its::face_normal_normalized;
use orca_model::model::ModelObject;
use orca_stl::{max, min, sort_by};

use crate::slicing::SlicingParameters;
use crate::slicing_layers::lerp;

/// The stop of [`SlicingAdaptive::prepare`] on an object with no instance: the C++ reads
/// `*object.instances.front()` of an empty vector.
pub const OBJECT_WITHOUT_INSTANCE: &str = "object-without-instance";

/// `SlicingAdaptive::FaceZ`: a face's z span and the cosine and sine of its normal to the Z axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceZ {
    /// `z_span`: the lowest and highest `z` of the face.
    pub z_span: (f32, f32),
    /// `n_cos`: cosine of the normal vector towards the Z axis.
    pub n_cos: f32,
    /// `n_sin`: sine of the normal vector towards the Z axis.
    pub n_sin: f32,
}

/// `std::pair<float, float>::operator<`: `x.first < y.first || (!(y.first < x.first) && x.second <
/// y.second)`, written out so that a NaN orders as it does in the C++ (hence the negated
/// comparison).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn pair_less(x: (f32, f32), y: (f32, f32)) -> bool {
    x.0 < y.0 || (!(y.0 < x.0) && x.1 < y.1)
}

/// `layer_height_from_slope(face, max_surface_deviation)`: for a given facet, the maximum height
/// within the allowed surface roughness / stairstepping deviation. Constant error measured as an
/// area of the surface error triangle, Vojtech's formula with clamping to roughness at 90
/// degrees.
fn layer_height_from_slope(face: &FaceZ, max_surface_deviation: f32) -> f32 {
    min(
        max_surface_deviation / 0.184_f32,
        if f64::from(face.n_cos) > 1e-5 {
            (1.44 * f64::from(max_surface_deviation) * f64::from(sqrtf(face.n_sin / face.n_cos)))
                as f32
        } else {
            f32::MAX
        },
    )
}

/// `SlicingAdaptive`: the slicing parameters and the faces of an object sorted by their z span.
#[derive(Clone, Debug, Default)]
pub struct SlicingAdaptive {
    /// `m_slicing_params`.
    m_slicing_params: SlicingParameters,
    /// `m_faces`.
    m_faces: Vec<FaceZ>,
}

impl SlicingAdaptive {
    /// `clear()`: the faces dropped.
    pub fn clear(&mut self) {
        self.m_faces.clear();
    }

    /// `set_slicing_parameters(params)`.
    pub fn set_slicing_parameters(&mut self, params: SlicingParameters) {
        self.m_slicing_params = params;
    }

    /// The faces, sorted by z span (the C++'s protected `m_faces`, which the probe reads).
    #[must_use]
    pub fn faces(&self) -> &[FaceZ] {
        &self.m_faces
    }

    /// `prepare(object)`: the faces of the object's model parts through its first instance's
    /// matrix, sorted lexicographically by their z span.
    ///
    /// # Panics
    ///
    /// With [`NotPorted`](orca_index::NotPorted) [`OBJECT_WITHOUT_INSTANCE`] for an object
    /// without an instance.
    pub fn prepare(&mut self, object: &ModelObject) {
        self.clear();

        let mut mesh = object.raw_mesh();
        let Some(first_instance) = object.instances.first() else {
            not_ported("SlicingAdaptive::prepare", OBJECT_WITHOUT_INSTANCE);
        };
        mesh.transform(first_instance.get_matrix(), first_instance.is_left_handed());

        // 1) Collect faces from mesh.
        self.m_faces.reserve(mesh.facets_count());
        for face in &mesh.its.indices {
            let vertex = [
                mesh.its.vertices[face[0] as usize],
                mesh.its.vertices[face[1] as usize],
                mesh.its.vertices[face[2] as usize],
            ];
            let n = face_normal_normalized(&vertex);
            let face_z_span = (
                min(min(vertex[0].z(), vertex[1].z()), vertex[2].z()),
                max(max(vertex[0].z(), vertex[1].z()), vertex[2].z()),
            );
            self.m_faces.push(FaceZ {
                z_span: face_z_span,
                n_cos: fabsf(n.z()),
                n_sin: sqrtf(n.x() * n.x() + n.y() * n.y()),
            });
        }

        // 2) Sort faces lexicographically by their Z span.
        sort_by(&mut self.m_faces, |f1, f2| pair_less(f1.z_span, f2.z_span));
    }

    /// `next_layer_height(print_z, quality_factor, current_facet)`: the height of the next layer
    /// starting from the last `print_z` (the top print surface of the previous layer), using a
    /// quality measure in range from 0 to 1 (0: highest quality at low layer heights, 1: lowest
    /// print quality at high layer heights). `current_facet` is an in/out parameter: it remembers
    /// the index of the last face visited, where this function will start from.
    #[must_use]
    pub fn next_layer_height(
        &self,
        print_z: f32,
        quality_factor: f32,
        current_facet: &mut usize,
    ) -> f32 {
        let sp = &self.m_slicing_params;
        let mut height = sp.max_layer_height as f32;

        let max_surface_deviation: f32 = {
            // Vojtech's formula for triangle area error metric.
            let delta_min = sp.min_layer_height;
            let delta_mid = sp.layer_height;
            let delta_max = sp.max_layer_height;
            (if quality_factor < 0.5_f32 {
                lerp(delta_min, delta_mid, 2. * f64::from(quality_factor))
            } else {
                lerp(delta_max, delta_mid, 2. * (1. - f64::from(quality_factor)))
            }) as f32
        };

        // find all facets intersecting the slice-layer
        let mut ordered_id = *current_facet;
        {
            let mut first_hit = false;
            while ordered_id < self.m_faces.len() {
                let zspan = self.m_faces[ordered_id].z_span;
                // facet's minimum is higher than slice_z -> end loop
                if zspan.0 >= print_z {
                    break;
                }
                // facet's maximum is higher than slice_z -> store the first event for next
                // cusp_height call to begin at this point
                if zspan.1 > print_z {
                    // first event?
                    if !first_hit {
                        first_hit = true;
                        *current_facet = ordered_id;
                    }
                    // skip touching facets which could otherwise cause small cusp values
                    if f64::from(zspan.1) < f64::from(print_z) + EPSILON {
                        ordered_id += 1;
                        continue;
                    }
                    // compute cusp-height for this facet and store minimum of all heights
                    height = min(
                        height,
                        layer_height_from_slope(&self.m_faces[ordered_id], max_surface_deviation),
                    );
                }
                ordered_id += 1;
            }
        }

        // lower height limit due to printer capabilities
        height = max(height, sp.min_layer_height as f32);

        // check for sloped facets inside the determined layer and correct height if necessary
        if height > sp.min_layer_height as f32 {
            while ordered_id < self.m_faces.len() {
                let zspan = self.m_faces[ordered_id].z_span;
                // facet's minimum is higher than slice_z + height -> end loop
                if zspan.0 >= print_z + height {
                    break;
                }

                // skip touching facets which could otherwise cause small cusp values
                if f64::from(zspan.1) < f64::from(print_z) + EPSILON {
                    ordered_id += 1;
                    continue;
                }

                // Compute cusp-height for this facet and check against height.
                let reduced_height =
                    layer_height_from_slope(&self.m_faces[ordered_id], max_surface_deviation);

                let z_diff = zspan.0 - print_z;
                if reduced_height < z_diff {
                    // The currently visited triangle's slope limits the next layer height so much,
                    // that the lowest point of the currently visible triangle is already above the
                    // newly proposed layer height. This means, that we need to limit the layer
                    // height so that the offending newly visited triangle is just above of the new
                    // layer.
                    height = z_diff;
                } else if reduced_height < height {
                    height = reduced_height;
                }
                ordered_id += 1;
            }
            // lower height limit due to printer capabilities again
            height = max(height, sp.min_layer_height as f32);
        }

        height
    }

    /// `horizontal_facet_distance(z)`: the distance to the next horizontal facet in Z-dir, to
    /// consider horizontal object features in slice thickness.
    #[must_use]
    pub fn horizontal_facet_distance(&self, z: f32) -> f32 {
        let sp = &self.m_slicing_params;
        for face in &self.m_faces {
            let zspan = face.z_span;
            // facet's minimum is higher than max forward distance -> end loop
            if f64::from(zspan.0) > f64::from(z) + sp.max_layer_height {
                break;
            }
            // min_z == max_z -> horizontal facet
            if zspan.0 > z && zspan.0 == zspan.1 {
                return zspan.0 - z;
            }
        }

        // objects maximum?
        if z + sp.max_layer_height as f32 > sp.object_print_z_height() as f32 {
            max(sp.object_print_z_height() as f32 - z, 0.0_f32)
        } else {
            sp.max_layer_height as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_eigen::Vector;
    use orca_mesh::admesh::IndexedTriangleSet;
    use orca_mesh::triangle_mesh::TriangleMesh;
    use orca_model::model::ModelVolumeType;

    /// An object of one pyramid with a square base of `w` and a height of `h`, its apex up, one
    /// instance at identity.
    fn pyramid(w: f32, h: f32) -> ModelObject {
        let v = |x: f32, y: f32, z: f32| Vector([x, y, z]);
        let f = |a: i32, b: i32, c: i32| Vector([a, b, c]);
        let its = IndexedTriangleSet::new(
            vec![
                f(0, 2, 1),
                f(0, 3, 2),
                f(0, 1, 4),
                f(1, 2, 4),
                f(2, 3, 4),
                f(3, 0, 4),
            ],
            vec![
                v(0.0, 0.0, 0.0),
                v(w, 0.0, 0.0),
                v(w, w, 0.0),
                v(0.0, w, 0.0),
                v(w / 2.0, w / 2.0, h),
            ],
        );
        let mut o = ModelObject::default();
        o.add_volume(
            TriangleMesh::from_its(its),
            ModelVolumeType::ModelPart,
            false,
        );
        o.add_instance();
        o
    }

    fn params(top: f64) -> SlicingParameters {
        SlicingParameters {
            layer_height: 0.2,
            min_layer_height: 0.07,
            max_layer_height: 0.3,
            first_object_layer_height: 0.2,
            object_print_z_max: top,
            object_print_z_uncompensated_max: top,
            ..SlicingParameters::default()
        }
    }

    #[test]
    fn the_faces_are_sorted_by_their_span() {
        let mut a = SlicingAdaptive::default();
        a.set_slicing_parameters(params(10.0));
        a.prepare(&pyramid(10.0, 10.0));
        let faces = a.faces();
        assert_eq!(faces.len(), 6);
        // The two base faces are horizontal and first.
        assert_eq!(faces[0].z_span, (0.0, 0.0));
        assert_eq!(faces[1].z_span, (0.0, 0.0));
        assert_eq!((faces[0].n_cos, faces[0].n_sin), (1.0, 0.0));
        assert!(
            faces
                .windows(2)
                .all(|w| !pair_less(w[1].z_span, w[0].z_span))
        );
        // The sides lean at atan(2) from the horizontal plane.
        assert!((faces[2].n_cos - 1.0 / 5.0_f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn steep_walls_allow_thick_layers_at_low_quality() {
        let mut a = SlicingAdaptive::default();
        a.set_slicing_parameters(params(10.0));
        // Sides at atan(2): sqrt(sin / cos) is sqrt(2).
        a.prepare(&pyramid(10.0, 10.0));
        let mut current = 0;
        // Quality 1 gives the maximum deviation (0.3): 1.44 * 0.3 * sqrt(2) is above the maximum
        // height, which is kept. The first face that spans the layer is the first side.
        let h = a.next_layer_height(1.0, 1.0, &mut current);
        assert_eq!(h, 0.3);
        assert_eq!(current, 2);
        // Quality 0 gives the minimum deviation (0.07): 1.44 * 0.07 * sqrt(2), about 0.1426.
        let mut current = 0;
        let h0 = a.next_layer_height(1.0, 0.0, &mut current);
        assert!(h0 < h && h0 > 0.07, "{h0}");
        assert!((h0 - 0.1426).abs() < 1e-3, "{h0}");
        // A flat pyramid limits the height more than a steep one.
        let mut flat = SlicingAdaptive::default();
        flat.set_slicing_parameters(params(1.0));
        flat.prepare(&pyramid(10.0, 1.0));
        let mut current = 0;
        assert!(flat.next_layer_height(0.5, 0.5, &mut current) <= h);
    }

    #[test]
    fn the_next_horizontal_face_or_the_top() {
        let mut a = SlicingAdaptive::default();
        a.set_slicing_parameters(params(10.0));
        a.prepare(&pyramid(10.0, 10.0));
        // The base is at 0: from just below it, its distance.
        assert_eq!(a.horizontal_facet_distance(-0.1), 0.1);
        // Mid-height: no horizontal face ahead, the maximum layer height.
        assert_eq!(a.horizontal_facet_distance(5.0), 0.3_f64 as f32);
        // Near the top: the distance to it.
        assert_eq!(a.horizontal_facet_distance(9.9), 10.0_f32 - 9.9);
    }

    #[test]
    fn an_object_without_an_instance_stops() {
        let mut o = pyramid(1.0, 1.0);
        o.instances.clear();
        let caught = std::panic::catch_unwind(move || {
            let mut a = SlicingAdaptive::default();
            a.prepare(&o);
        });
        let stop = caught
            .unwrap_err()
            .downcast::<orca_index::NotPorted>()
            .unwrap();
        assert_eq!(stop.condition, OBJECT_WITHOUT_INSTANCE);
    }
}
