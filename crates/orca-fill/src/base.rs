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
// Carries OrcaSlicer code: src/libslic3r/Fill/FillBase.hpp lines 1-235 and src/libslic3r/Fill/
// FillBase.cpp lines 1-39, 105-132, 255-321 and 1243-1262 of OrcaSlicer v2.4.2, translated on
// 2026-11-14 (orca-o3-fill-1). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro Ranellucci),
// under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! [`Fill`], the base of every infill pattern (FillBase.hpp), with the parts of FillBase.cpp that
//! piece fill-1 carries: the anchors' statics, `fill_surface` and `fill_surface_arachne`,
//! `_adjust_solid_spacing`, `_infill_direction` and the two `connect_infill` forms that hand on to
//! the third.
//!
//! - [`FillParams`] is the C++'s plain struct with its defaults; its `config` is a reference to
//!   the region's config, as the C++'s pointer is, so the struct borrows it (`'a`).
//! - `class Fill` is the trait [`Fill`] over the fields of [`FillBase`], which each filler holds
//!   and lends through [`Fill::base`] and [`Fill::base_mut`]. A virtual method is a trait method
//!   whose default body is the C++ base class's; that body is also a free function
//!   ([`fill_surface_base`], [`fill_surface_arachne_base`], [`infill_direction_base`],
//!   [`layer_angle_base`]) so that a filler that overrides the method can still call
//!   `Fill::<method>` as the C++ does. `print_config` and `print_object_config` are references
//!   into the print, borrowed for `'a`.
//! - The C++ names that start with an underscore drop it here: `_fill_surface_single` is
//!   [`Fill::fill_surface_single`] (and, for its `ThickPolylines` overload,
//!   [`Fill::fill_surface_single_thick`]), `_layer_angle` [`Fill::layer_angle`],
//!   `_infill_direction` [`Fill::infill_direction`], `_adjust_solid_spacing`
//!   [`adjust_solid_spacing`].
//! - A filler throws `InfillFailedException`, which every caller of `fill_surface` catches, and
//!   the flow's errors, which go on up: both are [`FillError`]. `fill_surface`,
//!   `fill_surface_arachne`, both `_fill_surface_single` and `fill_surface_extrusion` return it.
//! - `_infill_direction` prints `Using undefined infill angle` on stdout when the angle was never
//!   set (`FLT_MAX`), through [`orca_fmt::stdio::stdout_write`], C's `stdout` for the whole port
//!   (a sink set with `orca_fmt::stdio::set_stdout_sink` takes it instead, as the probe test does;
//!   else the process's stdout, each line feed as CR LF on Windows). It takes the reference point
//!   from the filler's box, or from the surface's contour when the box is empty (an undefined
//!   filler box).
//!   A surface whose contour has no points gives its box's centre, the origin; the C++ reads an
//!   indeterminate box there, and no domain-A surface is empty.
//! - The static members `Fill::connect_infill` (three forms), `connect_base_support` (two forms)
//!   and `_adjust_solid_spacing` are free functions; the overloads take the names
//!   [`connect_infill_expolygon`], [`connect_infill_polygons`] and [`connect_infill`], and
//!   [`connect_base_support`] and [`connect_base_support_polygons`].
//!
//! What stops, and who writes it (crate::stop):
//!
//! - [`Fill::fill_surface_extrusion`]'s default body and the third [`connect_infill`] are fill-2's
//!   (FillBase.cpp 133-191 and 1580-1818), in [`crate::extrusion`] and [`crate::connect`]; the
//!   trait method calls the first, and [`connect_infill`] is the second, used here.
//! - Both [`connect_base_support`] forms stop with the family `infill-supportbase`: their bodies
//!   (FillBase.cpp 1834-2711, no piece's lines) serve the support base pattern alone, and supports
//!   mark a job outside domain A.
//! - `_infill_direction` on a surface of 0 layers' thickness stops: the C++ divides by it.
//!
//! Left out, or written by another piece:
//!
//! - `adapt_fill_octree`, the adaptive cubic infill's octree: only the adaptive and support cubic
//!   fillers read it, and both patterns mark a job outside domain A (their octrees are phase O5's).
//! - `Fill::new_from_type` (both forms) and the static `Fill::use_bridge_flow` (FillBase.cpp
//!   40-104, with the static that calls it at start-up) are fill-4's, since they name every
//!   pattern; `extended_object_bounding_box` ([`FillBase::extended_object_bounding_box`]),
//!   `_create_gap_fill` and `chain_or_connect_infill` are fill-2's, in [`crate::connect`] and
//!   [`crate::extrusion`]; `multiline_fill` (declared in FillBase.hpp) is fill-3's. Each piece
//!   writes the signature where it writes the body.
//! - FillBase.cpp's includes (lines 1-31) bring nothing to translate. The statics
//!   `Fill::infill_anchor` and `Fill::infill_anchor_max` (lines 34-37) are constants here: nothing
//!   in the engine writes them.
//!
//! `LockRegionParam`'s four `std::map`s are vectors of their entries in key order: only
//! `group_fills` fills them, for `ipLockedZag`, which marks a job outside domain A, so in domain A
//! they stay empty.

use core::fmt;

use orca_config::print_config::{InfillPattern, PrintConfig, PrintObjectConfig, PrintRegionConfig};
use orca_geom::bounding_box::BoundingBox;
use orca_geom::clipper_utils::{DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, offset_ex_expolygon};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{Coord, Coordf, EPSILON, PI, Point, scale_};
use orca_geom::polygon::{self, Polygon};
use orca_geom::polyline::{Polylines, ThickPolylines};
use orca_layer::entity::ExtrusionEntitiesPtr;
use orca_layer::flow::{Flow, FlowError};
use orca_layer::role::ExtrusionRole;
use orca_layer::surface::Surface;

use crate::stop::{self, stop};

/// `Fill::infill_anchor`: 400, the anchor's default length, a percentage of the sparse infill's
/// line width ("0% of sparse_infill_line_width" in the C++'s comment). A static `float` the engine
/// never writes.
pub const INFILL_ANCHOR: f32 = 400.;

/// `Fill::infill_anchor_max`: 20 (mm). A static `float` the engine never writes.
pub const INFILL_ANCHOR_MAX: f32 = 20.;

/// What a filler throws: `InfillFailedException` (a `Slic3r::RuntimeError`, "Infill failed"),
/// which every caller of `fill_surface` catches, or another libslic3r exception, which goes on up
/// to `Print::process`'s caller. The second is orca-layer's [`FlowError`]: the flow's own errors,
/// and any `Slic3r::RuntimeError` or `InvalidArgument` by its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillError {
    /// `InfillFailedException`.
    InfillFailed,
    /// Any other exception.
    Other(FlowError),
}

impl FillError {
    /// `InfillFailedException::what()`.
    pub const INFILL_FAILED: &'static str = "Infill failed";

    /// The exception's `what()`.
    #[must_use]
    pub fn what(&self) -> String {
        match self {
            Self::InfillFailed => Self::INFILL_FAILED.to_string(),
            Self::Other(e) => e.what(),
        }
    }
}

impl From<FlowError> for FillError {
    fn from(e: FlowError) -> Self {
        Self::Other(e)
    }
}

impl fmt::Display for FillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what())
    }
}

impl std::error::Error for FillError {}

/// `LockRegionParam`: the locked zag pattern's densities and flows, each with its islands. Each
/// `std::map` is its entries in key order (`float`'s `<`, `Flow`'s `<` by its cross section).
#[derive(Clone, Debug, Default)]
pub struct LockRegionParam {
    /// `skin_density_params`.
    pub skin_density_params: Vec<(f32, ExPolygons)>,
    /// `skeleton_density_params`.
    pub skeleton_density_params: Vec<(f32, ExPolygons)>,
    /// `skin_flow_params`.
    pub skin_flow_params: Vec<(Flow, ExPolygons)>,
    /// `skeleton_flow_params`.
    pub skeleton_flow_params: Vec<(Flow, ExPolygons)>,
}

/// `FillParams`, a plain struct (the C++ asserts it is trivially copyable). Its `config` borrows
/// the region's config.
#[derive(Clone, Copy, Debug)]
pub struct FillParams<'a> {
    /// `density`: the fill density, a fraction in [0, 1].
    pub density: f32,
    /// `multiline`: lines printed for each infill line.
    pub multiline: i32,
    /// `anchor_length`: of an infill anchor along the perimeter (mm).
    pub anchor_length: f32,
    /// `anchor_length_max` (mm).
    pub anchor_length_max: f32,
    /// `resolution`: the G-code resolution.
    pub resolution: f64,
    /// `dont_adjust`: do not adjust the spacing to fill the space evenly.
    pub dont_adjust: bool,
    /// `monotonic`: strictly left to right.
    pub monotonic: bool,
    /// `complete`: complete each loop (honeycomb).
    pub complete: bool,
    /// `use_arachne`: concentric infill by Arachne.
    pub use_arachne: bool,
    /// `layer_height`: for concentric infill by Arachne.
    pub layer_height: Coordf,
    /// `gyroid_optimized`.
    pub gyroid_optimized: bool,
    /// `lateral_lattice_angle_1`.
    pub lateral_lattice_angle_1: Coordf,
    /// `lateral_lattice_angle_2`.
    pub lateral_lattice_angle_2: Coordf,
    /// `pattern`.
    pub pattern: InfillPattern,
    /// `infill_overhang_angle`: for the lateral honeycomb.
    pub infill_overhang_angle: f32,
    /// `flow`.
    pub flow: Flow,
    /// `extrusion_role`.
    pub extrusion_role: ExtrusionRole,
    /// `using_internal_flow`.
    pub using_internal_flow: bool,
    /// `no_extrusion_overlap`.
    pub no_extrusion_overlap: f32,
    /// `config`: the region's config, or none.
    pub config: Option<&'a PrintRegionConfig>,
    /// `dont_sort`: connect the lines without sorting them.
    pub dont_sort: bool,
    /// `can_reverse`.
    pub can_reverse: bool,
    /// `horiz_move`: moves the infill for the cross zag pattern.
    pub horiz_move: f32,
    /// `symmetric_infill_y_axis`.
    pub symmetric_infill_y_axis: bool,
    /// `symmetric_y_axis`.
    pub symmetric_y_axis: Coord,
    /// `locked_zag`.
    pub locked_zag: bool,
    /// `infill_lock_depth`.
    pub infill_lock_depth: f32,
    /// `skin_infill_depth`.
    pub skin_infill_depth: f32,
}

impl Default for FillParams<'_> {
    /// `FillParams{}`: the C++'s member initialisers.
    fn default() -> Self {
        Self {
            density: 0.,
            multiline: 1,
            anchor_length: 1000.,
            anchor_length_max: 1000.,
            resolution: 0.0125,
            dont_adjust: true,
            monotonic: false,
            complete: false,
            use_arachne: false,
            layer_height: 0.,
            gyroid_optimized: false,
            lateral_lattice_angle_1: 0.,
            lateral_lattice_angle_2: 0.,
            pattern: InfillPattern::ipRectilinear,
            infill_overhang_angle: 60.,
            flow: Flow::default(),
            extrusion_role: ExtrusionRole::None,
            using_internal_flow: false,
            no_extrusion_overlap: 0.,
            config: None,
            dont_sort: false,
            can_reverse: true,
            horiz_move: 0.,
            symmetric_infill_y_axis: false,
            symmetric_y_axis: 0,
            locked_zag: false,
            infill_lock_depth: 0.,
            skin_infill_depth: 0.,
        }
    }
}

impl FillParams<'_> {
    /// `full_infill()`: `density > 0.9999f`.
    #[must_use]
    pub fn full_infill(&self) -> bool {
        self.density > 0.9999_f32
    }

    /// `dont_connect()`: `anchor_length_max < 0.05f`; the fill lines are not connected around the
    /// inner perimeter.
    #[must_use]
    pub fn dont_connect(&self) -> bool {
        self.anchor_length_max < 0.05_f32
    }
}

/// The fields of `class Fill`, which every filler holds.
#[derive(Clone, Debug)]
pub struct FillBase<'a> {
    /// `layer_id`: the layer's index, `size_t(-1)` when not set.
    pub layer_id: usize,
    /// `z`: of the top print surface, unscaled.
    pub z: Coordf,
    /// `spacing`: unscaled.
    pub spacing: Coordf,
    /// `overlap`: of the infill and the perimeter, unscaled.
    pub overlap: Coordf,
    /// `angle`: in radians, counter-clockwise, 0 is east; `FLT_MAX` when not set.
    pub angle: f32,
    /// `fixed_angle`: the fill direction is the absolute angle.
    pub fixed_angle: bool,
    /// `link_max_length`: scaled; the longest perimeter segment that connects two infill lines,
    /// 0 for no limit.
    pub link_max_length: Coord,
    /// `loop_clipping`: scaled; the concentric loops' clipping.
    pub loop_clipping: Coord,
    /// `bounding_box`: scaled; of the object's 2D projection.
    pub bounding_box: BoundingBox,
    /// `print_config`: read by the fillers that use Arachne and by the gap fill.
    pub print_config: Option<&'a PrintConfig>,
    /// `print_object_config`: likewise.
    pub print_object_config: Option<&'a PrintObjectConfig>,
    /// `no_overlap_expolygons`: every no-overlap island of the layer.
    pub no_overlap_expolygons: ExPolygons,
    /// `dont_alternate_fill_direction`.
    pub dont_alternate_fill_direction: bool,
}

impl Default for FillBase<'_> {
    /// `Fill()`, the protected constructor: no layer, no spacing or overlap, the angle undefined
    /// (`FLT_MAX`), no link limit or clipping, and the box `BoundingBox(Point(0, 0), Point(-1,
    /// -1))`, which is undefined.
    fn default() -> Self {
        Self {
            layer_id: usize::MAX,
            z: 0.,
            spacing: 0.,
            overlap: 0.,
            angle: f32::MAX,
            fixed_angle: false,
            link_max_length: 0,
            loop_clipping: 0,
            bounding_box: BoundingBox::from_corners(Point::new(0, 0), Point::new(-1, -1)),
            print_config: None,
            print_object_config: None,
            no_overlap_expolygons: ExPolygons::new(),
            dont_alternate_fill_direction: false,
        }
    }
}

impl FillBase<'_> {
    /// `Fill()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `set_bounding_box(bbox)`.
    pub fn set_bounding_box(&mut self, bbox: BoundingBox) {
        self.bounding_box = bbox;
    }
}

/// `class Fill`: an infill pattern over the fields of [`FillBase`]. The default bodies are the
/// C++ base class's.
pub trait Fill<'a> {
    /// The filler's [`FillBase`].
    fn base(&self) -> &FillBase<'a>;

    /// The filler's [`FillBase`], to change.
    fn base_mut(&mut self) -> &mut FillBase<'a>;

    /// `clone()`: a copy of the filler.
    fn clone_box(&self) -> Box<dyn Fill<'a> + 'a>;

    /// `use_bridge_flow()`: whether the fill uses the bridge flow.
    fn use_bridge_flow(&self) -> bool {
        false
    }

    /// `no_sort()`: whether the fill lines are left unsorted.
    fn no_sort(&self) -> bool {
        false
    }

    /// `is_self_crossing()`, pure virtual (non-`const` in the C++; every filler answers a
    /// constant).
    fn is_self_crossing(&self) -> bool;

    /// `has_consistent_pattern()`: whether the pattern is the same from layer to layer.
    fn has_consistent_pattern(&self) -> bool {
        false
    }

    /// `fill_surface(surface, params)`: [`fill_surface_base`].
    ///
    /// # Errors
    /// What the filler throws ([`FillError`]).
    fn fill_surface(
        &mut self,
        surface: &Surface,
        params: &FillParams<'_>,
    ) -> Result<Polylines, FillError> {
        fill_surface_base(self, surface, params)
    }

    /// `fill_surface_arachne(surface, params)`: [`fill_surface_arachne_base`].
    ///
    /// # Errors
    /// What the filler throws ([`FillError`]).
    fn fill_surface_arachne(
        &mut self,
        surface: &Surface,
        params: &FillParams<'_>,
    ) -> Result<ThickPolylines, FillError> {
        fill_surface_arachne_base(self, surface, params)
    }

    /// `set_lock_region_param(lock_param)`: nothing in the base.
    fn set_lock_region_param(&mut self, _lock_param: &LockRegionParam) {}

    /// `fill_surface_extrusion(surface, params, out)`: the fill as extrusions appended to `out`:
    /// [`crate::extrusion::fill_surface_extrusion_base`] (FillBase.cpp 133-191, written by
    /// fill-2).
    ///
    /// # Errors
    /// What escapes the fill ([`FillError`]; it catches `InfillFailedException` itself).
    ///
    /// # Panics
    /// With [`crate::stop::NULL_PRINT_OBJECT_CONFIG`] or [`crate::stop::NULL_REGION_CONFIG`] where
    /// the C++ reads a config through null.
    fn fill_surface_extrusion(
        &mut self,
        surface: &Surface,
        params: &FillParams<'_>,
        out: &mut ExtrusionEntitiesPtr,
    ) -> Result<(), FillError> {
        crate::extrusion::fill_surface_extrusion_base(self, surface, params, out)
    }

    /// `_fill_surface_single(params, thickness_layers, direction, expolygon, polylines_out)`:
    /// fills one region (the region may be changed to avoid a copy); nothing in the base.
    ///
    /// # Errors
    /// What the filler throws ([`FillError`]).
    fn fill_surface_single(
        &mut self,
        _params: &FillParams<'_>,
        _thickness_layers: u32,
        _direction: (f32, Point),
        _expolygon: ExPolygon,
        _polylines_out: &mut Polylines,
    ) -> Result<(), FillError> {
        Ok(())
    }

    /// `_fill_surface_single(params, thickness_layers, direction, expolygon, thick_polylines_out)`,
    /// the overload for the concentric infill's Arachne lines; nothing in the base.
    ///
    /// # Errors
    /// What the filler throws ([`FillError`]).
    fn fill_surface_single_thick(
        &mut self,
        _params: &FillParams<'_>,
        _thickness_layers: u32,
        _direction: (f32, Point),
        _expolygon: ExPolygon,
        _thick_polylines_out: &mut ThickPolylines,
    ) -> Result<(), FillError> {
        Ok(())
    }

    /// `_layer_angle(idx)`: [`layer_angle_base`].
    fn layer_angle(&self, idx: usize) -> f32 {
        layer_angle_base(self.base(), idx)
    }

    /// `_infill_direction(surface)`: [`infill_direction_base`].
    fn infill_direction(&self, surface: &Surface) -> (f32, Point) {
        infill_direction_base(self, surface)
    }
}

/// `Fill::fill_surface` (FillBase.cpp 105-119): the surface's island offset by `overlap - spacing
/// / 2` (scaled, as a `float`), and each region it gives filled by `_fill_surface_single`, with
/// the direction worked out again for each.
///
/// # Errors
/// What `_fill_surface_single` throws; the lines filled before it are dropped.
pub fn fill_surface_base<'a, F: Fill<'a> + ?Sized>(
    f: &mut F,
    surface: &Surface,
    params: &FillParams<'_>,
) -> Result<Polylines, FillError> {
    // Perform offset.
    let delta = scale_(f.base().overlap - 0.5 * f.base().spacing) as f32;
    let expp = offset_ex_expolygon(
        &surface.expolygon,
        delta,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    );
    // Create the infills for each of the regions.
    let mut polylines_out = Polylines::new();
    for expolygon in expp {
        let direction = f.infill_direction(surface);
        f.fill_surface_single(
            params,
            u32::from(surface.thickness_layers),
            direction,
            expolygon,
            &mut polylines_out,
        )?;
    }
    Ok(polylines_out)
}

/// `Fill::fill_surface_arachne` (FillBase.cpp 121-130): as [`fill_surface_base`], into thick
/// polylines through the `ThickPolylines` overload of `_fill_surface_single`.
///
/// # Errors
/// What `_fill_surface_single` throws; the lines filled before it are dropped.
pub fn fill_surface_arachne_base<'a, F: Fill<'a> + ?Sized>(
    f: &mut F,
    surface: &Surface,
    params: &FillParams<'_>,
) -> Result<ThickPolylines, FillError> {
    // Perform offset.
    let delta = scale_(f.base().overlap - 0.5 * f.base().spacing) as f32;
    let expp = offset_ex_expolygon(
        &surface.expolygon,
        delta,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    );
    // Create the infills for each of the regions.
    let mut thick_polylines_out = ThickPolylines::new();
    for expolygon in expp {
        let direction = f.infill_direction(surface);
        f.fill_surface_single_thick(
            params,
            u32::from(surface.thickness_layers),
            direction,
            expolygon,
            &mut thick_polylines_out,
        )?;
    }
    Ok(thick_polylines_out)
}

/// `Fill::_layer_angle(idx)` (FillBase.hpp 211): 0 with a fixed angle, else π/2 (as a `float`)
/// on odd layers and 0 on even ones.
#[must_use]
pub fn layer_angle_base(base: &FillBase<'_>, idx: usize) -> f32 {
    if base.fixed_angle {
        0.
    } else if idx & 1 != 0 {
        (PI / 2.) as f32
    } else {
        0.
    }
}

/// `Fill::_infill_direction(surface)` (FillBase.cpp 273-320): the infill's angle and its reference
/// point.
///
/// The angle is the filler's (0, with `Using undefined infill angle` printed on stdout, when it is
/// `FLT_MAX`), or the surface's bridge angle when that is not negative, or else the filler's plus
/// `_layer_angle(layer_id / thickness_layers)` when the layer is set, the angle not fixed and the
/// direction alternated; then π/2 is added, all in `float`. The reference point is the centre of
/// the filler's box, or of the surface's contour's box when the filler's is empty.
///
/// # Panics
/// With [`crate::stop::ZERO_THICKNESS_LAYERS`] when it would divide by a thickness of 0 layers.
pub fn infill_direction_base<'a, F: Fill<'a> + ?Sized>(f: &F, surface: &Surface) -> (f32, Point) {
    let base = f.base();
    // set infill angle
    let mut out_angle = base.angle;

    if out_angle == f32::MAX {
        orca_fmt::stdio::stdout_write(b"Using undefined infill angle\n");
        out_angle = 0.;
    }

    // The bounding box is only undefined in unit tests.
    let out_shift = if base.bounding_box.is_empty() {
        surface.expolygon.contour.bounding_box().center()
    } else {
        base.bounding_box.center()
    };

    if surface.bridge_angle >= 0. {
        // use bridge angle
        out_angle = surface.bridge_angle as f32;
    } else if base.layer_id != usize::MAX && !base.fixed_angle {
        // alternate fill direction, unless the direction is not to alternate
        if !base.dont_alternate_fill_direction {
            if surface.thickness_layers == 0 {
                stop("Fill::_infill_direction", stop::ZERO_THICKNESS_LAYERS);
            }
            out_angle += f.layer_angle(base.layer_id / usize::from(surface.thickness_layers));
        }
    }

    out_angle += (PI / 2.) as f32;
    (out_angle, out_shift)
}

/// `Fill::_adjust_solid_spacing(width, distance)` (FillBase.cpp 255-271): a spacing that fills
/// `width` with a whole number of lines, the first and last centred at the ends. It may grow the
/// spacing, never shrinks it, and grows it by 20% at most.
///
/// The divisions are `(width - EPSILON) / distance` in `double`, truncated to `coord_t`; the cap is
/// `floor(distance * 1.2 + 0.5)`.
#[must_use]
pub fn adjust_solid_spacing(width: Coord, distance: Coord) -> Coord {
    // floor(width / distance)
    let number_of_intervals = orca_crt::f64_to_i64((width as f64 - EPSILON) / distance as f64);
    let mut distance_new = if number_of_intervals == 0 {
        distance
    } else {
        orca_crt::f64_to_i64((width as f64 - EPSILON) / number_of_intervals as f64)
    };
    let factor = distance_new as Coordf / distance as Coordf;
    // How much could the extrusion width be increased? By 20%.
    let factor_max: Coordf = 1.2;
    if factor > factor_max {
        distance_new = orca_crt::f64_to_i64(orca_crt::floor(distance as Coordf * factor_max + 0.5));
    }
    distance_new
}

/// `Fill::connect_infill(infill_ordered, const ExPolygon &boundary, polylines_out, spacing,
/// params)` (FillBase.cpp 1243-1252): the island's contour and then its holes, with the contour's
/// box, handed to [`connect_infill`].
pub fn connect_infill_expolygon(
    infill_ordered: Polylines,
    boundary_src: &ExPolygon,
    polylines_out: &mut Polylines,
    spacing: f64,
    params: &FillParams<'_>,
) {
    let mut polygons_src: Vec<&Polygon> = Vec::with_capacity(boundary_src.holes.len() + 1);
    polygons_src.push(&boundary_src.contour);
    for polygon in &boundary_src.holes {
        polygons_src.push(polygon);
    }

    connect_infill(
        infill_ordered,
        &polygons_src,
        &polygon::get_extents(&boundary_src.contour),
        polylines_out,
        spacing,
        params,
    );
}

/// `Fill::connect_infill(infill_ordered, const Polygons &boundary, bbox, polylines_out, spacing,
/// params)` (FillBase.cpp 1254-1261): the polygons in order, handed to [`connect_infill`].
pub fn connect_infill_polygons(
    infill_ordered: Polylines,
    boundary_src: &[Polygon],
    bbox: &BoundingBox,
    polylines_out: &mut Polylines,
    spacing: f64,
    params: &FillParams<'_>,
) {
    let polygons_src: Vec<&Polygon> = boundary_src.iter().collect();

    connect_infill(
        infill_ordered,
        &polygons_src,
        bbox,
        polylines_out,
        spacing,
        params,
    );
}

/// `Fill::connect_infill(infill_ordered, const std::vector<const Polygon*> &boundary, bbox,
/// polylines_out, spacing, params)` (FillBase.cpp 1580-1818), the form the other two hand on to:
/// in [`crate::connect`], written by fill-2.
pub use crate::connect::connect_infill;

/// `Fill::connect_base_support(infill_ordered, const std::vector<const Polygon*> &boundary, bbox,
/// polylines_out, spacing, params)` (FillBase.cpp 2247-2700): the support base pattern's lines
/// joined along the boundary.
///
/// # Panics
/// Always, with the family [`crate::stop::INFILL_SUPPORTBASE`]: domain A has no supports.
pub fn connect_base_support(
    _infill_ordered: Polylines,
    _boundary_src: &[&Polygon],
    _bbox: &BoundingBox,
    _polylines_out: &mut Polylines,
    _spacing: f64,
    _params: &FillParams<'_>,
) {
    stop("Fill::connect_base_support", stop::INFILL_SUPPORTBASE)
}

/// `Fill::connect_base_support(infill_ordered, const Polygons &boundary, bbox, polylines_out,
/// spacing, params)` (FillBase.cpp 2702-2710).
///
/// # Panics
/// Always, with the family [`crate::stop::INFILL_SUPPORTBASE`]: domain A has no supports.
pub fn connect_base_support_polygons(
    _infill_ordered: Polylines,
    _boundary_src: &[Polygon],
    _bbox: &BoundingBox,
    _polylines_out: &mut Polylines,
    _spacing: f64,
    _params: &FillParams<'_>,
) {
    stop("Fill::connect_base_support", stop::INFILL_SUPPORTBASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A filler of the test's own that keeps what `_fill_surface_single` is given.
    #[derive(Clone, Default)]
    struct Keep<'a> {
        base: FillBase<'a>,
        regions: Vec<(u32, (f32, Point), ExPolygon)>,
    }

    impl<'a> Fill<'a> for Keep<'a> {
        fn base(&self) -> &FillBase<'a> {
            &self.base
        }
        fn base_mut(&mut self) -> &mut FillBase<'a> {
            &mut self.base
        }
        fn clone_box(&self) -> Box<dyn Fill<'a> + 'a> {
            Box::new(self.clone())
        }
        fn is_self_crossing(&self) -> bool {
            false
        }
        fn fill_surface_single(
            &mut self,
            _params: &FillParams<'_>,
            thickness_layers: u32,
            direction: (f32, Point),
            expolygon: ExPolygon,
            _polylines_out: &mut Polylines,
        ) -> Result<(), FillError> {
            self.regions.push((thickness_layers, direction, expolygon));
            Ok(())
        }
    }

    fn square(size: Coord) -> ExPolygon {
        ExPolygon::new(Polygon::new(vec![
            Point::new(0, 0),
            Point::new(size, 0),
            Point::new(size, size),
            Point::new(0, size),
        ]))
    }

    #[test]
    fn the_defaults_are_the_cpp_initialisers() {
        let p = FillParams::default();
        assert!(!p.full_infill());
        assert!(!p.dont_connect());
        assert_eq!(p.pattern, InfillPattern::ipRectilinear);
        assert!(p.config.is_none() && p.can_reverse && p.dont_adjust);
        let b = FillBase::new();
        assert_eq!(b.layer_id, usize::MAX);
        assert_eq!(b.angle, f32::MAX);
        assert!(!b.bounding_box.defined && b.bounding_box.is_empty());
        assert_eq!(b.bounding_box.max, Point::new(-1, -1));
    }

    #[test]
    fn the_layer_angle_alternates_unless_fixed() {
        let mut b = FillBase::new();
        assert_eq!(layer_angle_base(&b, 0), 0.);
        assert_eq!(layer_angle_base(&b, 3), (PI / 2.) as f32);
        b.fixed_angle = true;
        assert_eq!(layer_angle_base(&b, 3), 0.);
    }

    #[test]
    fn the_direction_takes_the_bridge_angle_or_the_alternating_one() {
        let mut f = Keep::default();
        f.base.angle = 0.25;
        f.base.layer_id = 2;
        f.base.set_bounding_box(BoundingBox::from_corners(
            Point::new(0, 0),
            Point::new(10, 20),
        ));
        let mut s =
            Surface::with_expolygon(orca_layer::surface::SurfaceType::Internal, square(100));
        let quarter = (PI / 2.) as f32;
        // Layer 2 of a one-layer surface: an even index, no turn.
        assert_eq!(f.infill_direction(&s), (0.25 + quarter, Point::new(5, 10)));
        // Two layers thick: index 1, a quarter turn.
        s.thickness_layers = 2;
        assert_eq!(f.infill_direction(&s).0, 0.25 + quarter + quarter);
        s.bridge_angle = 1.;
        assert_eq!(f.infill_direction(&s).0, 1. + quarter);
        // An empty filler box: the contour's centre.
        f.base.bounding_box = BoundingBox::new();
        assert_eq!(f.infill_direction(&s).1, Point::new(50, 50));
    }

    #[test]
    fn a_surface_of_no_layers_stops() {
        let mut f = Keep::default();
        f.base.angle = 0.;
        f.base.layer_id = 1;
        let mut s =
            Surface::with_expolygon(orca_layer::surface::SurfaceType::Internal, square(100));
        s.thickness_layers = 0;
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.infill_direction(&s);
        }))
        .unwrap_err();
        let payload = payload.downcast::<stop::NotPorted>().unwrap();
        assert_eq!(payload.condition, stop::ZERO_THICKNESS_LAYERS);
    }

    #[test]
    fn fill_surface_offsets_and_fills_each_region() {
        let mut f = Keep::default();
        f.base.angle = 0.;
        f.base.spacing = 0.4;
        f.base.layer_id = 0;
        let s = Surface::with_expolygon(
            orca_layer::surface::SurfaceType::Internal,
            square(10_000_000),
        );
        let out = f.fill_surface(&s, &FillParams::default()).unwrap();
        assert!(out.is_empty());
        assert_eq!(f.regions.len(), 1);
        let (layers, direction, region) = &f.regions[0];
        assert_eq!(*layers, 1);
        assert_eq!(direction.0, (PI / 2.) as f32);
        // Shrunk by half the spacing: 0.2 mm on each side.
        let bb = region.contour.bounding_box();
        assert_eq!(bb.min, Point::new(200_000, 200_000));
        assert_eq!(bb.max, Point::new(9_800_000, 9_800_000));
    }

    #[test]
    fn the_solid_spacing_grows_by_a_fifth_at_most() {
        assert_eq!(adjust_solid_spacing(1_000_000, 450_000), 499_999);
        assert_eq!(adjust_solid_spacing(450_000, 450_000), 450_000);
        assert_eq!(adjust_solid_spacing(800_000, 450_000), 540_000);
        assert_eq!(adjust_solid_spacing(0, 450_000), 450_000);
    }

    #[test]
    fn the_connection_hands_on_to_the_third_form() {
        // No lines: nothing to connect, nothing out (fill-2's body, reached through the island's
        // form).
        let mut out = Polylines::new();
        connect_infill_expolygon(
            Polylines::new(),
            &square(100),
            &mut out,
            0.4,
            &FillParams::default(),
        );
        assert!(out.is_empty());
    }
}
