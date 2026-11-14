// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: src/libslic3r/Layer.hpp lines 1-349 of OrcaSlicer v2.4.2, and class
// CurledLine of src/libslic3r/Line.hpp lines 212-222, translated on 2026-11-14 (orca-o3-layer-2).
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors
// (Prusa Research) and the Slic3r authors (Alessandro Ranellucci), under the GNU Affero General
// Public License, version 3 (AGPL-3.0; LICENSE).
//
// Carries OrcaSlicer code: src/libslic3r/Layer.cpp lines 1-137 and 401-476 (empty, add_region,
// make_slices, the raw slices' backup and restores, merged, get_sparse_infill_max_void_area,
// get_extruder_id and both get_extents) and LayerRegion.cpp lines 1-80 (flow, bridging_flow and
// slices_to_fill_surfaces_clipped) of OrcaSlicer v2.4.2, translated on 2026-11-14
// (orca-o3-layer-3). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the
// PrusaSlicer authors (Prusa Research) and the Slic3r authors, under the GNU Affero General Public
// License, version 3 (AGPL-3.0; LICENSE).

//! Layer.hpp: [`Layer`], its [`LayerRegion`]s and [`SupportLayer`], with the members their
//! headers define.
//!
//! The C++ back-pointers are not held. A layer's object (`m_object`) and a region's layer
//! (`m_layer`) are arguments of the functions that need them; a region's print region
//! (`m_region`) is an index into its object's printing regions (`PrintObjectRegions::all_regions`,
//! where `PrintObject::slice` takes it from: layer region `i` of a sliced layer is printing region
//! `i`), and [`LayerRegion::region`] looks it up in the object. A layer's `upper_layer` and
//! `lower_layer` are indices into the list that owns the layer (the object's layers, or its
//! support layers for a support layer). A [`SupportLayer`] holds its [`Layer`] (C++ inheritance);
//! the C++'s virtual `has_extrusions` is [`Layer::has_extrusions`] for a layer and
//! [`SupportLayer::has_extrusions`] for a support layer. An [`AreaGroup`]'s `ExPolygon *area`
//! points into one of the support layer's area lists: here it is the index into the list its type
//! names (base, roof, floor, or the layer just below the roof).
//!
//! The C++ `mutable` members (`sharp_tails`, `cantilevers`, `sharp_tails_height`) are plain
//! fields: the support check that writes them holds the layers mutably.
//!
//! Layer.cpp's and LayerRegion.cpp's base (orca-o3-layer-3, 2026-11-14) are here:
//! [`Layer::empty`], [`Layer::add_region`], [`Layer::make_slices`] (one region's slices as they
//! are, several united with the safety offset, then ordered by `chain_points` over the contours'
//! first points), the raw slices' backup and two restores (Orca always keeps the raw slices),
//! [`Layer::merged`] (with no offset, an `EPSILON` offset out before the union and back after),
//! [`Layer::get_sparse_infill_max_void_area`] (its spacing computed in `float`, as the C++'s
//! `coord_t * float` is), [`Layer::get_extruder_id`], [`get_extents_layer_region`] and
//! [`get_extents_layer_regions`]; [`LayerRegion::flow`], [`LayerRegion::flow_at`],
//! [`LayerRegion::bridging_flow`] and [`LayerRegion::slices_to_fill_surfaces_clipped`]. The
//! layer's object and its print are arguments where the C++ follows `m_layer->object()` and
//! `object()->print()`. ClipperUtils' overloads over `Surfaces` forward each surface's island in
//! order, so they are the `ExPolygons` overloads of orca-geom over the islands.
//!
//! The methods whose bodies are in the later steps are written with their bodies, as free
//! functions or extension traits of the crate that carries them: `Layer::make_perimeters`,
//! `is_perimeter_compatible` and `LayerRegion::make_perimeters` (orca-perimeter);
//! `LayerRegion::prepare_fill_surfaces`, `process_external_surfaces`, `infill_area_threshold`,
//! `trim_surfaces` and `elephant_foot_compensation_step` (orca-surface and orca-print, where
//! their lines are); `Layer::make_fills`, `generate_sparse_infill_polylines_for_anchoring` and
//! `make_ironing` (orca-fill); the simplify functions of both and of `SupportLayer`
//! (`simplify_wall_extrusion_path`, `simplify_infill_extrusion_path`,
//! `simplify_infill_extrusion_entity`, `simplify_wall_extrusion_entity`,
//! `simplify_support_extrusion_path`, whose inline bodies call LayerRegion.cpp's and Layer.cpp's
//! `simplify_*`, orca-plate's); `make_contour_z` (non-planar contouring, never in domain A); and
//! the SVG exports (debugging only).

use orca_config::print_config::InfillPattern;
use orca_geom::bounding_box::BoundingBox;
use orca_geom::clipper::PolyFillType;
use orca_geom::clipper_utils::{
    ApplySafetyOffset, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, intersection_ex_expolygons,
    offset_ex_expolygons, offset_expolygons, union_ex, union_safety_offset_ex,
};
use orca_geom::expolygon::ExPolygons;
use orca_geom::line::Line;
use orca_geom::point::{Point, Points};
use orca_geom::polygon::Polygons;
use orca_geom::polyline::Polylines;
use orca_index::stop::not_ported;
use orca_paths::points::chain_points;

use crate::collection::ExtrusionEntityCollection;
use crate::flow::{Flow, FlowError, FlowRole, nozzle_diameter_at};
use crate::print::Print;
use crate::print_object::PrintObject;
use crate::surface::{self, ST_COUNT, Surface, SurfaceType, to_expolygons, to_polygons_surfaces};
use crate::surface_collection::{ExPolygonContains, SurfaceCollection};

/// `class CurledLine` of Line.hpp: a line and the height its extrusion curls up to. Belongs in
/// orca-geom (Line.hpp), which has not needed it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurledLine {
    /// The `Line` it derives from.
    pub line: Line,
    /// `curled_height`.
    pub curled_height: f32,
}

impl CurledLine {
    /// `CurledLine(a, b)`: no curl.
    #[must_use]
    pub fn new(a: Point, b: Point) -> Self {
        Self::with_height(a, b, 0.0)
    }

    /// `CurledLine(a, b, curled_height)`.
    #[must_use]
    pub fn with_height(a: Point, b: Point, curled_height: f32) -> Self {
        Self {
            line: Line::new(a, b),
            curled_height,
        }
    }
}

/// `CurledLines`.
pub type CurledLines = Vec<CurledLine>;

/// `class LayerRegion`: one print region's part of a layer.
#[derive(Clone, Debug, Default)]
pub struct LayerRegion {
    /// `slices`: the surfaces of slicing the original geometry, by type.
    pub slices: SurfaceCollection,
    /// `raw_slices`: the slices before they were split into top, bottom and internal, kept for
    /// layers of several regions or with the elephant foot compensation.
    pub raw_slices: ExPolygons,
    /// `thin_fills`: the gap fills the perimeter generator makes, copied to `fills` by the infill.
    pub thin_fills: ExtrusionEntityCollection,
    /// `fill_expolygons`: unspecified fill polygons, for overhangs and restarting the infill.
    pub fill_expolygons: ExPolygons,
    /// `fill_surfaces`: the surfaces the infill fills.
    pub fill_surfaces: SurfaceCollection,
    /// `fill_no_overlap_expolygons`: fill polygons that keep the infill off the perimeters.
    pub fill_no_overlap_expolygons: ExPolygons,
    /// `unsupported_bridge_edges`.
    pub unsupported_bridge_edges: Polylines,
    /// `perimeters`: the perimeters, each an `ExtrusionEntityCollection`.
    pub perimeters: ExtrusionEntityCollection,
    /// `fills`: the infill, each an `ExtrusionEntityCollection`.
    pub fills: ExtrusionEntityCollection,
    /// `m_region`: the index of its print region in the object's printing regions.
    pub region: usize,
}

impl LayerRegion {
    /// `LayerRegion(layer, region)`: empty, of the object's printing region `region`.
    #[must_use]
    pub fn new(region: usize) -> Self {
        Self {
            region,
            ..Self::default()
        }
    }

    /// `region()`: the region's print region, found in its object.
    #[must_use]
    pub fn region<'a>(
        &self,
        object: &'a crate::print_object::PrintObject,
    ) -> &'a crate::print_region::PrintRegion {
        object.printing_region(self.region)
    }

    /// `get_slices()`.
    #[must_use]
    pub fn get_slices(&self) -> &SurfaceCollection {
        &self.slices
    }

    /// `has_extrusions()`: some perimeter or fill.
    #[must_use]
    pub fn has_extrusions(&self) -> bool {
        !self.perimeters.entities.is_empty() || !self.fills.entities.is_empty()
    }
}

/// `LayerRegionPtrs`.
pub type LayerRegionPtrs = Vec<LayerRegion>;

/// `class Layer`.
#[derive(Clone, Debug)]
pub struct Layer {
    /// `upper_layer`: the index of the layer above in the list that owns this one.
    pub upper_layer: Option<usize>,
    /// `lower_layer`: the index of the layer below in the list that owns this one.
    pub lower_layer: Option<usize>,
    /// `slicing_errors`.
    pub slicing_errors: bool,
    /// `slice_z`: the z slicing used, unscaled.
    pub slice_z: f64,
    /// `print_z`: the z printing uses, unscaled.
    pub print_z: f64,
    /// `height`: the layer height, unscaled.
    pub height: f64,
    /// `curled_lines`: extrusions estimated to curl badly, which fast travels avoid.
    pub curled_lines: CurledLines,
    /// `sharp_tails` (BBS, `mutable`).
    pub sharp_tails: ExPolygons,
    /// `cantilevers` (BBS, `mutable`).
    pub cantilevers: ExPolygons,
    /// `sharp_tails_height` (BBS, `mutable`).
    pub sharp_tails_height: Vec<f32>,
    /// `lslices`: the slices of every region merged into islands; on the first layer without the
    /// elephant foot compensation.
    pub lslices: ExPolygons,
    /// `lslices_extrudable` (BBS): the extrudable part of `lslices`, for tree supports.
    pub lslices_extrudable: ExPolygons,
    /// `lslices_bboxes`.
    pub lslices_bboxes: Vec<BoundingBox>,
    /// `loverhangs` (BBS).
    pub loverhangs: ExPolygons,
    /// `loverhangs_bbox` (BBS).
    pub loverhangs_bbox: BoundingBox,
    /// `m_id`: the layer's index in its object's layers, after the raft layers.
    pub id: usize,
    /// `m_regions`.
    pub regions: LayerRegionPtrs,
}

impl Layer {
    /// `Layer(id, object, height, print_z, slice_z)`: no neighbours, no slicing errors, no
    /// regions.
    #[must_use]
    pub fn new(id: usize, height: f64, print_z: f64, slice_z: f64) -> Self {
        Self {
            upper_layer: None,
            lower_layer: None,
            slicing_errors: false,
            slice_z,
            print_z,
            height,
            curled_lines: CurledLines::new(),
            sharp_tails: ExPolygons::new(),
            cantilevers: ExPolygons::new(),
            sharp_tails_height: Vec::new(),
            lslices: ExPolygons::new(),
            lslices_extrudable: ExPolygons::new(),
            lslices_bboxes: Vec::new(),
            loverhangs: ExPolygons::new(),
            loverhangs_bbox: BoundingBox::new(),
            id,
            regions: LayerRegionPtrs::new(),
        }
    }

    /// `id()`.
    #[must_use]
    pub fn id(&self) -> usize {
        self.id
    }

    /// `set_id(id)`.
    pub fn set_id(&mut self, id: usize) {
        self.id = id;
    }

    /// `bottom_z()`: `print_z - height`.
    #[must_use]
    pub fn bottom_z(&self) -> f64 {
        self.print_z - self.height
    }

    /// `region_count()`.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// `get_region(idx)` (the C++ takes an `int`; a negative one is undefined).
    #[must_use]
    pub fn get_region(&self, idx: usize) -> &LayerRegion {
        &self.regions[idx]
    }

    /// `get_region(idx)`, to change it.
    pub fn get_region_mut(&mut self, idx: usize) -> &mut LayerRegion {
        &mut self.regions[idx]
    }

    /// `regions()`.
    #[must_use]
    pub fn regions(&self) -> &[LayerRegion] {
        &self.regions
    }

    /// `any_internal_region_slice_contains(item)`.
    #[must_use]
    pub fn any_internal_region_slice_contains<T: ExPolygonContains + ?Sized>(
        &self,
        item: &T,
    ) -> bool {
        self.regions
            .iter()
            .any(|layerm| layerm.slices.any_internal_contains(item))
    }

    /// `any_bottom_region_slice_contains(item)`.
    #[must_use]
    pub fn any_bottom_region_slice_contains<T: ExPolygonContains + ?Sized>(
        &self,
        item: &T,
    ) -> bool {
        self.regions
            .iter()
            .any(|layerm| layerm.slices.any_bottom_contains(item))
    }

    /// `has_extrusions()` of a `Layer`: some region has an extrusion.
    #[must_use]
    pub fn has_extrusions(&self) -> bool {
        self.regions.iter().any(LayerRegion::has_extrusions)
    }

    /// `idx_higher_or_equal(begin, end, idx, fn_higher_equal)`: the first item from `idx` on for
    /// which `fn_higher_equal` holds, or the item count when none does; 0 for no items. With
    /// `idx` at `usize::MAX` (`IndexType(-1)`) it searches by halves, in `int`s, as the C++ does.
    pub fn idx_higher_or_equal<T>(
        items: &[T],
        mut idx: usize,
        mut fn_higher_equal: impl FnMut(&T) -> bool,
    ) -> usize {
        let size = items.len() as i32;
        if size == 0 {
            idx = 0;
        } else if idx == usize::MAX {
            // The first of a batch of layers: search by halves.
            let mut idx_low: i32 = 0;
            let mut idx_high: i32 = if 0 < size - 1 { size - 1 } else { 0 };
            while idx_low + 1 < idx_high {
                let idx_mid = (idx_low + idx_high) / 2;
                if fn_higher_equal(&items[idx_mid as usize]) {
                    idx_high = idx_mid;
                } else {
                    idx_low = idx_mid;
                }
            }
            idx = if fn_higher_equal(&items[idx_low as usize]) {
                idx_low as usize
            } else if fn_higher_equal(&items[idx_high as usize]) {
                idx_high as usize
            } else {
                size as usize
            };
        } else {
            // The other layers of the batch: search upwards, cheaper than by halves.
            while (idx as i32) < size && !fn_higher_equal(&items[idx]) {
                idx += 1;
            }
        }
        idx
    }
}

/// `LayerPtrs`.
pub type LayerPtrs = Vec<Layer>;

/// `enum SupportInnerType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SupportInnerType {
    /// `stInnerNormal`.
    #[default]
    Normal,
    /// `stInnerTree`.
    Tree,
}

/// `SupportLayer::AreaType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum AreaType {
    /// `BaseType`: in `base_areas`.
    Base = 0,
    /// `RoofType`: in `roof_areas`.
    Roof = 1,
    /// `FloorType`: in `floor_areas`.
    Floor = 2,
    /// `Roof1stLayer`: in `roof_1st_layer`.
    Roof1stLayer = 3,
}

/// `SupportLayer::AreaGroup`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaGroup {
    /// `area`: the index of the area in the support layer's list of its type.
    pub area: usize,
    /// `type` (an `AreaType` value).
    pub area_type: i32,
    /// `interface_id`.
    pub interface_id: i32,
    /// `interface_as_base`.
    pub interface_as_base: bool,
    /// `dist_to_top`: mm to the top.
    pub dist_to_top: f64,
    /// `need_infill`.
    pub need_infill: bool,
    /// `need_extra_wall`.
    pub need_extra_wall: bool,
}

impl AreaGroup {
    /// `AreaGroup(a, t, d)`.
    #[must_use]
    pub fn new(area: usize, area_type: i32, dist_to_top: f64) -> Self {
        Self {
            area,
            area_type,
            interface_id: 0,
            interface_as_base: false,
            dist_to_top,
            need_infill: false,
            need_extra_wall: false,
        }
    }
}

/// `class SupportLayer`: a layer of support.
#[derive(Clone, Debug)]
pub struct SupportLayer {
    /// The `Layer` it derives from.
    pub layer: Layer,
    /// `support_islands`: the areas the supports cover (base, interface and contact).
    pub support_islands: ExPolygons,
    /// `support_fills`: the support's extrusions.
    pub support_fills: ExtrusionEntityCollection,
    /// `support_type`.
    pub support_type: SupportInnerType,
    /// `base_areas`, for tree supports.
    pub base_areas: ExPolygons,
    /// `m_interface_id`: the index of an interface layer, alternating the interface direction.
    pub interface_id: usize,
    /// `roof_areas`, for tree supports.
    pub roof_areas: ExPolygons,
    /// `roof_1st_layer`: the layer just below the roof.
    pub roof_1st_layer: ExPolygons,
    /// `floor_areas`.
    pub floor_areas: ExPolygons,
    /// `roof_gap_areas`: the gap between the support roof and the overhang.
    pub roof_gap_areas: ExPolygons,
    /// `area_groups`.
    pub area_groups: Vec<AreaGroup>,
}

impl SupportLayer {
    /// `SupportLayer(id, interface_id, object, height, print_z, slice_z)`.
    #[must_use]
    pub fn new(id: usize, interface_id: usize, height: f64, print_z: f64, slice_z: f64) -> Self {
        Self {
            layer: Layer::new(id, height, print_z, slice_z),
            support_islands: ExPolygons::new(),
            support_fills: ExtrusionEntityCollection::default(),
            support_type: SupportInnerType::Normal,
            base_areas: ExPolygons::new(),
            interface_id,
            roof_areas: ExPolygons::new(),
            roof_1st_layer: ExPolygons::new(),
            floor_areas: ExPolygons::new(),
            roof_gap_areas: ExPolygons::new(),
            area_groups: Vec::new(),
        }
    }

    /// `has_extrusions()` of a `SupportLayer`: some support fill.
    #[must_use]
    pub fn has_extrusions(&self) -> bool {
        !self.support_fills.is_empty()
    }

    /// `interface_id()`.
    #[must_use]
    pub fn interface_id(&self) -> usize {
        self.interface_id
    }
}

/// `SupportLayerPtrs`.
pub type SupportLayerPtrs = Vec<SupportLayer>;

/// `EPSILON` of `libslic3r.h`.
const EPSILON: f64 = 1e-4;

impl Layer {
    /// `empty()`: no region has slices.
    #[must_use]
    pub fn empty(&self) -> bool {
        // (The C++ also skips a null region, which cannot occur here.)
        for layerm in &self.regions {
            if !layerm.slices.empty() {
                // Non empty layer.
                return false;
            }
        }
        true
    }

    /// `add_region(print_region)`: a new empty region of the object's printing region
    /// `print_region`, appended.
    pub fn add_region(&mut self, print_region: usize) -> &mut LayerRegion {
        self.regions.push(LayerRegion::new(print_region));
        let last = self.regions.len() - 1;
        &mut self.regions[last]
    }

    /// `make_slices()`: the regions' slices merged into islands (one region's taken as they are,
    /// several united with the safety offset), in `lslices` in the order `chain_points` gives
    /// their contours' first points.
    ///
    /// # Panics
    /// For an island with an empty contour (the C++ reads its front).
    pub fn make_slices(&mut self) {
        let mut slices: ExPolygons = if self.regions.len() == 1 {
            // Optimization: one region's slices are the islands.
            to_expolygons(&self.regions[0].slices.surfaces)
        } else {
            let mut slices_p = Polygons::new();
            for layerm in &self.regions {
                slices_p.extend(to_polygons_surfaces(&layerm.slices.surfaces));
            }
            union_safety_offset_ex(&slices_p)
        };

        self.lslices.clear();
        self.lslices.reserve(slices.len());

        // The ordering points.
        let mut ordering_points: Points = Points::with_capacity(slices.len());
        for ex in &slices {
            if ex.contour.points.is_empty() {
                not_ported(
                    "Layer::make_slices",
                    "an island with an empty contour (first_point reads its front)",
                );
            }
            ordering_points.push(ex.contour.first_point());
        }

        // Sort the slices.
        let order = chain_points(&ordering_points, None);

        // Populate the slices.
        for i in order {
            self.lslices.push(core::mem::take(&mut slices[i]));
        }
    }

    /// `backup_untyped_slices()`: each region's slices kept as `raw_slices` (`layer_needs_raw_backup`
    /// is always true in Orca: BBS keeps the raw slices for the supports).
    pub fn backup_untyped_slices(&mut self) {
        for layerm in &mut self.regions {
            layerm.raw_slices = to_expolygons(&layerm.slices.surfaces);
        }
    }

    /// `restore_untyped_slices()`: each region's slices set back from `raw_slices`, all internal.
    pub fn restore_untyped_slices(&mut self) {
        for layerm in &mut self.regions {
            layerm
                .slices
                .set_typed(&layerm.raw_slices, SurfaceType::Internal);
        }
    }

    /// `restore_untyped_slices_no_extra_perimeters()`: as [`Layer::restore_untyped_slices`] (BBS
    /// removed its test of `extra_perimeters`, always false).
    pub fn restore_untyped_slices_no_extra_perimeters(&mut self) {
        for layerm in &mut self.regions {
            layerm
                .slices
                .set_typed(&layerm.raw_slices, SurfaceType::Internal);
        }
    }

    /// `merged(offset_scaled)`: the slices of the regions that print something (shells, infill or
    /// walls), offset by `offset_scaled` and united; with no offset, by `EPSILON` and back after
    /// the union. `object` holds the regions' print regions.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn merged(&self, object: &PrintObject, mut offset_scaled: f32) -> ExPolygons {
        // With no offset, an EPSILON offset before the union, reverted after it.
        let mut offset_scaled2: f32 = 0.;
        if offset_scaled == 0.0f32 {
            offset_scaled = EPSILON as f32;
            offset_scaled2 = (-EPSILON) as f32;
        }
        let mut polygons = Polygons::new();
        for layerm in &self.regions {
            let config = layerm.region(object).config();
            // Users make empty volumes to act as subtracters: only a region that prints something
            // is added.
            if config.bottom_shell_layers.value > 0
                || config.top_shell_layers.value > 0
                || config.sparse_infill_density.value > 0.
                || config.wall_loops.value > 0
            {
                polygons.extend(offset_surfaces(&layerm.slices.surfaces, offset_scaled));
            }
        }
        let mut out = union_ex(&polygons, PolyFillType::NonZero);
        if offset_scaled2 != 0.0f32 {
            out =
                offset_ex_expolygons(&out, offset_scaled2, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT);
        }
        out
    }

    /// `get_sparse_infill_max_void_area()` (BBS, a rough estimate): the largest void the sparse
    /// infill of a region leaves, by its pattern, from the infill flow's spacing; -1 when a region
    /// has no sparse infill. `object` is the layer's object and `print` its print.
    ///
    /// # Errors
    /// What a region's infill flow gives.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn get_sparse_infill_max_void_area(
        &self,
        object: &PrintObject,
        print: &Print,
    ) -> Result<f64, FlowError> {
        let mut max_void_area: f64 = 0.;
        for layerm in &self.regions {
            let flow = layerm.flow(self, object, print, FlowRole::Infill)?;
            let density = layerm.region(object).config().sparse_infill_density.value as f32;
            let pattern = layerm.region(object).config().sparse_infill_pattern.value;
            if f64::from(density) == 0. {
                return Ok(-1.);
            }

            // coord_t times float is float, as is the division.
            let spacing = f64::from(flow.scaled_spacing() as f32 * (100.0f32 - density) / density);
            max_void_area = match pattern {
                InfillPattern::ipConcentric
                | InfillPattern::ipRectilinear
                | InfillPattern::ipLine
                | InfillPattern::ipGyroid
                | InfillPattern::ipTpmsD
                | InfillPattern::ipTpmsFK
                | InfillPattern::ipAlignedRectilinear
                | InfillPattern::ipOctagramSpiral
                | InfillPattern::ipHilbertCurve
                | InfillPattern::ipLateralHoneycomb
                | InfillPattern::ip3DHoneycomb
                | InfillPattern::ipArchimedeanChords => {
                    orca_stl::max(max_void_area, spacing * spacing)
                }
                InfillPattern::ipGrid
                | InfillPattern::ipLateralLattice
                | InfillPattern::ipHoneycomb
                | InfillPattern::ipLightning => {
                    orca_stl::max(max_void_area, 4.0 * spacing * spacing)
                }
                InfillPattern::ipCubic
                | InfillPattern::ipAdaptiveCubic
                | InfillPattern::ipTriangles
                | InfillPattern::ipStars
                | InfillPattern::ipSupportCubic => {
                    orca_stl::max(max_void_area, 4.5 * spacing * spacing)
                }
                _ => orca_stl::max(max_void_area, spacing * spacing),
            };
        }
        Ok(max_void_area)
    }

    /// `get_extruder_id(filament_id)`: the print's ([`Print::get_extruder_id`]).
    #[must_use]
    pub fn get_extruder_id(&self, print: &Print, filament_id: u32) -> usize {
        print.get_extruder_id(filament_id)
    }
}

impl LayerRegion {
    /// `flow(role)`: [`LayerRegion::flow_at`] the layer's height.
    ///
    /// # Errors
    /// As [`LayerRegion::flow_at`].
    pub fn flow(
        &self,
        layer: &Layer,
        object: &PrintObject,
        print: &Print,
        role: FlowRole,
    ) -> Result<Flow, FlowError> {
        self.flow_at(layer, object, print, role, layer.height)
    }

    /// `flow(role, layer_height)`: the print region's flow for the role, the first layer's on the
    /// layer of id 0. `layer` holds this region, `object` the layer, `print` the object.
    ///
    /// # Errors
    /// What [`PrintRegion::flow`](crate::print_region::PrintRegion::flow) gives.
    pub fn flow_at(
        &self,
        layer: &Layer,
        object: &PrintObject,
        print: &Print,
        role: FlowRole,
        layer_height: f64,
    ) -> Result<Flow, FlowError> {
        self.region(object)
            .flow(object, print, role, layer_height, layer.id() == 0)
    }

    /// `bridging_flow(role, thick_bridge)`: a thick bridge is a round thread of the bridge width
    /// (or the nozzle) times the root of the bridge flow; a thin one is the role's flow (at the
    /// bridge width when set) with the bridge flow ratio applied, its spacing kept.
    ///
    /// # Errors
    /// "Unknown role" for the support roles; what the flows give.
    #[allow(clippy::cast_possible_truncation)]
    pub fn bridging_flow(
        &self,
        layer: &Layer,
        object: &PrintObject,
        print: &Print,
        role: FlowRole,
        thick_bridge: bool,
    ) -> Result<Flow, FlowError> {
        let region = self.region(object);
        let region_config = region.config();
        // extruder(role) - 1 may underflow to UINT_MAX; get_at then falls back to the first.
        let e = region.extruder(role)?.wrapping_sub(1);
        let nozzle_diameter =
            nozzle_diameter_at(print.config(), e as usize, "LayerRegion::bridging_flow") as f32;
        let bridge_width_opt = &region_config.bridge_line_width;
        let bridge_width = bridge_width_opt.get_abs_value(f64::from(nozzle_diameter));
        let has_bridge_width = bridge_width > 0.;
        let bridge_flow_ratio = region_config.bridge_flow.value;

        let bridge_flow = if thick_bridge {
            // The old Slic3r way: round extrusions.
            let mut thread_diameter = if has_bridge_width {
                bridge_width as f32
            } else {
                nozzle_diameter
            };
            if bridge_flow_ratio > 0. {
                thread_diameter *= orca_crt::sqrt(bridge_flow_ratio) as f32;
            }
            Flow::bridging_flow(thread_diameter, nozzle_diameter)
        } else {
            // As other slicers: normal extrusions, the bridge flow applied keeping the spacing.
            let mut base_flow = self.flow(layer, object, print, role)?;
            if has_bridge_width {
                base_flow = Flow::new(bridge_width as f32, base_flow.height(), nozzle_diameter)?;
            }
            base_flow.with_flow_ratio(bridge_flow_ratio)?
        };
        Ok(bridge_flow)
    }

    /// `slices_to_fill_surfaces_clipped()`: `fill_surfaces` made anew from the slices of each
    /// type in turn, clipped by `fill_expolygons`, typed as the slices were.
    ///
    /// # Panics
    /// For a slice of type `stCount` (the C++ indexes past its array).
    pub fn slices_to_fill_surfaces_clipped(&mut self) {
        // Idempotent: fill_surfaces changes in place, but only its boundaries are used.
        // The surfaces by type.
        let mut by_surface: [Vec<&Surface>; ST_COUNT] = Default::default();
        for surface in &self.slices.surfaces {
            if surface.surface_type == SurfaceType::Count {
                not_ported(
                    "LayerRegion::slices_to_fill_surfaces_clipped",
                    "a slice of type stCount (the C++ indexes past its array)",
                );
            }
            by_surface[surface.surface_type as usize].push(surface);
        }
        // Trim the surfaces by the fill boundaries.
        self.fill_surfaces.surfaces.clear();
        for (surface_type, this_surfaces) in by_surface.iter().enumerate() {
            if !this_surfaces.is_empty() {
                let subject = to_expolygons(this_surfaces.iter().copied());
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let st = SurfaceType::from_i32(surface_type as i32).unwrap_or(SurfaceType::Count);
                self.fill_surfaces.append_typed_owned(
                    intersection_ex_expolygons(
                        &subject,
                        &self.fill_expolygons,
                        ApplySafetyOffset::No,
                    ),
                    st,
                );
            }
        }
    }
}

/// `offset(const Surfaces &, delta)` of ClipperUtils (the default miter join and limit): the
/// offset of the surfaces' islands in order, as `offset(const ExPolygons &, delta)` gives it.
/// Belongs in orca-geom (the Surfaces overloads of ClipperUtils).
fn offset_surfaces(surfaces: &[Surface], delta: f32) -> Polygons {
    offset_expolygons(
        &to_expolygons(surfaces),
        delta,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    )
}

/// `get_extents(const LayerRegion &)`: the box of its slices, the first surface's merged with the
/// others'; undefined for none.
#[must_use]
pub fn get_extents_layer_region(layer_region: &LayerRegion) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    let surfaces = &layer_region.slices.surfaces;
    if !surfaces.is_empty() {
        bbox = surface::get_extents(&surfaces[0]);
        for it in &surfaces[1..] {
            bbox.merge_box(&surface::get_extents(it));
        }
    }
    bbox
}

/// `get_extents(const LayerRegionPtrs &)`: the regions' boxes merged; undefined for none.
#[must_use]
pub fn get_extents_layer_regions(layer_regions: &[LayerRegion]) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    if !layer_regions.is_empty() {
        bbox = get_extents_layer_region(&layer_regions[0]);
        for it in &layer_regions[1..] {
            bbox.merge_box(&get_extents_layer_region(it));
        }
    }
    bbox
}

/// `zs_from_layers(layers)`: each layer's `slice_z` as a float.
#[must_use]
pub fn zs_from_layers<'a, I>(layers: I) -> Vec<f32>
where
    I: IntoIterator<Item = &'a Layer>,
    I::IntoIter: ExactSizeIterator,
{
    let it = layers.into_iter();
    let mut zs = Vec::with_capacity(it.len());
    for l in it {
        zs.push(l.slice_z as f32);
    }
    zs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idx_higher_or_equal_by_halves_and_upwards() {
        let zs = [0.2, 0.4, 0.6, 0.8, 1.0];
        let f = |z: &f64| *z >= 0.55;
        assert_eq!(Layer::idx_higher_or_equal(&zs, usize::MAX, f), 2);
        assert_eq!(Layer::idx_higher_or_equal(&zs, 0, f), 2);
        assert_eq!(Layer::idx_higher_or_equal(&zs, 4, f), 4);
        assert_eq!(
            Layer::idx_higher_or_equal(&zs, usize::MAX, |z: &f64| *z > 2.0),
            5
        );
        assert_eq!(
            Layer::idx_higher_or_equal::<f64>(&[], usize::MAX, |_| true),
            0
        );
    }

    #[test]
    fn a_new_layer_and_its_zs() {
        let mut l = Layer::new(3, 0.2, 0.8, 0.7);
        assert!((l.bottom_z() - 0.6).abs() < 1e-12);
        assert!(!l.has_extrusions());
        l.regions.push(LayerRegion::new(0));
        assert_eq!(l.region_count(), 1);
        assert_eq!(zs_from_layers(&[l]), vec![0.7f32]);
        let s = SupportLayer::new(0, 2, 0.2, 0.2, 0.1);
        assert!(!s.has_extrusions() && s.interface_id() == 2);
    }
}
