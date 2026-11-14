// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/PerimeterGenerator.hpp (1-11, 48-173) and
// PerimeterGenerator.cpp (1-672, 1086-1143, and 2537-2547, PerimeterGeneratorLoop's
// is_internal_contour) of OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-walls-1).
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors
// (Prusa Research) and the Slic3r authors, under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The perimeter generator's state and the helpers its two processes share: the classic loops'
//! tree ([`PerimeterGeneratorLoop`]) and its walk ([`traverse_loops`]), the Arachne lines' walk
//! ([`traverse_extrusions`]) with its width-keeping clip ([`clip_extrusion`]), the steep overhang
//! test ([`detect_steep_overhang`]), the loops' reorientation ([`reorient_perimeters`]), the top
//! surface split ([`PerimeterGenerator::split_top_surfaces`]) and the extra perimeters on
//! overhangs ([`PerimeterGenerator::apply_extra_perimeters`]). `process_classic`,
//! `process_no_bridge` and `generate_lower_polygons_series` are walls-2's; `process_arachne` and
//! `add_infill_contour_for_arachne` walls-3's.
//!
//! The C++ generator holds pointers to its inputs and outputs. Here the inputs are references
//! (`Option` where the C++ pointer may be null), and the outputs (`loops`, `gap_fill`,
//! `fill_surfaces`, `fill_no_overlap`) are owned: the caller moves its collections in through
//! [`PerimeterGenerator::new`] and takes them back after the process. `compatible_regions` are
//! [`CompatibleRegion`]s, each a layer region with its print region's configuration, since a
//! layer region holds its print region as an index. `PerimeterGeneratorArachneExtrusion` holds its
//! line as an index into the caller's list of lines, which [`traverse_extrusions`] takes.
//!
//! [`traverse_loops`] walks the loops of one depth: each loop's polygon (after the fuzzy skin)
//! becomes one path, or, with overhang detection on above the raft, its part over the grown lower
//! layer as walls and the rest as overhang walls, chained from the first point; the thin walls
//! join the first depth; the loops are chained from the origin and each placed after its children
//! (a contour) or before them (a hole), turned to the wall direction. [`traverse_extrusions`] does
//! the same for Arachne lines, the overhang split done on the widths by [`clip_extrusion`], whose
//! ClipperLib_Z fill interpolates the width (z) at each crossing; an open line's start is the end
//! seen once, preferably not an overhang. The C++ quirks are kept: the fill measures the
//! crossing's distance in x, y and z (the crossing's z is 0 then), and a point left without a z is
//! given the width of the subject at its projection, unless the subject has two points or fewer.
//!
//! A branch the C++ reaches only with undefined behaviour stops with
//! [`NotPorted`](crate::stop::NotPorted): a loop whose overhang paths are all empty (the C++ then
//! casts a thin wall to a loop), no lower slices where overhangs are tested, an empty lower
//! polygon series, a generator without islands' perimeters for the extra perimeters.

use orca_arachne::extrusion_line::ExtrusionLine;
use orca_config::print_config::{
    PrintConfig, PrintObjectConfig, PrintRegionConfig, WallDirection, WallSequence,
};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT,
    clip_clipper_polygon_with_subject_bbox_points, clip_clipper_polygons_with_subject_bbox,
    clip_clipper_polygons_with_subject_bbox_expolygons, diff_ex_expolygons,
    diff_ex_expolygons_polygons, diff_pl, diff_pl_polygons, intersection_ex_expolygons,
    intersection_pl_polygons, offset, offset_ex_expolygons, union_ex_two,
};
use orca_geom::clipper_z::{self, ClipType, IntPoint, PolyFillType, PolyTree, PolyType};
use orca_geom::expolygon::{ExPolygon, ExPolygons, get_extents_ex};
use orca_geom::line::Line;
use orca_geom::point::{EPSILON, Point, Points, SCALED_EPSILON, scale_, scaled_f64, unscale_};
use orca_geom::polygon::{Polygon, Polygons};
use orca_geom::polyline::{Polyline, Polylines, ThickPolylines, get_extents_polylines};
use orca_geom::polyline3::Polyline3;
use orca_layer::chain::{chain_and_reorder_extrusion_paths, chain_extrusion_entities};
use orca_layer::collection::ExtrusionEntityCollection;
use orca_layer::cpp_vec;
use orca_layer::entity::{ExtrusionEntity, extrusion_paths_append};
use orca_layer::extrusion_loop::ExtrusionLoop;
use orca_layer::flow::{Flow, FlowRole};
use orca_layer::layer::LayerRegion;
use orca_layer::multi_path::ExtrusionMultiPath;
use orca_layer::path::{ExtrusionPath, ExtrusionPaths};
use orca_layer::role::{ExtrusionLoopRole, ExtrusionRole};
use orca_layer::surface_collection::SurfaceCollection;
use orca_layer::variable_width::variable_width;
use orca_stl::UnorderedMap;
use orca_voronoi::point_hash;

use crate::extra_perimeters::{BRIDGE_INFILL_MARGIN, generate_extra_perimeters_over_overhangs};
use crate::extrusion_paths::{extrusion_paths_append_line, extrusion_paths_append_zpaths};
use crate::fuzzy_skin::{FuzzySkinConfig, apply_fuzzy_skin, apply_fuzzy_skin_line};
use crate::stop::{EMPTY_LIST, PerimeterError, not_ported};

/// `overhang_sampling_number`.
pub const OVERHANG_SAMPLING_NUMBER: i32 = 6;
/// `narrow_loop_length_threshold`.
pub const NARROW_LOOP_LENGTH_THRESHOLD: f64 = 10.;
/// `SMALLER_EXT_INSET_OVERLAP_TOLERANCE` (BBS): an island narrower than the external wall's width
/// plus its spacing times `1 - 0.22` is a small detail, walled with a narrower line.
pub const SMALLER_EXT_INSET_OVERLAP_TOLERANCE: f64 = 0.22;

/// The condition of a loop entity the chain gives where the C++ casts another entity to a loop.
const NOT_A_LOOP: &str = "a thin wall where a loop was expected (a loop of no overhang paths)";
/// The condition of overhang detection without lower slices, which the C++ dereferences.
const NO_LOWER_SLICES: &str = "no lower slices where overhangs are detected";
/// The condition of `split_top_surfaces` without upper slices, which the C++ dereferences.
const NO_UPPER_SLICES: &str = "no upper slices where the top surfaces are split";
/// The condition of `apply_extra_perimeters` whose last perimeters are not a collection.
const NO_ISLAND_PERIMETERS: &str = "no islands' perimeters collection to add the extra ones to";

/// `PerimeterGeneratorLoop`: one classic loop and the loops inside it.
#[derive(Clone, Debug)]
pub struct PerimeterGeneratorLoop {
    /// `polygon`.
    pub polygon: Polygon,
    /// `is_contour`: a contour, else a hole.
    pub is_contour: bool,
    /// `is_smaller_width_perimeter` (BBS): walled with the narrower external line.
    pub is_smaller_width_perimeter: bool,
    /// `depth`: 0 the external wall.
    pub depth: u16,
    /// `children`: contours or holes.
    pub children: Vec<PerimeterGeneratorLoop>,
}

/// `PerimeterGeneratorLoops`.
pub type PerimeterGeneratorLoops = Vec<PerimeterGeneratorLoop>;

impl PerimeterGeneratorLoop {
    /// `PerimeterGeneratorLoop(polygon, depth, is_contour, is_small_width_perimeter)`.
    #[must_use]
    pub fn new(
        polygon: &Polygon,
        depth: u16,
        is_contour: bool,
        is_small_width_perimeter: bool,
    ) -> Self {
        Self {
            polygon: polygon.clone(),
            is_contour,
            is_smaller_width_perimeter: is_small_width_perimeter,
            depth,
            children: Vec::new(),
        }
    }

    /// `is_external()`: depth 0.
    #[must_use]
    pub fn is_external(&self) -> bool {
        self.depth == 0
    }

    /// `is_internal_contour()` (PerimeterGenerator.cpp 2537-2547): a contour with no contour among
    /// its children (an island which may have holes but no inner island).
    #[must_use]
    pub fn is_internal_contour(&self) -> bool {
        // An internal contour is a contour containing no other contours
        if !self.is_contour {
            return false;
        }
        for loop_ in &self.children {
            if loop_.is_contour {
                return false;
            }
        }
        true
    }
}

/// An entry of `compatible_regions` (`LayerRegionPtrs`): the layer region and its print region's
/// configuration (`region->region().config()`).
#[derive(Clone, Copy, Debug)]
pub struct CompatibleRegion<'a> {
    /// The layer region.
    pub layer_region: &'a LayerRegion,
    /// Its print region's configuration.
    pub config: &'a PrintRegionConfig,
}

/// `PerimeterGenerator`.
#[derive(Debug)]
pub struct PerimeterGenerator<'a> {
    /// `slices`.
    pub slices: &'a SurfaceCollection,
    /// `compatible_regions`.
    pub compatible_regions: &'a [CompatibleRegion<'a>],
    /// `upper_slices`.
    pub upper_slices: Option<&'a [ExPolygon]>,
    /// `upper_slices_same_region`.
    pub upper_slices_same_region: Option<&'a SurfaceCollection>,
    /// `lower_slices`.
    pub lower_slices: Option<&'a [ExPolygon]>,
    /// `layer_height`.
    pub layer_height: f64,
    /// `layer_id`: -1 until set.
    pub layer_id: i32,
    /// `slice_z`.
    pub slice_z: f64,
    /// `perimeter_flow`.
    pub perimeter_flow: Flow,
    /// `ext_perimeter_flow`.
    pub ext_perimeter_flow: Flow,
    /// `overhang_flow`.
    pub overhang_flow: Flow,
    /// `solid_infill_flow`.
    pub solid_infill_flow: Flow,
    /// `config`.
    pub config: &'a PrintRegionConfig,
    /// `object_config`.
    pub object_config: &'a PrintObjectConfig,
    /// `print_config`.
    pub print_config: &'a PrintConfig,
    /// `loops`: the loops with the external thin walls (owned; see the module header).
    pub loops: ExtrusionEntityCollection,
    /// `gap_fill`: the gaps without the thin walls.
    pub gap_fill: ExtrusionEntityCollection,
    /// `fill_surfaces`: the infill without the gap fills.
    pub fill_surfaces: SurfaceCollection,
    /// `fill_no_overlap` (BBS).
    pub fill_no_overlap: ExPolygons,
    /// `smaller_ext_perimeter_flow` (BBS).
    pub smaller_ext_perimeter_flow: Flow,
    /// `m_lower_polygons_series` (BBS).
    pub m_lower_polygons_series: Vec<Polygons>,
    /// `m_external_lower_polygons_series` (BBS).
    pub m_external_lower_polygons_series: Vec<Polygons>,
    /// `m_smaller_external_lower_polygons_series` (BBS).
    pub m_smaller_external_lower_polygons_series: Vec<Polygons>,
    /// `has_fuzzy_skin`.
    pub has_fuzzy_skin: bool,
    /// `has_fuzzy_hole`.
    pub has_fuzzy_hole: bool,
    /// `regions_by_fuzzify`: in construction order, so that overlaps are decided the same way.
    pub regions_by_fuzzify: Vec<(FuzzySkinConfig, ExPolygons)>,
    /// `m_spiral_vase`.
    pub(crate) m_spiral_vase: bool,
    /// `m_scaled_resolution`.
    pub(crate) m_scaled_resolution: f64,
    /// `m_model_rotation_rad`.
    pub(crate) m_model_rotation_rad: f64,
    /// `m_ext_mm3_per_mm`.
    pub(crate) m_ext_mm3_per_mm: f64,
    /// `m_mm3_per_mm`.
    pub(crate) m_mm3_per_mm: f64,
    /// `m_mm3_per_mm_overhang`.
    pub(crate) m_mm3_per_mm_overhang: f64,
    /// `m_ext_mm3_per_mm_smaller_width` (BBS).
    pub(crate) m_ext_mm3_per_mm_smaller_width: f64,
    /// `m_lower_slices_polygons`.
    pub(crate) m_lower_slices_polygons: Polygons,
}

impl<'a> PerimeterGenerator<'a> {
    /// `PerimeterGenerator(slices, compatible_regions, layer_height, slice_z, flow, config,
    /// object_config, print_config, spiral_mode, model_rotation_rad, loops, gap_fill,
    /// fill_surfaces, fill_no_overlap)`: every flow `flow`, no upper or lower slices, layer -1, the
    /// resolution scaled (at least `EPSILON`), the volumes per millimetre -1. The four outputs are
    /// moved in.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slices: &'a SurfaceCollection,
        compatible_regions: &'a [CompatibleRegion<'a>],
        layer_height: f64,
        slice_z: f64,
        flow: Flow,
        config: &'a PrintRegionConfig,
        object_config: &'a PrintObjectConfig,
        print_config: &'a PrintConfig,
        spiral_mode: bool,
        model_rotation_rad: f64,
        loops: ExtrusionEntityCollection,
        gap_fill: ExtrusionEntityCollection,
        fill_surfaces: SurfaceCollection,
        fill_no_overlap: ExPolygons,
    ) -> Self {
        Self {
            slices,
            compatible_regions,
            upper_slices: None,
            upper_slices_same_region: None,
            lower_slices: None,
            layer_height,
            layer_id: -1,
            slice_z,
            perimeter_flow: flow,
            ext_perimeter_flow: flow,
            overhang_flow: flow,
            solid_infill_flow: flow,
            config,
            object_config,
            print_config,
            loops,
            gap_fill,
            fill_surfaces,
            fill_no_overlap,
            smaller_ext_perimeter_flow: Flow::default(),
            m_lower_polygons_series: Vec::new(),
            m_external_lower_polygons_series: Vec::new(),
            m_smaller_external_lower_polygons_series: Vec::new(),
            has_fuzzy_skin: false,
            has_fuzzy_hole: false,
            regions_by_fuzzify: Vec::new(),
            m_spiral_vase: spiral_mode,
            m_scaled_resolution: scaled_f64(if print_config.resolution.value > EPSILON {
                print_config.resolution.value
            } else {
                EPSILON
            }),
            m_model_rotation_rad: model_rotation_rad,
            m_ext_mm3_per_mm: -1.,
            m_mm3_per_mm: -1.,
            m_mm3_per_mm_overhang: -1.,
            m_ext_mm3_per_mm_smaller_width: -1.,
            m_lower_slices_polygons: Polygons::new(),
        }
    }

    /// `ext_mm3_per_mm()`.
    #[must_use]
    pub fn ext_mm3_per_mm(&self) -> f64 {
        self.m_ext_mm3_per_mm
    }

    /// `mm3_per_mm()`.
    #[must_use]
    pub fn mm3_per_mm(&self) -> f64 {
        self.m_mm3_per_mm
    }

    /// `mm3_per_mm_overhang()`.
    #[must_use]
    pub fn mm3_per_mm_overhang(&self) -> f64 {
        self.m_mm3_per_mm_overhang
    }

    /// `smaller_width_ext_mm3_per_mm()` (BBS).
    #[must_use]
    pub fn smaller_width_ext_mm3_per_mm(&self) -> f64 {
        self.m_ext_mm3_per_mm_smaller_width
    }

    /// `lower_slices_polygons()`.
    #[must_use]
    pub fn lower_slices_polygons(&self) -> &[Polygon] {
        &self.m_lower_slices_polygons
    }

    /// `m_spiral_vase`.
    #[must_use]
    pub fn spiral_vase(&self) -> bool {
        self.m_spiral_vase
    }

    /// `m_scaled_resolution`.
    #[must_use]
    pub fn scaled_resolution(&self) -> f64 {
        self.m_scaled_resolution
    }

    /// `m_model_rotation_rad`.
    #[must_use]
    pub fn model_rotation_rad(&self) -> f64 {
        self.m_model_rotation_rad
    }

    /// `split_top_surfaces(orig_polygons, top_fills, non_top_polygons, fill_clip)`: the top
    /// surfaces of `orig_polygons` (off the upper slices grown by the minimum top width, and not
    /// over a bridge) united into `top_fills`, `fill_clip` set to the infill's clip, and the rest
    /// returned (the C++'s `non_top_polygons`, which its caller passes as `orig_polygons` too; every
    /// read of `orig_polygons` comes before that write, so returning it is the same).
    ///
    /// # Panics
    /// With [`NotPorted`](crate::stop::NotPorted) without upper slices (or the same region's with
    /// `interface_shells`), which the C++ dereferences.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    pub fn split_top_surfaces(
        &self,
        orig_polygons: &[ExPolygon],
        top_fills: &mut ExPolygons,
        fill_clip: &mut ExPolygons,
    ) -> ExPolygons {
        // other perimeters
        let perimeter_width = self.perimeter_flow.scaled_width();
        let perimeter_spacing = self.perimeter_flow.scaled_spacing();

        // external perimeters
        let ext_perimeter_width = self.ext_perimeter_flow.scaled_width();
        let ext_perimeter_spacing = self.ext_perimeter_flow.scaled_spacing();

        let has_gap_fill = self.config.gap_infill_speed.value > 0.;

        // split the polygons with top/not_top; get the offset from solid surface anchor
        let wall_loops = self.config.wall_loops.value;
        let mut offset_top_surface = orca_crt::f64_to_i64(scale_(
            0.9 * (if wall_loops == 0 {
                0.
            } else {
                unscale_(
                    ext_perimeter_width
                        .wrapping_add(perimeter_spacing.wrapping_mul(i64::from(wall_loops - 1)))
                        as f64,
                )
            }),
        ));
        // if possible, try to not push the extra perimeters inside the sparse infill
        let inner = if wall_loops <= 1 {
            0.
        } else {
            perimeter_spacing.wrapping_mul(i64::from(wall_loops - 1)) as f64
        };
        if offset_top_surface as f64 > 0.9 * inner {
            offset_top_surface -= orca_crt::f64_to_i64(0.9 * inner);
        } else {
            offset_top_surface = 0;
        }
        // don't takes into account too thin areas; get boungding box of last
        let mut last_box = get_extents_ex(orig_polygons);
        last_box.offset(SCALED_EPSILON);

        // skip if the exposed area is smaller than "min_width_top_surface"
        let min_width_top_surface = orca_stl::max(
            ext_perimeter_spacing as f64 / 2. + 10.,
            scale_(
                self.config
                    .min_width_top_surface
                    .get_abs_value(unscale_(perimeter_width as f64)),
            ),
        );

        // get the Polygons upper the polygon this layer
        let mut upper_polygons_series_clipped = if self.object_config.interface_shells.value {
            let Some(same_region) = self.upper_slices_same_region else {
                not_ported("PerimeterGenerator::split_top_surfaces", NO_UPPER_SLICES)
            };
            let upper_slicer_same_region =
                orca_layer::surface::to_expolygons(&same_region.surfaces);
            clip_clipper_polygons_with_subject_bbox_expolygons(
                &upper_slicer_same_region,
                &last_box,
                false,
            )
        } else {
            let Some(upper) = self.upper_slices else {
                not_ported("PerimeterGenerator::split_top_surfaces", NO_UPPER_SLICES)
            };
            clip_clipper_polygons_with_subject_bbox_expolygons(upper, &last_box, false)
        };

        upper_polygons_series_clipped = offset(
            &upper_polygons_series_clipped,
            min_width_top_surface as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        // set the clip to a virtual "second perimeter"
        *fill_clip = offset_ex_expolygons(
            orig_polygons,
            -(ext_perimeter_spacing as f64) as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );
        // get the real top surface
        let mut bridge_checker = ExPolygons::new();
        // `get_at(int - 1)`: filament 0 gives -1, a huge `size_t`, so the first nozzle.
        let nozzle_diameter = *self
            .print_config
            .nozzle_diameter
            .get_at(self.config.outer_wall_filament_id.value.wrapping_sub(1) as isize as usize);
        // Check whether surface be bridge or not
        if let Some(lower) = self.lower_slices {
            // BBS: get the Polygons below the polygon this layer
            let lower_polygons_series_clipped =
                clip_clipper_polygons_with_subject_bbox_expolygons(lower, &last_box, false);
            let bridge_offset = orca_stl::max(ext_perimeter_spacing as f64, perimeter_width as f64);
            // SoftFever: improve bridging
            let bridge_margin = orca_stl::min(
                scale_(BRIDGE_INFILL_MARGIN) as f32,
                scale_(nozzle_diameter * BRIDGE_INFILL_MARGIN / 0.4) as f32,
            );
            bridge_checker = offset_ex_expolygons(
                &diff_ex_expolygons_polygons(
                    orig_polygons,
                    &lower_polygons_series_clipped,
                    ApplySafetyOffset::Yes,
                ),
                (1.5 * bridge_offset + f64::from(bridge_margin) + perimeter_spacing as f64 / 2.)
                    as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            );
        }
        let delete_bridge =
            diff_ex_expolygons(orig_polygons, &bridge_checker, ApplySafetyOffset::Yes);

        let mut top_polygons = diff_ex_expolygons_polygons(
            &delete_bridge,
            &upper_polygons_series_clipped,
            ApplySafetyOffset::Yes,
        );
        // get the not-top surface, from the "real top" but enlarged by external_infill_margin (and
        // the min_width_top_surface we removed a bit before)
        let temp_gap = diff_ex_expolygons(&top_polygons, fill_clip, ApplySafetyOffset::No);
        let inner_polygons = diff_ex_expolygons(
            orig_polygons,
            &offset_ex_expolygons(
                &top_polygons,
                (offset_top_surface as f64 + min_width_top_surface
                    - ext_perimeter_spacing as f64 / 2.) as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            ApplySafetyOffset::Yes,
        );
        // get the enlarged top surface, by using inner_polygons instead of upper_slices, and clip
        // it for it to be exactly the polygons to fill.
        top_polygons = diff_ex_expolygons(fill_clip, &inner_polygons, ApplySafetyOffset::Yes);
        // increase by half peri the inner space to fill the frontier between last and stored.
        *top_fills = union_ex_two(top_fills, &top_polygons, false);
        //set the clip to the external wall but go back inside by infill_extrusion_width/2 to be
        // sure the extrusion won't go outside even with a 100% overlap.
        let mut infill_spacing_unscaled = self
            .config
            .sparse_infill_line_width
            .get_abs_value(nozzle_diameter);
        if infill_spacing_unscaled == 0. {
            infill_spacing_unscaled = f64::from(Flow::auto_extrusion_width(
                FlowRole::Infill,
                nozzle_diameter as f32,
            ));
        }
        *fill_clip = offset_ex_expolygons(
            orig_polygons,
            (ext_perimeter_spacing as f64 / 2. - scale_(infill_spacing_unscaled / 2.)) as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );

        let mut non_top_polygons =
            intersection_ex_expolygons(&inner_polygons, orig_polygons, ApplySafetyOffset::No);
        if has_gap_fill {
            non_top_polygons = union_ex_two(&non_top_polygons, &temp_gap, false);
        }
        non_top_polygons
    }

    /// `apply_extra_perimeters(infill_area)`: with extra perimeters on overhangs on (and overhang
    /// detection, walls, lower slices, above the raft, not spiral), the extra perimeters of the
    /// infill area put before the last island's perimeters, and the area they fill taken out of
    /// `fill_surfaces`.
    ///
    /// # Errors
    /// What [`generate_extra_perimeters_over_overhangs`] returns.
    ///
    /// # Panics
    /// With [`NotPorted`](crate::stop::NotPorted) when the last loop entity is not a collection.
    pub fn apply_extra_perimeters(
        &mut self,
        infill_area: &[ExPolygon],
    ) -> Result<(), PerimeterError> {
        if !self.m_spiral_vase
            && self.lower_slices.is_some()
            && self.config.detect_overhang_wall.value
            && self.config.extra_perimeters_on_overhangs.value
            && self.config.wall_loops.value > 0
            && self.layer_id > self.object_config.raft_layers.value
        {
            // Generate extra perimeters on overhang areas, and cut them to these parts only, to
            // save print time and material
            let (extra_perimeters, filled_area) = generate_extra_perimeters_over_overhangs(
                infill_area.to_vec(),
                &self.m_lower_slices_polygons,
                self.config.wall_loops.value,
                &self.overhang_flow,
                self.m_scaled_resolution,
                self.object_config,
                self.print_config,
            )?;
            if !extra_perimeters.is_empty() {
                let Some(ExtrusionEntity::Collection(this_islands_perimeters)) =
                    self.loops.entities.last_mut()
                else {
                    not_ported(
                        "PerimeterGenerator::apply_extra_perimeters",
                        NO_ISLAND_PERIMETERS,
                    )
                };
                let mut new_perimeters = ExtrusionEntityCollection::default();
                new_perimeters.no_sort = this_islands_perimeters.no_sort;
                for paths in &extra_perimeters {
                    new_perimeters.append_paths(paths);
                }
                new_perimeters.append_entities(&this_islands_perimeters.entities);
                this_islands_perimeters.swap(&mut new_perimeters);

                let orig_surfaces = self.fill_surfaces.clone();
                self.fill_surfaces.clear();
                for surface in &orig_surfaces.surfaces {
                    let new_surfaces = diff_ex_expolygons_polygons(
                        core::slice::from_ref(&surface.expolygon),
                        &filled_area,
                        ApplySafetyOffset::No,
                    );
                    self.fill_surfaces.append_templ(&new_surfaces, surface);
                }
            }
        }
        Ok(())
    }
}

/// An extrusion `detect_steep_overhang` tests: the C++ template's two instances, a loop's
/// `Polygons` and an Arachne line's `Polylines`, whose `diff_pl` differ.
pub trait SteepOverhangExtrusion {
    /// `diff_pl(extrusion, clip)`.
    fn diff_pl_with(&self, clip: &[Polygon]) -> Polylines;
}

impl SteepOverhangExtrusion for [Polygon] {
    fn diff_pl_with(&self, clip: &[Polygon]) -> Polylines {
        diff_pl_polygons(self, clip)
    }
}

impl SteepOverhangExtrusion for [Polyline] {
    fn diff_pl_with(&self, clip: &[Polygon]) -> Polylines {
        diff_pl(self, clip)
    }
}

/// `detect_steep_overhang(config, is_contour, extrusion_bboxs, extrusion_width, extrusion,
/// lower_slices, steep_overhang_contour, steep_overhang_hole)`: whether some of the extrusion lies
/// farther than the overhang reverse threshold off the lower slices (always, for a threshold under
/// `EPSILON`), which marks the contours or the holes as steep.
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) without lower slices past the threshold test, which
/// the C++ dereferences.
#[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
pub fn detect_steep_overhang<T: SteepOverhangExtrusion + ?Sized>(
    config: &PrintRegionConfig,
    is_contour: bool,
    extrusion_bboxs: &BoundingBox,
    extrusion_width: f64,
    extrusion: &T,
    lower_slices: Option<&[ExPolygon]>,
    steep_overhang_contour: &mut bool,
    steep_overhang_hole: &mut bool,
) -> bool {
    let threshold = config
        .overhang_reverse_threshold
        .get_abs_value(extrusion_width);
    // Special case: reverse on every even (from GUI POV) layer
    if threshold < EPSILON {
        if is_contour {
            *steep_overhang_contour = true;
        } else {
            *steep_overhang_hole = true;
        }

        return true;
    }

    let Some(lower_slices) = lower_slices else {
        not_ported("detect_steep_overhang", NO_LOWER_SLICES)
    };
    let lower_slcier_chopped =
        clip_clipper_polygons_with_subject_bbox_expolygons(lower_slices, extrusion_bboxs, true);

    // All we need to check is whether we have lines outside `threshold`
    let off = threshold - 0.5 * extrusion_width;

    let limiton_polygons = offset(
        &lower_slcier_chopped,
        scale_(off) as f32,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    );

    let remain_polylines = extrusion.diff_pl_with(&limiton_polygons);
    if !remain_polylines.is_empty() {
        if is_contour {
            *steep_overhang_contour = true;
        } else {
            *steep_overhang_hole = true;
        }

        return true;
    }

    false
}

/// `traverse_loops(perimeter_generator, loops, thin_walls, steep_overhang_contour,
/// steep_overhang_hole, reverse_thin_wall_hole)`: the loops of one depth and, recursively, their
/// children as one collection, the thin walls joining the first call's (see the module header).
///
/// # Errors
/// A flow error of the thin walls' `variable_width`.
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) where the C++ has undefined behaviour (the module
/// header lists them) or a region would fuzzify a wall.
#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
pub fn traverse_loops(
    perimeter_generator: &PerimeterGenerator<'_>,
    loops: &[PerimeterGeneratorLoop],
    thin_walls: &mut ThickPolylines,
    steep_overhang_contour: &mut bool,
    steep_overhang_hole: &mut bool,
    reverse_thin_wall_hole: bool,
) -> Result<ExtrusionEntityCollection, PerimeterError> {
    let pg = perimeter_generator;
    // loops is an arrayref of ::Loop objects; turn each one into an ExtrusionLoop object
    let mut coll = ExtrusionEntityCollection::default();

    // Detect steep overhangs. Only calculate overhang degree on even (from GUI POV) layers
    let overhangs_reverse = pg.config.overhang_reverse.value && pg.layer_id % 2 == 1;

    for loop_ in loops {
        let is_external = loop_.is_external();
        let is_small_width = loop_.is_smaller_width_perimeter;

        let role = if is_external {
            ExtrusionRole::ExternalPerimeter
        } else {
            ExtrusionRole::Perimeter
        };
        let loop_role = if loop_.is_internal_contour() {
            // Note that we set loop role to ContourInternalPerimeter also when loop is both
            // internal and external (i.e. there's only one contour loop).
            ExtrusionLoopRole::INTERNAL
        } else if loop_.is_contour {
            ExtrusionLoopRole::DEFAULT
        } else {
            ExtrusionLoopRole::HOLE
        };

        // BBS: get lower polygons series, width, mm3_per_mm
        let (lower_polygons_series, extrusion_mm3_per_mm, extrusion_width) = if is_external {
            if is_small_width {
                //BBS: smaller width external perimeter
                (
                    &pg.m_smaller_external_lower_polygons_series,
                    pg.smaller_width_ext_mm3_per_mm(),
                    f64::from(pg.smaller_ext_perimeter_flow.width()),
                )
            } else {
                //BBS: normal external perimeter
                (
                    &pg.m_external_lower_polygons_series,
                    pg.ext_mm3_per_mm(),
                    f64::from(pg.ext_perimeter_flow.width()),
                )
            }
        } else {
            //BBS: normal perimeter
            (
                &pg.m_lower_polygons_series,
                pg.mm3_per_mm(),
                f64::from(pg.perimeter_flow.width()),
            )
        };

        // Apply fuzzy skin if it is enabled for at least some part of the polygon.
        let polygon = apply_fuzzy_skin(
            &loop_.polygon,
            pg,
            usize::from(loop_.depth),
            loop_.is_contour,
        );

        let mut paths = ExtrusionPaths::new();
        if pg.config.detect_overhang_wall.value && pg.layer_id > pg.object_config.raft_layers.value
        {
            // detect overhanging/bridging perimeters

            // get non 100% overhang paths by intersecting this loop with the grown lower slices;
            // prepare grown lower layer slices for overhang detection
            let mut bbox = BoundingBox::from_points(&polygon.points);
            bbox.offset(SCALED_EPSILON);

            // Always reverse extrusion if use fuzzy skin:
            // https://github.com/OrcaSlicer/OrcaSlicer/pull/2413#issuecomment-1769735357
            if overhangs_reverse && pg.has_fuzzy_skin {
                if loop_.is_contour {
                    *steep_overhang_contour = true;
                } else if pg.has_fuzzy_hole {
                    *steep_overhang_hole = true;
                }
            }
            // Detect steep overhang. Skip the check if we already found steep overhangs
            let found_steep_overhang = (loop_.is_contour && *steep_overhang_contour)
                || (!loop_.is_contour && *steep_overhang_hole);
            if overhangs_reverse && !found_steep_overhang {
                detect_steep_overhang(
                    pg.config,
                    loop_.is_contour,
                    &bbox,
                    extrusion_width,
                    core::slice::from_ref(&polygon),
                    pg.lower_slices,
                    steep_overhang_contour,
                    steep_overhang_hole,
                );
            }

            let Some(series_back) = lower_polygons_series.last() else {
                not_ported("traverse_loops", EMPTY_LIST)
            };
            let lower_polygons_series_clipped =
                clip_clipper_polygons_with_subject_bbox(series_back, &bbox);

            let inside_polines = intersection_pl_polygons(
                core::slice::from_ref(&polygon),
                &lower_polygons_series_clipped,
            );

            let remain_polines = diff_pl_polygons(
                core::slice::from_ref(&polygon),
                &lower_polygons_series_clipped,
            );

            if !inside_polines.is_empty() {
                extrusion_paths_append(
                    &mut paths,
                    &inside_polines,
                    role,
                    extrusion_mm3_per_mm,
                    extrusion_width as f32,
                    pg.layer_height as f32,
                );
            }

            // get 100% overhang paths by checking what parts of this loop fall outside the grown
            // lower slices (thus where the distance between the loop centerline and original
            // lower slices is >= half nozzle diameter
            if !remain_polines.is_empty() {
                extrusion_paths_append(
                    &mut paths,
                    &remain_polines,
                    ExtrusionRole::OverhangPerimeter,
                    pg.mm3_per_mm_overhang(),
                    pg.overhang_flow.width(),
                    pg.overhang_flow.height(),
                );
            }

            // Reapply the nearest point search for starting point. We allow polyline reversal
            // because Clipper may have randomly reversed polylines during clipping.
            if paths.is_empty() {
                continue;
            }
            let first = paths[0].first_point();
            let start_pt = Point::new(first.x, first.y);
            chain_and_reorder_extrusion_paths(&mut paths, Some(&start_pt));
        } else {
            if overhangs_reverse && pg.layer_id > pg.object_config.raft_layers.value {
                // Always reverse if detect overhang wall is not enabled
                *steep_overhang_contour = true;
                *steep_overhang_hole = true;
            }

            let mut path = ExtrusionPath::with_role(role);
            //BBS.
            path.polyline = Polyline3::from_polyline(&polygon.split_at_first_point(), 0);
            path.mm3_per_mm = extrusion_mm3_per_mm;
            path.width = extrusion_width as f32;
            path.height = pg.layer_height as f32;
            cpp_vec::push(&mut paths, path);
        }

        coll.append_move(ExtrusionEntity::Loop(ExtrusionLoop::from_paths(
            paths, loop_role,
        )));
    }

    // Append thin walls to the nearest-neighbor search (only for first iteration)
    if !thin_walls.is_empty() {
        variable_width(
            thin_walls,
            ExtrusionRole::ExternalPerimeter,
            &pg.ext_perimeter_flow,
            &mut coll.entities,
        )?;
        thin_walls.clear();
    }

    // Traverse children and build the final collection.
    let zero_point = Point::new(0, 0);
    let chain = chain_extrusion_entities(&coll.entities, Some(&zero_point));
    let mut slots: Vec<Option<ExtrusionEntity>> = core::mem::take(&mut coll.entities)
        .into_iter()
        .map(Some)
        .collect();
    let mut out = ExtrusionEntityCollection::default();
    for &(idx, reverse) in &chain {
        if idx >= loops.len() {
            // This is a thin wall.
            let entity = slots[idx].take().expect("each entity is chained once");
            out.entities.push(entity);
            if reverse {
                let last = out.entities.len() - 1;
                out.entities[last].reverse();
            }
        } else {
            let loop_ = &loops[idx];
            let reverse_children_thin_wall_hole = loops.len() == 1
                && loop_.is_contour
                && loop_.children.len() == 1
                && !loop_.children[0].is_contour
                && loop_.children[0].children.is_empty();
            let children = traverse_loops(
                pg,
                &loop_.children,
                thin_walls,
                steep_overhang_contour,
                steep_overhang_hole,
                reverse_children_thin_wall_hole,
            )?;
            let Some(ExtrusionEntity::Loop(mut eloop)) = slots[idx].take() else {
                not_ported("traverse_loops", NOT_A_LOOP)
            };

            if (pg.config.wall_direction.value == WallDirection::CounterClockwise)
                == (loop_.is_contour || reverse_thin_wall_hole)
            {
                eloop.make_counter_clockwise();
            } else {
                eloop.make_clockwise();
            }

            // Orca: Reverse print order for thin wall holes.
            if reverse_thin_wall_hole {
                out.entities.reverse();
            }

            eloop.inset_idx = i32::from(loop_.depth);
            if loop_.is_contour {
                out.append_entities_move(children.entities);
                out.entities.push(ExtrusionEntity::Loop(eloop));
            } else {
                out.entities.push(ExtrusionEntity::Loop(eloop));
                out.append_entities_move(children.entities);
            }
        }
    }
    Ok(out)
}

/// `(a - b).cast<double>().squaredNorm()` of two ClipperLib_Z points: x, y and z, the differences
/// taken in 64-bit integers.
#[allow(clippy::cast_precision_loss)]
fn squared_norm_3(a: IntPoint, b: IntPoint) -> f64 {
    let dx = a.x.wrapping_sub(b.x) as f64;
    let dy = a.y.wrapping_sub(b.y) as f64;
    let dz = a.z.wrapping_sub(b.z) as f64;
    dx * dx + dy * dy + dz * dz
}

/// `clip_extrusion(subject, clip, clipType)`: the open path `subject` (z its width) clipped by the
/// closed `clip` paths, each crossing given the width interpolated along the subject's edge, and
/// each point left without one the width at its projection onto the subject.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn clip_extrusion(
    subject: &[IntPoint],
    clip: &[clipper_z::Path],
    clip_type: ClipType,
) -> clipper_z::Paths {
    let mut clipper = clipper_z::Clipper::new(0);
    clipper.z_fill_function(|e1bot, e1top, e2bot, e2top, pt: &mut IntPoint| {
        let mut start = e1bot;
        let mut end = e1top;

        if start.z <= 0 && end.z <= 0 {
            start = e2bot;
            end = e2top;
        }

        // Interpolate extrusion line width.
        let length_sqr = squared_norm_3(end, start);
        let dist_sqr = squared_norm_3(*pt, start);
        let t = orca_crt::sqrt(dist_sqr / length_sqr);

        pt.z = start
            .z
            .wrapping_add(orca_crt::f64_to_i64(end.z.wrapping_sub(start.z) as f64 * t));
    });

    clipper.add_path(subject, PolyType::Subject, false);
    clipper.add_paths(clip.iter().map(Vec::as_slice), PolyType::Clip, true);

    let mut clipped_paths;
    {
        let mut clipped_polytree = PolyTree::default();
        clipper.execute_tree(
            clip_type,
            &mut clipped_polytree,
            PolyFillType::NonZero,
            PolyFillType::NonZero,
        );
        clipped_paths = clipper_z::poly_tree_to_paths(&clipped_polytree);
    }

    // Clipped path could contain vertices from the clip with a Z coordinate equal to zero. For
    // those vertices, we must assign value based on the subject. This happens only in sporadic
    // cases.
    for path in &mut clipped_paths {
        for c_pt in path.iter_mut() {
            if c_pt.z == 0 {
                // Now we must find the corresponding line on with this point is located and
                // compute line width (Z coordinate).
                if subject.len() <= 2 {
                    continue;
                }

                let pt = Point::new(c_pt.x, c_pt.y);
                let mut projected_pt_min = Point::default();
                let mut it_min = 0usize;
                let mut dist_sqr_min = f64::MAX;
                let mut prev = Point::new(subject[0].x, subject[0].y);
                for (it, s) in subject.iter().enumerate().skip(1) {
                    let curr = Point::new(s.x, s.y);
                    let projected_pt = pt.projection_onto_line(&Line { a: prev, b: curr });
                    let dist_sqr = (projected_pt - pt).cast_f64().squared_norm();
                    if dist_sqr < dist_sqr_min {
                        dist_sqr_min = dist_sqr;
                        projected_pt_min = projected_pt;
                        it_min = it - 1;
                    }
                    prev = curr;
                }

                let pt_a = Point::new(subject[it_min].x, subject[it_min].y);
                let pt_b = Point::new(subject[it_min + 1].x, subject[it_min + 1].y);
                let line_len = (pt_b - pt_a).cast_f64().norm();
                let dist = (projected_pt_min - pt_a).cast_f64().norm();
                c_pt.z = orca_crt::f64_to_i64(
                    subject[it_min].z as f64
                        + (dist / line_len)
                            * subject[it_min + 1].z.wrapping_sub(subject[it_min].z) as f64,
                );
            }
        }
    }

    clipped_paths
}

/// `PerimeterGeneratorArachneExtrusion`: an Arachne line, as its index in the caller's list of
/// lines, and whether a closed one is a contour (else a hole).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PerimeterGeneratorArachneExtrusion {
    /// `extrusion`: the line's index in the list [`traverse_extrusions`] is given.
    pub extrusion: usize,
    /// `is_contour`: only read for a closed line.
    pub is_contour: bool,
}

/// `PointInfo`: how often a point ends a path, and whether it ends an overhang path.
#[derive(Clone, Copy, Debug, Default)]
struct PointInfo {
    occurrence: usize,
    is_overhang: bool,
}

/// `traverse_extrusions(perimeter_generator, pg_extrusions, steep_overhang_contour,
/// steep_overhang_hole)`: the Arachne lines in the given order as loops (closed lines) and
/// multi-paths (open ones), split into walls and overhang walls where overhangs are detected (see
/// the module header). `lines` holds the lines the entries index; the fuzzy skin may change them.
///
/// # Errors
/// A flow error of `extrusion_paths_append`.
///
/// # Panics
/// With [`NotPorted`](crate::stop::NotPorted) where a region would fuzzify a wall or the steep
/// overhang test lacks lower slices.
#[allow(clippy::too_many_lines)]
pub fn traverse_extrusions(
    perimeter_generator: &PerimeterGenerator<'_>,
    pg_extrusions: &[PerimeterGeneratorArachneExtrusion],
    lines: &mut [ExtrusionLine],
    steep_overhang_contour: &mut bool,
    steep_overhang_hole: &mut bool,
) -> Result<ExtrusionEntityCollection, PerimeterError> {
    let pg = perimeter_generator;
    // Detect steep overhangs. Only calculate overhang degree on even (from GUI POV) layers
    let overhangs_reverse = pg.config.overhang_reverse.value && pg.layer_id % 2 == 1;

    let mut extrusion_coll = ExtrusionEntityCollection::default();
    for pg_extrusion in pg_extrusions {
        let extrusion = &mut lines[pg_extrusion.extrusion];
        if extrusion.empty() {
            continue;
        }

        let is_external = extrusion.inset_idx == 0;
        let role = if is_external {
            ExtrusionRole::ExternalPerimeter
        } else {
            ExtrusionRole::Perimeter
        };

        let is_contour = !extrusion.is_closed || pg_extrusion.is_contour;
        apply_fuzzy_skin_line(extrusion, pg, is_contour);
        let extrusion = &*extrusion;
        let flow = if is_external {
            &pg.ext_perimeter_flow
        } else {
            &pg.perimeter_flow
        };

        let mut paths = ExtrusionPaths::new();
        // detect overhanging/bridging perimeters
        if pg.config.detect_overhang_wall.value && pg.layer_id > pg.object_config.raft_layers.value
        {
            let mut extrusion_path = clipper_z::Path::with_capacity(extrusion.size());
            let mut extrusion_path_bbox = BoundingBox::new();
            for ej in &extrusion.junctions {
                extrusion_path.push(IntPoint::new(ej.p.x, ej.p.y, ej.w));
                extrusion_path_bbox.merge(Point::new(ej.p.x, ej.p.y));
            }

            let mut lower_slices_paths =
                clipper_z::Paths::with_capacity(pg.lower_slices_polygons().len());
            {
                let mut clipped: Points;
                extrusion_path_bbox.offset(SCALED_EPSILON);
                for poly in pg.lower_slices_polygons() {
                    clipped = clip_clipper_polygon_with_subject_bbox_points(
                        &poly.points,
                        &extrusion_path_bbox,
                        false,
                    );
                    if !clipped.is_empty() {
                        let mut out = clipper_z::Path::with_capacity(clipped.len());
                        for pt in &clipped {
                            out.push(IntPoint::new(pt.x, pt.y, 0));
                        }
                        lower_slices_paths.push(out);
                    }
                }
            }

            // get non-overhang paths by intersecting this loop with the grown lower slices
            extrusion_paths_append_zpaths(
                &mut paths,
                &clip_extrusion(&extrusion_path, &lower_slices_paths, ClipType::Intersection),
                role,
                flow,
            )?;

            // Always reverse extrusion if use fuzzy skin:
            // https://github.com/OrcaSlicer/OrcaSlicer/pull/2413#issuecomment-1769735357
            if overhangs_reverse && pg.has_fuzzy_skin {
                if pg_extrusion.is_contour {
                    *steep_overhang_contour = true;
                } else if pg.has_fuzzy_hole {
                    *steep_overhang_hole = true;
                }
            }
            // Detect steep overhang. Skip the check if we already found steep overhangs
            let found_steep_overhang = (pg_extrusion.is_contour && *steep_overhang_contour)
                || (!pg_extrusion.is_contour && *steep_overhang_hole);
            if overhangs_reverse && !found_steep_overhang {
                // `std::map<double, ExtrusionPaths>`: the paths by width, in increasing order,
                // each a copy (the `std::move` of a const path copies).
                let mut recognization_paths: Vec<(f64, ExtrusionPaths)> = Vec::new();
                for path in &paths {
                    let width = f64::from(path.width);
                    // The key's place by `std::less<double>`; equal when neither is less.
                    let i =
                        orca_stl::lower_bound_by(&recognization_paths, &width, |(k, _), w| k < w);
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    let equivalent =
                        i < recognization_paths.len() && !(width < recognization_paths[i].0);
                    if equivalent {
                        cpp_vec::push(&mut recognization_paths[i].1, path.clone());
                    } else {
                        recognization_paths.insert(i, (width, vec![path.clone()]));
                    }
                }
                for (width, group) in &recognization_paths {
                    let mut be_clipped = Polylines::new();

                    for p in group {
                        be_clipped.push(p.polyline.to_polyline());
                    }

                    let extrusion_bboxs = get_extents_polylines(&be_clipped);

                    if detect_steep_overhang(
                        pg.config,
                        pg_extrusion.is_contour,
                        &extrusion_bboxs,
                        *width,
                        be_clipped.as_slice(),
                        pg.lower_slices,
                        steep_overhang_contour,
                        steep_overhang_hole,
                    ) {
                        break;
                    }
                }
            }

            // get overhang paths by checking what parts of this loop fall outside the grown lower
            // slices (thus where the distance between the loop centerline and original lower
            // slices is >= half nozzle diameter
            extrusion_paths_append_zpaths(
                &mut paths,
                &clip_extrusion(&extrusion_path, &lower_slices_paths, ClipType::Difference),
                ExtrusionRole::OverhangPerimeter,
                &pg.overhang_flow,
            )?;

            // Reapply the nearest point search for starting point. We allow polyline reversal
            // because Clipper may have randomly reversed polylines during clipping. Arachne
            // sometimes creates extrusion with zero-length (just two same endpoints);
            if !paths.is_empty() {
                let mut start_point = paths[0].first_point();
                if !extrusion.is_closed {
                    // Especially for open extrusion, we need to select a starting point that is
                    // at the start or the end of the extrusions to make one continuous line.
                    // Also, we prefer a non-overhang starting point.
                    let mut point_occurrence: UnorderedMap<Point, PointInfo, _> =
                        UnorderedMap::with_hasher(|p: &Point| point_hash(*p), orca_stl::EqualTo);
                    for path in &paths {
                        let first_p = path.polyline.first_point().to_point();
                        let last_p = path.polyline.last_point().to_point();
                        point_occurrence.get_or_insert_default(first_p).occurrence += 1;
                        point_occurrence.get_or_insert_default(last_p).occurrence += 1;
                        if path.role() == ExtrusionRole::OverhangPerimeter {
                            point_occurrence.get_or_insert_default(first_p).is_overhang = true;
                            point_occurrence.get_or_insert_default(last_p).is_overhang = true;
                        }
                    }

                    // Prefer non-overhang point as a starting point.
                    for (p, info) in point_occurrence.iter() {
                        if info.occurrence == 1 {
                            start_point = *p;
                            if !info.is_overhang {
                                start_point = *p;
                                break;
                            }
                        }
                    }
                }

                chain_and_reorder_extrusion_paths(&mut paths, Some(&start_point));

                if overhangs_reverse {
                    for path in &paths {
                        if path.role() == ExtrusionRole::OverhangPerimeter {
                            if pg_extrusion.is_contour {
                                *steep_overhang_contour = true;
                            } else {
                                *steep_overhang_hole = true;
                            }
                            break;
                        }
                    }
                }
            }
        } else {
            if overhangs_reverse && pg.layer_id > pg.object_config.raft_layers.value {
                // Always reverse if detect overhang wall is not enabled
                *steep_overhang_contour = true;
                *steep_overhang_hole = true;
            }

            extrusion_paths_append_line(&mut paths, extrusion, role, flow)?;
        }

        // Append paths to collection.
        if !paths.is_empty() {
            if extrusion.is_closed {
                let mut extrusion_loop = ExtrusionLoop::from_paths(
                    paths,
                    if pg_extrusion.is_contour {
                        ExtrusionLoopRole::DEFAULT
                    } else {
                        ExtrusionLoopRole::HOLE
                    },
                );
                if (pg.config.wall_direction.value == WallDirection::CounterClockwise)
                    == (pg_extrusion.is_contour || pg_extrusions.len() == 2)
                {
                    extrusion_loop.make_counter_clockwise();
                } else {
                    extrusion_loop.make_clockwise();
                }
                extrusion_coll.append_move(ExtrusionEntity::Loop(extrusion_loop));
                // Orca: Reverse the order of paths for thin wall holes. We define thin wall hole
                // as a hole with only one perimeter.
                let thin_wall_hole = !pg_extrusion.is_contour && pg_extrusions.len() == 2;
                if thin_wall_hole && pg.config.wall_sequence.value != WallSequence::OuterInner {
                    extrusion_coll.entities.reverse();
                }
            } else {
                // Because we are processing one ExtrusionLine all ExtrusionPaths should form one
                // connected path. But there is possibility that due to numerical issue there is
                // poss
                let mut rest = paths.into_iter();
                let mut multi_path = ExtrusionMultiPath::default();
                if let Some(front) = rest.next() {
                    cpp_vec::push(&mut multi_path.paths, front);
                }

                for it_path in rest {
                    if multi_path.paths[multi_path.paths.len() - 1].last_point()
                        != it_path.first_point()
                    {
                        extrusion_coll.append_move(ExtrusionEntity::MultiPath(core::mem::take(
                            &mut multi_path,
                        )));
                    }
                    cpp_vec::push(&mut multi_path.paths, it_path);
                }

                extrusion_coll.append_move(ExtrusionEntity::MultiPath(multi_path));
            }
        }
    }

    Ok(extrusion_coll)
}

/// `reorient_perimeters(entities, steep_overhang_contour, steep_overhang_hole,
/// reverse_internal_only)`: each loop reversed when its kind (hole or contour) is steep, unless
/// only internal loops are reversed and it has an external wall.
pub fn reorient_perimeters(
    entities: &mut ExtrusionEntityCollection,
    steep_overhang_contour: bool,
    steep_overhang_hole: bool,
    reverse_internal_only: bool,
) {
    if steep_overhang_hole || steep_overhang_contour {
        for entity in &mut entities.entities {
            let eloop = match entity {
                ExtrusionEntity::Loop(l) => &*l,
                ExtrusionEntity::LoopSloped(ls) => &ls.base,
                _ => continue,
            };
            // Only reverse when needed
            let need_reverse =
                if (eloop.loop_role().0 & ExtrusionLoopRole::HOLE.0) == ExtrusionLoopRole::HOLE.0 {
                    steep_overhang_hole
                } else {
                    steep_overhang_contour
                };

            let mut is_external = false;
            if reverse_internal_only {
                for path in &eloop.paths {
                    if path.role() == ExtrusionRole::ExternalPerimeter {
                        is_external = true;
                        break;
                    }
                }
            }

            if need_reverse && !is_external {
                entity.reverse();
            }
        }
    }
}
