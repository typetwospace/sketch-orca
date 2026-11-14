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
// Carries OrcaSlicer code: src/libslic3r/Fill/FillBase.cpp lines 133-254 of OrcaSlicer v2.4.2,
// translated on 2026-11-14 (orca-o3-fill-2). Copyright the OrcaSlicer authors, the BambuStudio
// authors (Bambu Lab), the PrusaSlicer authors (Prusa Research) and the Slic3r authors (Alessandro
// Ranellucci), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The fill as extrusions (FillBase.cpp): `Fill::fill_surface_extrusion`'s base body
//! ([`fill_surface_extrusion_base`]) and `Fill::_create_gap_fill` ([`create_gap_fill`]).
//!
//! - `fill_surface_extrusion` catches `InfillFailedException` alone ([`FillError::InfillFailed`]);
//!   the flow's errors go on up. With lines, it works the flow out again from the filler's spacing
//!   (a `double` passed as the `float` of `Flow::with_spacing`) unless the internal flow was used,
//!   appends one collection to `out` first and fills it there (so it stays in `out` when an error
//!   escapes later, as the C++'s pointer does), marks it unsorted for the filler that is not sorted
//!   and for top solid infill when `calib_flowrate_topinfill_special_order` is set (read through
//!   `ConfigBase::option` and `getBool`), makes paths (or, with Arachne, `variable_width` lines with
//!   the flow at the filler's spacing), turns them not reversible when the parameters say so or
//!   for that calibration, and runs the gap fill on the collection.
//! - `_create_gap_fill` does nothing without a print object config, when the gap fill target is
//!   nowhere, or on internal solid infill unless the target is everywhere. It takes the no-overlap
//!   islands the collection does not cover (`polygons_covered_by_spacing(10)`), keeps what an
//!   opening by `min / 2` leaves outside an opening-like `offset2_ex(-max / 2, max / 2 + 10)` (each
//!   offset a `float`), orders the gaps by `chain_points` of their contours' first points,
//!   simplifies each by Douglas-Peucker at a tenth of `SCALED_RESOLUTION` and takes its medial
//!   axis between `min` (0.2 times the scaled spacing times `1 - INSET_OVERLAP_TOLERANCE`) and `max`
//!   (twice the scaled spacing). Unless the role is a bridge, the lines shorter than the region's
//!   `filter_out_gap_fill` (scaled) are dropped and the rest appended as gap fill of variable width
//!   with the parameters' flow, only when the density is 1 or more.
//! - The medial axis's exceptions (`Slic3r::InvalidArgument`, `OutOfRange`, `RuntimeError`) go on
//!   up as [`FillError::Other`] with their `what()`: an `InvalidArgument` as orca-layer's
//!   `FlowError::InvalidArgument`, the other two as `FlowError::Runtime`.
//!
//! What stops ([`crate::stop`]): top solid infill on a filler without a print object config
//! ([`crate::stop::NULL_PRINT_OBJECT_CONFIG`]) and the gap fill's filter read from parameters
//! without a region config ([`crate::stop::NULL_REGION_CONFIG`]), where the C++ reads through null.
//!
//! Left out: `Fill::_adjust_solid_spacing` (FillBase.cpp 255-271, at the end of this range) is
//! fill-1's, in [`crate::base`]; the `reserve` calls.

use orca_config::ConfigBase;
use orca_config::print_config::GapFillTarget;
use orca_geom::clipper::PolyFillType;
use orca_geom::clipper_utils::{
    ApplySafetyOffset, CLIPPER_SAFETY_OFFSET, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT,
    diff_ex_expolygons, intersection_ex_expolygons, offset2_ex, opening_ex, union_ex,
    union_ex_expolygons,
};
use orca_geom::expolygon::ExPolygons;
use orca_geom::point::{INSET_OVERLAP_TOLERANCE, Points, SCALED_RESOLUTION, scale_};
use orca_geom::polyline::{Polylines, ThickPolylines};
use orca_layer::collection::ExtrusionEntityCollection;
use orca_layer::entity::{ExtrusionEntitiesPtr, ExtrusionEntity, extrusion_entities_append_paths};
use orca_layer::flow::FlowError;
use orca_layer::role::{ExtrusionRole, is_bridge};
use orca_layer::surface::{Surface, SurfaceType};
use orca_layer::variable_width::variable_width;
use orca_paths::chain_points;
use orca_voronoi::{ExPolygonMedialAxis, VoronoiError};

use crate::base::{Fill, FillBase, FillError, FillParams};
use crate::stop::{self, stop};

/// The key of the flow rate calibration's top infill order.
const CALIB_FLOWRATE_TOPINFILL_SPECIAL_ORDER: &str = "calib_flowrate_topinfill_special_order";

/// A medial axis's exception as what a filler throws, with its `what()`.
fn voronoi_error(e: VoronoiError) -> FillError {
    match e {
        VoronoiError::InvalidArgument(what) => {
            FillError::Other(FlowError::InvalidArgument(what.to_string()))
        }
        VoronoiError::OutOfRange(what) | VoronoiError::RuntimeError(what) => {
            FillError::Other(FlowError::Runtime(what.to_string()))
        }
    }
}

/// `Fill::fill_surface_extrusion(surface, params, out)` (FillBase.cpp 133-191), the base body
/// of [`Fill::fill_surface_extrusion`]: the filler's lines (or Arachne lines) as one collection of
/// extrusions appended to `out`, with the gap fill (see the module's notes). Nothing is appended
/// when the filler gives no lines or throws `InfillFailedException`.
///
/// # Errors
/// What escapes the fill other than `InfillFailedException`, the flow's errors and the gap fill's.
///
/// # Panics
/// With [`crate::stop::NULL_PRINT_OBJECT_CONFIG`] for top solid infill on a filler without a
/// print object config.
pub fn fill_surface_extrusion_base<'a, F: Fill<'a> + ?Sized>(
    f: &mut F,
    surface: &Surface,
    params: &FillParams<'_>,
    out: &mut ExtrusionEntitiesPtr,
) -> Result<(), FillError> {
    let mut polylines = Polylines::new();
    let mut thick_polylines = ThickPolylines::new();
    let filled = if params.use_arachne {
        f.fill_surface_arachne(surface, params)
            .map(|t| thick_polylines = t)
    } else {
        f.fill_surface(surface, params).map(|p| polylines = p)
    };
    match filled {
        Ok(()) | Err(FillError::InfillFailed) => {}
        Err(e) => return Err(e),
    }

    if !polylines.is_empty() || !thick_polylines.is_empty() {
        // calculate actual flow from spacing (which might have been adjusted by the infill
        // pattern generator)
        let mut flow_mm3_per_mm = params.flow.mm3_per_mm()?;
        let mut flow_width = f64::from(params.flow.width());
        if !params.using_internal_flow {
            // (with the internal flow the infill is not solid, so the slight variation that might
            // have been applied to the spacing can be ignored)
            let new_flow = params.flow.with_spacing(f.base().spacing as f32)?;
            flow_mm3_per_mm = new_flow.mm3_per_mm()?;
            flow_width = f64::from(new_flow.width());
        }
        // Save into layer.
        out.push(ExtrusionEntity::Collection(
            ExtrusionEntityCollection::default(),
        ));
        let Some(ExtrusionEntity::Collection(eec)) = out.last_mut() else {
            unreachable!("the collection was just appended")
        };
        // Only concentric fills are not sorted.
        eec.no_sort = f.no_sort();
        // ORCA: special flag for flow rate calibration
        let is_flow_calib = params.extrusion_role == ExtrusionRole::TopSolidInfill && {
            let Some(poc) = f.base().print_object_config else {
                stop(
                    "Fill::fill_surface_extrusion",
                    stop::NULL_PRINT_OBJECT_CONFIG,
                )
            };
            match poc.option(CALIB_FLOWRATE_TOPINFILL_SPECIAL_ORDER) {
                Some(opt) => opt
                    .get_bool()
                    .map_err(|e| FillError::Other(FlowError::Runtime(e.0)))?,
                None => false,
            }
        };
        if is_flow_calib {
            eec.no_sort = true;
        }
        let idx = eec.entities.len();
        if params.use_arachne {
            let new_flow = params.flow.with_spacing(f.base().spacing as f32)?;
            variable_width(
                &thick_polylines,
                params.extrusion_role,
                &new_flow,
                &mut eec.entities,
            )?;
            thick_polylines.clear();
        } else {
            extrusion_entities_append_paths(
                &mut eec.entities,
                &polylines,
                params.extrusion_role,
                flow_mm3_per_mm,
                flow_width as f32,
                params.flow.height(),
                true,
            );
        }
        if !params.can_reverse || is_flow_calib {
            for entity in &mut eec.entities[idx..] {
                entity.set_reverse();
            }
        }

        // Orca: run gap fill
        create_gap_fill(f.base(), surface, params, eec)?;
    }
    Ok(())
}

/// `Fill::_create_gap_fill(surface, params, out)` (FillBase.cpp 195-248): the gaps the
/// collection's extrusions leave in the no-overlap islands, filled with gap fill of variable width
/// appended to `out`, as the print object's gap fill target allows (see the module's notes).
///
/// # Errors
/// The flow's errors and the medial axis's exceptions.
///
/// # Panics
/// With [`crate::stop::NULL_REGION_CONFIG`] when it reads the gap fill's filter from parameters
/// without a region config.
pub fn create_gap_fill(
    base: &FillBase<'_>,
    surface: &Surface,
    params: &FillParams<'_>,
    out: &mut ExtrusionEntityCollection,
) -> Result<(), FillError> {
    //Orca: just to be safe, check against null pointer for the print object config and if NULL
    // return.
    let Some(print_object_config) = base.print_object_config else {
        return Ok(());
    };

    // Orca: Enable gap fill as per the user preference. Return early if gap fill is to not be
    // applied.
    if print_object_config.gap_fill_target.value == GapFillTarget::gftNowhere
        || (surface.surface_type == SurfaceType::InternalSolid
            && print_object_config.gap_fill_target.value != GapFillTarget::gftEverywhere)
    {
        return Ok(());
    }

    let new_flow = params.flow;
    let unextruded_areas: ExPolygons = diff_ex_expolygons(
        &base.no_overlap_expolygons,
        &union_ex(&out.covered_by_spacing(10.)?, PolyFillType::NonZero),
        ApplySafetyOffset::No,
    );
    let mut gapfill_areas = union_ex_expolygons(&unextruded_areas);
    if !base.no_overlap_expolygons.is_empty() {
        gapfill_areas = intersection_ex_expolygons(
            &gapfill_areas,
            &base.no_overlap_expolygons,
            ApplySafetyOffset::No,
        );
    }

    if !gapfill_areas.is_empty() && params.density >= 1. {
        let min = 0.2 * new_flow.scaled_spacing() as f64 * (1. - INSET_OVERLAP_TOLERANCE);
        let max = 2. * new_flow.scaled_spacing() as f64;
        let mut gaps_ex = diff_ex_expolygons(
            &opening_ex(
                &gapfill_areas,
                (min / 2.) as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            &offset2_ex(
                &gapfill_areas,
                -((max / 2.) as f32),
                (max / 2. + f64::from(CLIPPER_SAFETY_OFFSET)) as f32,
                DEFAULT_JOIN_TYPE,
                DEFAULT_MITER_LIMIT,
            ),
            ApplySafetyOffset::No,
        );
        //BBS: sort the gap_ex to avoid mess travel
        let ordering_points: Points = gaps_ex.iter().map(|ex| ex.contour.first_point()).collect();
        let order2 = chain_points(&ordering_points, None);
        let mut gaps_ex_sorted: ExPolygons = Vec::with_capacity(gaps_ex.len());
        for i in order2 {
            gaps_ex_sorted.push(core::mem::take(&mut gaps_ex[i]));
        }

        let mut polylines = ThickPolylines::new();
        for ex in &mut gaps_ex_sorted {
            //BBS: Use DP simplify to avoid duplicated points and accelerate medial-axis
            // calculation as well.
            ex.douglas_peucker(SCALED_RESOLUTION * 0.1);
            ex.medial_axis(min, max, &mut polylines)
                .map_err(voronoi_error)?;
        }

        if !polylines.is_empty() && !is_bridge(params.extrusion_role) {
            let Some(config) = params.config else {
                stop("Fill::_create_gap_fill", stop::NULL_REGION_CONFIG)
            };
            let filter = scale_(config.filter_out_gap_fill.value);
            polylines.retain(|p| {
                let shorter = p.length() < filter;
                !shorter
            });

            let mut gap_fill = ExtrusionEntityCollection::default();
            variable_width(
                &polylines,
                ExtrusionRole::GapFill,
                &params.flow,
                &mut gap_fill.entities,
            )?;
            let gap = core::mem::take(&mut gap_fill.entities);
            out.append_entities(&gap);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use orca_config::print_config::{PrintObjectConfig, PrintRegionConfig};
    use orca_geom::expolygon::ExPolygon;
    use orca_geom::point::{Coord, Point};
    use orca_geom::polygon::Polygon;
    use orca_geom::polyline::Polyline;
    use orca_layer::flow::Flow;

    /// A filler of the test's own: its fill gives the lines it holds, or fails.
    #[derive(Clone, Default)]
    struct Give<'a> {
        base: FillBase<'a>,
        lines: Polylines,
        fails: bool,
    }

    impl<'a> Fill<'a> for Give<'a> {
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
        fn fill_surface(
            &mut self,
            _surface: &Surface,
            _params: &FillParams<'_>,
        ) -> Result<Polylines, FillError> {
            if self.fails {
                return Err(FillError::InfillFailed);
            }
            Ok(self.lines.clone())
        }
    }

    fn rect(w: Coord, h: Coord) -> ExPolygon {
        ExPolygon::new(Polygon::new(vec![
            Point::new(0, 0),
            Point::new(w, 0),
            Point::new(w, h),
            Point::new(0, h),
        ]))
    }

    fn solid(flow: Flow) -> FillParams<'static> {
        FillParams {
            density: 1.,
            flow,
            extrusion_role: ExtrusionRole::SolidInfill,
            ..FillParams::default()
        }
    }

    fn surface(surface_type: SurfaceType) -> Surface {
        Surface::with_expolygon(surface_type, rect(10_000_000, 10_000_000))
    }

    #[test]
    fn no_lines_or_a_failed_fill_append_nothing() {
        let flow = Flow::new(0.45, 0.2, 0.4).unwrap();
        let mut f = Give::default();
        let mut out = ExtrusionEntitiesPtr::new();
        fill_surface_extrusion_base(
            &mut f,
            &surface(SurfaceType::Internal),
            &solid(flow),
            &mut out,
        )
        .unwrap();
        assert!(out.is_empty());
        f.fails = true;
        f.lines = vec![Polyline::new(vec![
            Point::new(0, 0),
            Point::new(1_000_000, 0),
        ])];
        fill_surface_extrusion_base(
            &mut f,
            &surface(SurfaceType::Internal),
            &solid(flow),
            &mut out,
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn the_lines_become_one_collection_of_paths() {
        let flow = Flow::new(0.45, 0.2, 0.4).unwrap();
        let mut f = Give::default();
        f.base.spacing = f64::from(flow.spacing());
        f.lines = vec![
            Polyline::new(vec![Point::new(0, 0), Point::new(5_000_000, 0)]),
            Polyline::new(vec![Point::new(0, 500_000), Point::new(5_000_000, 500_000)]),
            // A line of one point is not valid: no path.
            Polyline::new(vec![Point::new(1, 1)]),
        ];
        let mut out = ExtrusionEntitiesPtr::new();
        let mut params = solid(flow);
        params.can_reverse = false;
        // No print object config: the gap fill returns at once.
        fill_surface_extrusion_base(&mut f, &surface(SurfaceType::Internal), &params, &mut out)
            .unwrap();
        assert_eq!(out.len(), 1);
        let ExtrusionEntity::Collection(eec) = &out[0] else {
            panic!("not a collection")
        };
        assert!(!eec.no_sort);
        assert_eq!(eec.entities.len(), 2);
        assert!(eec.entities.iter().all(|e| !e.can_reverse()));
    }

    #[test]
    fn a_flow_narrower_than_its_height_escapes_before_the_collection() {
        let flow = Flow::new(0.45, 0.3, 0.4).unwrap();
        let mut f = Give::default();
        f.base.spacing = 0.1;
        f.lines = vec![Polyline::new(vec![
            Point::new(0, 0),
            Point::new(1_000_000, 0),
        ])];
        let mut out = ExtrusionEntitiesPtr::new();
        let res = fill_surface_extrusion_base(
            &mut f,
            &surface(SurfaceType::Internal),
            &solid(flow),
            &mut out,
        );
        assert!(matches!(res, Err(FillError::Other(_))));
        assert!(out.is_empty());
    }

    #[test]
    fn top_solid_infill_without_a_print_object_config_stops() {
        let flow = Flow::new(0.45, 0.2, 0.4).unwrap();
        let mut f = Give::default();
        f.base.spacing = f64::from(flow.spacing());
        f.lines = vec![Polyline::new(vec![
            Point::new(0, 0),
            Point::new(1_000_000, 0),
        ])];
        let mut params = solid(flow);
        params.extrusion_role = ExtrusionRole::TopSolidInfill;
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut out = ExtrusionEntitiesPtr::new();
            let _ =
                fill_surface_extrusion_base(&mut f, &surface(SurfaceType::Top), &params, &mut out);
        }))
        .unwrap_err();
        let payload = payload.downcast::<stop::NotPorted>().unwrap();
        assert_eq!(payload.condition, stop::NULL_PRINT_OBJECT_CONFIG);
    }

    #[test]
    fn a_thin_strip_left_open_gets_gap_fill_as_the_target_allows() {
        let flow = Flow::new(0.45, 0.2, 0.4).unwrap();
        let mut object = PrintObjectConfig::default();
        object.gap_fill_target.value = GapFillTarget::gftEverywhere;
        let mut top_bottom = PrintObjectConfig::default();
        top_bottom.gap_fill_target.value = GapFillTarget::gftTopBottom;
        let mut nowhere = PrintObjectConfig::default();
        nowhere.gap_fill_target.value = GapFillTarget::gftNowhere;
        let mut region = PrintRegionConfig::default();
        region.filter_out_gap_fill.value = 0.;
        let mut base = FillBase::new();
        base.print_object_config = Some(&object);
        // A strip 0.5 mm wide and 10 mm long that no extrusion covers.
        base.no_overlap_expolygons = vec![rect(10_000_000, 500_000)];
        let mut params = solid(flow);
        params.config = Some(&region);
        let top = surface(SurfaceType::Top);
        let mut out = ExtrusionEntityCollection::default();
        create_gap_fill(&base, &top, &params, &mut out).unwrap();
        assert!(!out.entities.is_empty());
        assert!(
            out.entities
                .iter()
                .all(|e| e.role() == ExtrusionRole::GapFill)
        );

        // Below full density, on internal solid infill unless everywhere, and nowhere: none.
        let mut half = params;
        half.density = 0.5;
        let mut none = ExtrusionEntityCollection::default();
        create_gap_fill(&base, &top, &half, &mut none).unwrap();
        assert!(none.entities.is_empty());
        let mut other = base.clone();
        other.print_object_config = Some(&top_bottom);
        create_gap_fill(
            &other,
            &surface(SurfaceType::InternalSolid),
            &params,
            &mut none,
        )
        .unwrap();
        assert!(none.entities.is_empty());
        other.print_object_config = Some(&nowhere);
        create_gap_fill(&other, &top, &params, &mut none).unwrap();
        assert!(none.entities.is_empty());
        // A wide island is no gap: the opening and the double offset agree on it.
        base.no_overlap_expolygons = vec![rect(10_000_000, 10_000_000)];
        create_gap_fill(&base, &top, &params, &mut none).unwrap();
        assert!(none.entities.is_empty());
    }
}
