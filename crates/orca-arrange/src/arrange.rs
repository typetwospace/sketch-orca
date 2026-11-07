// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arrange.cpp and Arrange.hpp (CircleBed, InfiniteBed,
// ArrangePolygon, ArrangeParams, fill_config, fixed_overfit, AutoArranger, remove_large_items,
// _arrange, to_nestbin, to_circle, process_arrangeable, call_with_bed and the arrange overloads)
// and Print::is_filaments_compatible of src/libslic3r/Print.cpp, of OrcaSlicer v2.4.2, translated
// on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `Slic3r::arrangement::arrange`: polygons placed on a bed (a box, a circle, a polygon or an
//! infinite bed) and on more plates of it as needed, with libnest2d's first-fit selection and
//! no-fit-polygon placer and Orca's object function.
//!
//! - Each polygon becomes an item: its contour only, clockwise, with its rotation, translation,
//!   inflation and Orca's fields; a contour of fewer than three points is skipped, which shifts
//!   the results written back, as in the C++. Fixed items (the excludes) are shrunk by
//!   `scaled(-2 * EPSILON)`; excludes outside the bed that are not virtual objects are dropped.
//! - The object function (`objfunc`) scores a position by the distance to the pile's centre and
//!   the starting corner, the alignment with items of the same area, the pile's density, the
//!   height differences, filament compatibility and extruder changes; per bin it adds the
//!   squared overfit (a box caps a rejected score at 1.5e7). Once a plate holds fixed items,
//!   `on_preload` switches it to `fixed_overfit`, which adds how much the pile's box grows the
//!   bed's box.
//! - The spatial indexes of the packed items' boxes are [`SpatIndex`]es.
//! - `allow_rotations` needs libnest2d's rotating calipers (`minAreaBoundingBox`,
//!   `fitIntoBoxRotation`), which are not translated: [`ArrangeError::RotationsNotTranslated`],
//!   before anything changes. The command line never rotates.
//! - `ArrangeParams`' callbacks are [`ArrangeCallbacks`]; its default progress callback, which
//!   prints `st=<n>, <name>` to standard output, is [`stdout_progressind`], not a default here.
//! - The ArrangePolygon's `setter` (and `apply`) is the model's (ModelArrange.cpp), not carried
//!   here.

use crate::ArrangeError;
use crate::bg::within_shape_shape;
use crate::firstfit::{SelectionCallbacks, pack_items};
use crate::geometry::{
    Box, Circle, PI, bounding_box_shapes, bounding_box2, convex_hull_shapes, distance,
    is_inside_shape_box, is_inside_shape_circle,
};
use crate::item::{Item, LARGE_COST_TO_REJECT};
use crate::placer::{
    Alignment, Bin, Hooks, PlacerConfig, overfit_box_box, overfit_shape_circle, overfit_shape_shape,
};
use crate::rtree::SpatIndex;
use orca_geom::bounding_box::BoundingBox;
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_geom::point::{Coord, EPSILON, Point, SCALED_EPSILON, scaled};
use orca_geom::polygon::Polygon;
use std::collections::BTreeSet;

/// `BIG_ITEM_TRESHOLD`: an item above this share of the bed's area is big.
pub const BIG_ITEM_TRESHOLD: f64 = 0.02;
/// `UNARRANGED`.
pub const UNARRANGED: i32 = -1;
/// `BED_SHRINK_SEQ_PRINT`: the bed shrink, in millimetres, of a print by object.
pub const BED_SHRINK_SEQ_PRINT: i32 = 5;

/// `FilamentTempType::HighTemp`.
pub const FILAMENT_HIGH_TEMP: i32 = 0;
/// `FilamentTempType::LowTemp`.
pub const FILAMENT_LOW_TEMP: i32 = 1;

/// `Print::is_filaments_compatible(types)`: false when the types hold both a high-temperature
/// and a low-temperature filament.
#[must_use]
pub fn is_filaments_compatible(filament_types: &[i32]) -> bool {
    let mut has_high = false;
    let mut has_low = false;
    for &t in filament_types {
        if t == FILAMENT_HIGH_TEMP {
            has_high = true;
        } else if t == FILAMENT_LOW_TEMP {
            has_low = true;
        }
    }
    !(has_high && has_low)
}

/// `CircleBed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleBed {
    /// `center()`.
    pub center: Point,
    /// `radius()`: NaN for a bed that is not a circle.
    pub radius: f64,
}

impl Default for CircleBed {
    /// `CircleBed()`: at the origin, radius NaN.
    fn default() -> CircleBed {
        CircleBed {
            center: Point::new(0, 0),
            radius: f64::NAN,
        }
    }
}

/// `InfiniteBed`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InfiniteBed {
    /// `center`.
    pub center: Point,
}

/// The bed shapes `arrange` takes.
#[derive(Clone, Debug, PartialEq)]
pub enum Bed {
    /// A rectangle, `BoundingBox`.
    Box(BoundingBox),
    /// `CircleBed`.
    Circle(CircleBed),
    /// An irregular bed, `Polygon`.
    Polygon(Polygon),
    /// `InfiniteBed`.
    Infinite(InfiniteBed),
}

/// `ArrangePolygon`, without its `setter`.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangePolygon {
    /// `poly`: the silhouette; only its contour is arranged.
    pub poly: ExPolygon,
    /// `translation`.
    pub translation: Point,
    /// `rotation`, in radians.
    pub rotation: f64,
    /// `inflation`.
    pub inflation: Coord,
    /// `bed_idx`: `UNARRANGED` until arranged.
    pub bed_idx: i32,
    /// `priority`.
    pub priority: i32,
    /// `locked_plate`.
    pub locked_plate: i32,
    /// `is_virt_object`.
    pub is_virt_object: bool,
    /// `is_extrusion_cali_object`.
    pub is_extrusion_cali_object: bool,
    /// `is_wipe_tower`.
    pub is_wipe_tower: bool,
    /// `has_tree_support`.
    pub has_tree_support: bool,
    /// `row`.
    pub row: i32,
    /// `col`.
    pub col: i32,
    /// `extrude_ids`.
    pub extrude_ids: Vec<i32>,
    /// `filament_temp_type`: -1 unset.
    pub filament_temp_type: i32,
    /// `bed_temp`.
    pub bed_temp: i32,
    /// `print_temp`.
    pub print_temp: i32,
    /// `first_bed_temp`.
    pub first_bed_temp: i32,
    /// `first_print_temp`.
    pub first_print_temp: i32,
    /// `vitrify_temp`.
    pub vitrify_temp: i32,
    /// `itemid`.
    pub itemid: i32,
    /// `is_applied`.
    pub is_applied: i32,
    /// `height`.
    pub height: f64,
    /// `brim_width`.
    pub brim_width: f64,
    /// `name`.
    pub name: String,
    /// `allowed_rotations`.
    pub allowed_rotations: Vec<f64>,
}

impl Default for ArrangePolygon {
    fn default() -> ArrangePolygon {
        ArrangePolygon {
            poly: ExPolygon::default(),
            translation: Point::new(0, 0),
            rotation: 0.0,
            inflation: 0,
            bed_idx: UNARRANGED,
            priority: 0,
            locked_plate: -1,
            is_virt_object: false,
            is_extrusion_cali_object: false,
            is_wipe_tower: false,
            has_tree_support: false,
            row: 0,
            col: 0,
            extrude_ids: Vec::new(),
            filament_temp_type: -1,
            bed_temp: 0,
            print_temp: 0,
            first_bed_temp: 0,
            first_print_temp: 0,
            vitrify_temp: 0,
            itemid: 0,
            is_applied: 0,
            height: 0.0,
            brim_width: 0.0,
            name: String::new(),
            allowed_rotations: vec![0.0],
        }
    }
}

impl ArrangePolygon {
    /// `is_arranged()`.
    #[must_use]
    pub fn is_arranged(&self) -> bool {
        self.bed_idx != UNARRANGED
    }

    /// `transformed_poly()`: the polygon rotated, then translated.
    #[must_use]
    pub fn transformed_poly(&self) -> ExPolygon {
        let mut ret = self.poly.clone();
        ret.rotate(self.rotation);
        ret.translate_f64(self.translation.x as f64, self.translation.y as f64);
        ret
    }
}

/// `ArrangeParams`, without its callbacks (see [`ArrangeCallbacks`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeParams {
    /// `min_obj_distance`.
    pub min_obj_distance: Coord,
    /// `accuracy`.
    pub accuracy: f32,
    /// `parallel`.
    pub parallel: bool,
    /// `allow_rotations`.
    pub allow_rotations: bool,
    /// `do_final_align`.
    pub do_final_align: bool,
    /// `allow_multi_materials_on_same_plate`.
    pub allow_multi_materials_on_same_plate: bool,
    /// `avoid_extrusion_cali_region`.
    pub avoid_extrusion_cali_region: bool,
    /// `is_seq_print`.
    pub is_seq_print: bool,
    /// `align_to_y_axis`.
    pub align_to_y_axis: bool,
    /// `bed_shrink_x`.
    pub bed_shrink_x: f32,
    /// `bed_shrink_y`.
    pub bed_shrink_y: f32,
    /// `brim_skirt_distance`.
    pub brim_skirt_distance: f32,
    /// `clearance_height_to_rod`.
    pub clearance_height_to_rod: f32,
    /// `clearance_height_to_lid`.
    pub clearance_height_to_lid: f32,
    /// `clearance_radius`.
    pub clearance_radius: f32,
    /// `object_skirt_offset`.
    pub object_skirt_offset: f32,
    /// `nozzle_height`.
    pub nozzle_height: f32,
    /// `printable_height`.
    pub printable_height: f32,
    /// `align_center` (`Vec2d`).
    pub align_center: (f64, f64),
    /// `excluded_regions`: areas nothing may use.
    pub excluded_regions: Vec<ArrangePolygon>,
    /// `nonprefered_regions`: areas used last.
    pub nonprefered_regions: Vec<ArrangePolygon>,
}

impl Default for ArrangeParams {
    fn default() -> ArrangeParams {
        ArrangeParams {
            min_obj_distance: 0,
            accuracy: 1.0,
            parallel: true,
            allow_rotations: false,
            do_final_align: true,
            allow_multi_materials_on_same_plate: true,
            avoid_extrusion_cali_region: true,
            is_seq_print: false,
            align_to_y_axis: false,
            bed_shrink_x: 1.0,
            bed_shrink_y: 1.0,
            brim_skirt_distance: 0.0,
            clearance_height_to_rod: 0.0,
            clearance_height_to_lid: 0.0,
            clearance_radius: 0.0,
            object_skirt_offset: 0.0,
            nozzle_height: 0.0,
            printable_height: 256.0,
            align_center: (0.5, 0.5),
            excluded_regions: Vec::new(),
            nonprefered_regions: Vec::new(),
        }
    }
}

/// `progressind(num_finished, name)`.
pub type ProgressInd<'a> = dyn FnMut(u32, &str) + 'a;

/// `ArrangeParams`' callbacks: `progressind`, `on_packed` (given the packed item's bed and
/// priority, all the C++ sets of its `ArrangePolygon`) and `stopcondition`. None by default; the
/// C++ default progress callback is [`stdout_progressind`].
#[derive(Default)]
pub struct ArrangeCallbacks<'a> {
    /// `progressind(num_finished, name)`.
    pub progressind: Option<&'a mut ProgressInd<'a>>,
    /// `on_packed(ap)`, with `ap.bed_idx` and `ap.priority`.
    pub on_packed: Option<&'a mut dyn FnMut(i32, i32)>,
    /// `stopcondition()`.
    pub stopcondition: Option<&'a mut dyn FnMut() -> bool>,
}

/// `ArrangeParams`' default `progressind`: `std::cout << "st=" << st << ", " << str << std::endl`.
pub fn stdout_progressind(st: u32, name: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "st={st}, {name}");
    let _ = out.flush();
}

/// The object functions of a placer's configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObjFn {
    /// `get_objfn()`'s function for the bin, with its packing origin.
    Main {
        /// `origin_pack`.
        origin_pack: Point,
    },
    /// `on_preload`'s: `fixed_overfit(objfunc(item, starting_point), binbb)`.
    FixedOverfit {
        /// `binbb`.
        binbb: Box,
        /// `starting_point`.
        starting_point: Point,
    },
}

/// `fixed_overfit(result, binbb)`: the score, plus how much the pile's box grows the bed's box
/// (the areas in 64-bit integers, then `double`).
#[must_use]
pub fn fixed_overfit(result: (f64, Box), binbb: &Box) -> f64 {
    let (mut score, pilebb) = result;
    let fullbb = bounding_box2(&pilebb, binbb);
    let diff = fullbb.area_i64() as f64 - binbb.area_i64() as f64;
    if diff > 0.0 {
        score += diff;
    }
    score
}

/// `fill_config(pcfg, params)`, the excluded regions returned as items.
fn fill_config(pcfg: &mut PlacerConfig<ObjFn>, params: &ArrangeParams) -> Vec<Item> {
    pcfg.starting_point = if params.is_seq_print {
        Alignment::BottomLeft
    } else {
        Alignment::TopRight
    };
    pcfg.alignment = if params.do_final_align {
        Alignment::Center
    } else {
        Alignment::DontAlign
    };
    pcfg.rotations = if params.allow_rotations {
        vec![0.0, PI / 4.0, PI / 2.0, 3.0 * PI / 4.0]
    } else {
        vec![0.0]
    };
    pcfg.accuracy = params.accuracy;
    pcfg.parallel = params.parallel;
    let mut excluded = Vec::new();
    for poly in &params.excluded_regions {
        process_arrangeable(poly, &mut excluded);
    }
    for itm in &mut excluded {
        itm.mark_as_fixed_in_bin(0);
        itm.inflate(scaled(-2.0 * EPSILON));
    }
    excluded
}

/// `AutoArranger<TBin>`: the hooks of the placers and the selection.
pub struct AutoArranger<'p> {
    bin: Bin,
    bin_area: f64,
    norm: f64,
    pconf: PlacerConfig<ObjFn>,
    params: &'p ArrangeParams,
    rtree: SpatIndex,
    smallsrtree: SpatIndex,
    merged_pile: ExPolygons,
    pilebb: Box,
    remaining: Vec<usize>,
    items: Vec<usize>,
    item_count: usize,
    excluded_regions: Vec<Item>,
}

/// `objfunc`'s cases (`BIG_ITEM`, `LAST_BIG_ITEM`, `SMALL_ITEM`).
enum Case {
    Big,
    LastBig,
    Small,
}

impl<'p> AutoArranger<'p> {
    /// `AutoArranger(bin, params, ...)`.
    #[must_use]
    pub fn new(bin: Bin, params: &'p ArrangeParams) -> AutoArranger<'p> {
        let bin_area = orca_crt::fabs(bin.area());
        let norm = orca_crt::sqrt(bin_area);
        let mut pconf = PlacerConfig::new(ObjFn::Main {
            origin_pack: Point::new(0, 0),
        });
        let excluded_regions = fill_config(&mut pconf, params);
        if orca_crt::fabs(params.align_center.0 - 0.5) > 0.001
            || orca_crt::fabs(params.align_center.1 - 0.5) > 0.001
        {
            let binbb = bin.bounding_box();
            let off = Point::new(
                orca_crt::f64_to_i64(orca_crt::round(
                    binbb.width() as f64 * params.align_center.0,
                )),
                orca_crt::f64_to_i64(orca_crt::round(
                    binbb.height() as f64 * params.align_center.1,
                )),
            );
            pconf.best_object_pos = binbb.min + off;
            pconf.alignment = Alignment::UserDefined;
        }
        let mut a = AutoArranger {
            bin,
            bin_area,
            norm,
            pconf,
            params,
            rtree: SpatIndex::new(),
            smallsrtree: SpatIndex::new(),
            merged_pile: ExPolygons::new(),
            pilebb: Box::default(),
            remaining: Vec::new(),
            items: Vec::new(),
            item_count: 0,
            excluded_regions,
        };
        a.pconf.object_function = a.get_objfn();
        a
    }

    /// `get_objfn()`: the bin's function, with its packing origin.
    fn get_objfn(&self) -> ObjFn {
        let sp = self.pconf.starting_point;
        let origin_pack = match &self.bin {
            Bin::Box(b) => {
                if sp == Alignment::Center {
                    b.center()
                } else if sp == Alignment::TopRight {
                    b.max
                } else {
                    b.min
                }
            }
            other => {
                let bb = other.bounding_box();
                if sp == Alignment::Center {
                    bb.center()
                } else {
                    bb.min
                }
            }
        };
        ObjFn::Main { origin_pack }
    }

    /// `norm(val)`: `double(val) / m_norm`.
    fn norm(&self, val: f64) -> f64 {
        val / self.norm
    }

    /// `isBig(area)`.
    fn is_big(&self, a: f64) -> bool {
        a / self.bin_area > BIG_ITEM_TRESHOLD
    }

    /// `dist_for_BOTTOM_LEFT(ibb, origin_pack)`.
    fn dist_for_bottom_left(&self, ibb: &Box, origin_pack: Point) -> f64 {
        let dist_corner_y = ibb.min.y.wrapping_sub(origin_pack.y) as f64;
        let dist_corner_x = ibb.min.x.wrapping_sub(origin_pack.x) as f64;
        let mut bindist = ibb.max.y.wrapping_sub(ibb.min.y) as f64;
        if dist_corner_x >= 0.0 && dist_corner_y >= 0.0 {
            bindist += dist_corner_y + 1.0 * dist_corner_x;
        } else {
            if dist_corner_x < 0.0 {
                bindist += 10.0 * (-dist_corner_x);
            }
            if dist_corner_y < 0.0 {
                bindist += 10.0 * (-dist_corner_y);
            }
        }
        self.norm(bindist)
    }

    /// `dist_to_bin(ibb, origin_pack, starting_point)`.
    fn dist_to_bin(&self, ibb: &Box, origin_pack: Point, sp: Alignment) -> f64 {
        if sp == Alignment::BottomLeft {
            self.norm(distance(ibb.min, origin_pack))
        } else if sp == Alignment::TopRight {
            self.norm(distance(ibb.max, origin_pack))
        } else {
            self.norm(distance(ibb.center(), origin_pack))
        }
    }

    /// `objfunc(item, origin_pack)`: the score and the box of the pile with the item.
    #[allow(clippy::too_many_lines)]
    fn objfunc(&self, arena: &[Item], item: &Item, origin_pack: Point) -> (f64, Box) {
        let ibb = item.bounding_box();
        let fullbb = bounding_box2(&self.pilebb, &ibb);
        let bigbb = if self.rtree.is_empty() {
            fullbb
        } else {
            self.rtree.bounds()
        };
        let mut score: f64;
        let bigitems = self.is_big(item.area()) || self.rtree.is_empty();
        let case = if !self.params.is_seq_print && bigitems && !self.remaining.is_empty() {
            Case::Big
        } else if bigitems && self.remaining.is_empty() {
            Case::LastBig
        } else {
            Case::Small
        };
        let sp = self.pconf.starting_point;
        match case {
            Case::Big => {
                let minc = ibb.min;
                let maxc = ibb.max;
                let top_left = Point::new(minc.x, maxc.y);
                let bottom_right = Point::new(maxc.x, minc.y);
                let cc = fullbb.center();
                let dists = [
                    distance(minc, cc),
                    distance(maxc, cc),
                    distance(ibb.center(), cc),
                    distance(top_left, cc),
                    distance(bottom_right, cc),
                ];
                let mut dist = self.norm(dists[orca_stl::min_element_f64(&dists)]);
                if sp == Alignment::BottomLeft {
                    let bindist = self.dist_for_bottom_left(&ibb, origin_pack);
                    score = 0.2 * dist + 0.8 * bindist;
                } else {
                    let bindist = self.dist_to_bin(&ibb, origin_pack, sp);
                    dist = 0.8 * dist + 0.2 * bindist;
                    let mut alignment_score = 1.0;
                    let index = if self.is_big(item.area()) {
                        &self.rtree
                    } else {
                        &self.smallsrtree
                    };
                    let result = index.query_intersects(&ibb);
                    for e in &result {
                        let p = &arena[self.items[e.1 as usize]];
                        let parea = p.area();
                        if orca_crt::fabs(1.0 - parea / item.area()) < 1e-6 {
                            let bb = bounding_box2(&p.bounding_box(), &ibb);
                            let bbarea = bb.area_i64();
                            let ascore = 1.0 - (item.area() + parea) / bbarea as f64;
                            if ascore < alignment_score {
                                alignment_score = ascore;
                            }
                        }
                    }
                    let density = orca_crt::sqrt(
                        self.norm(fullbb.width() as f64) * self.norm(fullbb.height() as f64),
                    );
                    let r = self.remaining.len() as f64 / self.item_count as f64;
                    let alignment_weight =
                        orca_stl::max(0.3, 0.6 * item.area() / ibb.area_i64() as f64);
                    score = if result.is_empty() {
                        0.50 * dist + 0.50 * density
                    } else {
                        (1.0 - 0.2 - alignment_weight) * dist
                            + (1.0 - r) * 0.20 * density
                            + alignment_weight * alignment_score
                    };
                }
            }
            Case::LastBig => {
                score = if sp == Alignment::BottomLeft {
                    self.dist_for_bottom_left(&ibb, origin_pack)
                } else if self.pilebb.defined {
                    0.5 * self.norm(distance(ibb.center(), self.pilebb.center()))
                } else {
                    0.5 * self.norm(distance(ibb.center(), origin_pack))
                };
            }
            Case::Small => {
                score = if sp == Alignment::BottomLeft {
                    self.dist_for_bottom_left(&ibb, origin_pack)
                } else {
                    0.8 * self.norm(distance(ibb.center(), bigbb.center()))
                        + 0.2 * self.norm(distance(ibb.center(), origin_pack))
                };
            }
        }

        if self.params.is_seq_print {
            let clearance_height_to_lid = f64::from(self.params.clearance_height_to_lid);
            let clearance_height_to_rod = f64::from(self.params.clearance_height_to_rod);
            let mut has_row_height_conflict = false;
            let mut has_lid_height_conflict = false;
            let iy1 = item.bounding_box().min.y;
            let iy2 = item.bounding_box().max.y;
            let ix1 = item.bounding_box().min.x;
            for &pi in &self.items {
                let p = &arena[pi];
                if p.is_virt_object {
                    continue;
                }
                let pbb = p.bounding_box();
                let px1 = pbb.min.x;
                let py1 = pbb.min.y;
                let py2 = pbb.max.y;
                let inter_min = orca_stl::max(iy1, py1);
                let inter_max = orca_stl::min(iy2, py2);
                if iy2 < py1 {
                    has_row_height_conflict |= item.height > clearance_height_to_rod;
                } else if inter_max.wrapping_sub(inter_min) > 0 {
                    let h = if ix1 < px1 { item.height } else { p.height };
                    has_row_height_conflict |= h > clearance_height_to_rod;
                }
                has_lid_height_conflict |= p.height > clearance_height_to_lid;
            }
            let lambda3 = LARGE_COST_TO_REJECT * 1.1;
            let lambda4 = LARGE_COST_TO_REJECT * 1.2;
            for &pi in &self.items {
                let p = &arena[pi];
                if p.is_virt_object {
                    continue;
                }
                if !is_filaments_compatible(&[item.filament_temp_type, p.filament_temp_type]) {
                    score += lambda3;
                }
            }
            score += lambda4 * f64::from(u8::from(has_row_height_conflict))
                + lambda4 * f64::from(u8::from(has_lid_height_conflict));
        } else {
            let mut valid_items_cnt: i32 = 0;
            let mut height_score = 0.0;
            let printable_height = f64::from(self.params.printable_height);
            for &pi in &self.items {
                let p = &arena[pi];
                if !p.is_virt_object {
                    valid_items_cnt += 1;
                    height_score += (1.0
                        - orca_crt::fabs(item.height - p.height) / printable_height)
                        * self.norm(distance(ibb.center(), p.bounding_box().center()));
                    if !is_filaments_compatible(&[item.filament_temp_type, p.filament_temp_type]) {
                        score += LARGE_COST_TO_REJECT;
                        break;
                    }
                }
            }
            if valid_items_cnt > 0 {
                score += height_score / f64::from(valid_items_cnt);
            }
        }

        let mut extruder_ids: BTreeSet<i32> = BTreeSet::new();
        for &pi in &self.items {
            let p = &arena[pi];
            if p.is_virt_object {
                continue;
            }
            extruder_ids.extend(p.extrude_ids.iter().copied());
        }
        if !self.params.allow_multi_materials_on_same_plate {
            let first_object = extruder_ids.is_empty();
            let item_extruder_ids: BTreeSet<i32> = item.extrude_ids.iter().copied().collect();
            let same_color_with_previous_items = extruder_ids.is_subset(&item_extruder_ids)
                || item_extruder_ids.is_subset(&extruder_ids);
            if !(first_object || same_color_with_previous_items) {
                score += LARGE_COST_TO_REJECT * 1.3;
            }
        }
        let last_extruder_cnt = extruder_ids.len() as i32;
        extruder_ids.extend(item.extrude_ids.iter().copied());
        let new_extruder_cnt = extruder_ids.len() as i32;
        if !self.params.is_seq_print {
            score += f64::from(new_extruder_cnt - last_extruder_cnt);
        }
        (score, fullbb)
    }

    /// The function `get_objfn()` returns, for the bin.
    fn main_objective(
        &self,
        arena: &[Item],
        item: &Item,
        origin_pack: Point,
    ) -> Result<f64, ArrangeError> {
        let (mut score, fullbb) = self.objfunc(arena, item, origin_pack);
        match &self.bin {
            Bin::Box(b) => {
                let mut miss = overfit_box_box(&fullbb, b);
                miss = if miss > 0.0 { miss } else { 0.0 };
                score += miss * miss;
                if score > LARGE_COST_TO_REJECT {
                    score = 1.5 * LARGE_COST_TO_REJECT;
                }
            }
            Bin::Circle(c) => {
                if self.is_big(item.area()) {
                    let mut mp = self.merged_pile.clone();
                    mp.push(item.transformed_shape().clone());
                    let chull = convex_hull_shapes(&mp);
                    let mut miss = overfit_shape_circle(&chull, c)?;
                    if miss < 0.0 {
                        miss = 0.0;
                    }
                    score += miss * miss;
                }
            }
            Bin::Shape(s) => {
                let mut mp = self.merged_pile.clone();
                mp.push(item.transformed_shape().clone());
                let chull = convex_hull_shapes(&mp);
                if self.pconf.starting_point == Alignment::BottomLeft {
                    if !within_shape_shape(&chull, s) {
                        score += LARGE_COST_TO_REJECT;
                    }
                } else {
                    let mut miss = overfit_shape_shape(&chull, s);
                    miss = if miss > 0.0 { miss } else { 0.0 };
                    score += miss * miss;
                }
            }
        }
        Ok(score)
    }

    /// `preload(fixeditems)`: each marked fixed in its bin (fixed when it is not negative).
    fn preload(&mut self, arena: &mut [Item], fixed: std::ops::Range<usize>) {
        for itm in &mut arena[fixed.clone()] {
            let b = itm.bin_id();
            itm.mark_as_fixed_in_bin(b);
        }
        self.item_count += fixed.len();
    }
}

impl Hooks for AutoArranger<'_> {
    type ObjFn = ObjFn;

    fn object_function(&self, arena: &[Item], f: &ObjFn, item: &Item) -> Result<f64, ArrangeError> {
        match *f {
            ObjFn::Main { origin_pack } => self.main_objective(arena, item, origin_pack),
            ObjFn::FixedOverfit {
                binbb,
                starting_point,
            } => Ok(fixed_overfit(
                self.objfunc(arena, item, starting_point),
                &binbb,
            )),
        }
    }

    fn before_packing(
        &mut self,
        arena: &[Item],
        merged_pile: &[ExPolygon],
        packed: &[usize],
        remaining: &[usize],
    ) {
        self.items = packed.to_vec();
        self.merged_pile = merged_pile.to_vec();
        self.remaining = remaining.to_vec();
        self.pilebb.defined = false;
        if !merged_pile.is_empty() {
            self.pilebb = bounding_box_shapes(merged_pile);
            self.pilebb.defined = true;
        }
        self.rtree.clear();
        self.smallsrtree.clear();
        for (idx, &i) in packed.iter().enumerate() {
            let itm = &arena[i];
            if itm.is_virt_object {
                continue;
            }
            if self.is_big(itm.area()) {
                self.rtree.insert((itm.bounding_box(), idx as u32));
            }
            self.smallsrtree.insert((itm.bounding_box(), idx as u32));
        }
    }

    fn on_preload(&self, arena: &[Item], items: &[usize], cfg: &mut PlacerConfig<ObjFn>) {
        if items.is_empty() {
            return;
        }
        let binbb = self.bin.bounding_box();
        let mut starting_point = if cfg.starting_point == Alignment::BottomLeft {
            binbb.min
        } else {
            binbb.center()
        };
        for &i in items {
            if arena[i].is_wipe_tower {
                starting_point = arena[i].bounding_box().center();
                break;
            }
        }
        cfg.object_function = ObjFn::FixedOverfit {
            binbb,
            starting_point,
        };
    }

    #[allow(clippy::float_cmp)]
    fn sort_less(&self, i1: &Item, i2: &Item) -> bool {
        let (p1, p2) = (i1.priority(), i2.priority());
        if p1 != p2 {
            return p1 > p2;
        }
        if self.params.is_seq_print {
            if i1.bed_temp != i2.bed_temp {
                i1.bed_temp > i2.bed_temp
            } else if i1.height != i2.height {
                i1.height < i2.height
            } else if orca_crt::fabs(i1.area() / i2.area() - 1.0) > 0.2 {
                i1.area() > i2.area()
            } else {
                front(&i1.extrude_ids) < front(&i2.extrude_ids)
            }
        } else if i1.bed_temp != i2.bed_temp {
            i1.bed_temp > i2.bed_temp
        } else if i1.extrude_ids != i2.extrude_ids {
            front(&i1.extrude_ids) < front(&i2.extrude_ids)
        } else {
            i1.area() > i2.area()
        }
    }
}

/// `extrude_ids.front()`.
///
/// # Panics
///
/// For no extruders, where the C++ dereferences an empty vector.
fn front(v: &[i32]) -> i32 {
    assert!(
        !v.is_empty(),
        "extrude_ids.front() of an item without extruders"
    );
    v[0]
}

/// `process_arrangeable(arrpoly, outp)`: the contour, clockwise, as an item with the polygon's
/// fields; nothing for fewer than three points.
fn process_arrangeable(arrpoly: &ArrangePolygon, outp: &mut Vec<Item>) {
    let mut p = arrpoly.poly.contour.clone();
    if p.is_counter_clockwise() {
        p.reverse();
    }
    if p.points.len() < 3 {
        return;
    }
    let mut item = Item::new(ExPolygon::new(p));
    item.set_rotation(arrpoly.rotation);
    item.set_translation(arrpoly.translation);
    item.set_bin_id(arrpoly.bed_idx);
    item.set_priority(arrpoly.priority);
    item.itemid = arrpoly.itemid;
    item.extrude_ids = arrpoly.extrude_ids.clone();
    item.height = arrpoly.height;
    item.name = arrpoly.name.clone();
    item.is_virt_object = arrpoly.is_virt_object;
    item.is_wipe_tower = arrpoly.is_wipe_tower;
    item.bed_temp = f64::from(arrpoly.first_bed_temp);
    item.print_temp = f64::from(arrpoly.print_temp);
    item.vitrify_temp = f64::from(arrpoly.vitrify_temp);
    item.set_inflation(arrpoly.inflation);
    item.filament_temp_type = arrpoly.filament_temp_type;
    outp.push(item);
}

/// `remove_large_items(items, bin)`: excludes that are not virtual objects and are not inside the
/// bin are dropped.
fn remove_large_items(items: &mut Vec<Item>, bin: &Bin) {
    items.retain(|it| {
        it.is_virt_object
            || match bin {
                Bin::Box(b) => is_inside_shape_box(it.transformed_shape(), b),
                Bin::Circle(c) => is_inside_shape_circle(it.transformed_shape(), c),
                Bin::Shape(s) => within_shape_shape(it.transformed_shape(), s),
            }
    });
}

/// `_arrange(shapes, excludes, bin, params, progressfn, stopfn)`, on the items' arena: the shapes
/// first, then the excludes, then the excluded regions of the parameters.
fn arrange_items(
    shapes: Vec<Item>,
    mut excludes: Vec<Item>,
    bin: Bin,
    params: &ArrangeParams,
    cb: &mut ArrangeCallbacks<'_>,
) -> Result<Vec<Item>, ArrangeError> {
    let mut mod_params = params.clone();
    mod_params.min_obj_distance = 0;
    let mut arranger = AutoArranger::new(bin.clone(), &mod_params);
    remove_large_items(&mut excludes, &bin);
    let ns = shapes.len();
    let ne = excludes.len();
    let regions = std::mem::take(&mut arranger.excluded_regions);
    let nr = regions.len();
    let mut arena: Vec<Item> = Vec::with_capacity(ns + ne + nr);
    arena.extend(shapes);
    arena.extend(excludes);
    arena.extend(regions);
    if ne > 0 {
        arranger.preload(&mut arena, ns..ns + ne);
    }

    // operator()(from, to): the spatial index cleared, the items counted, the nester run.
    arranger.rtree.clear();
    arranger.item_count += ns + ne;
    let range: Vec<usize> = (0..ns + ne).collect();
    let excluded_items: Vec<usize> = (ns + ne..ns + ne + nr).collect();
    let pconfig = arranger.pconf.clone();
    let mut never = || false;
    let ArrangeCallbacks {
        progressind,
        on_packed,
        stopcondition,
    } = cb;
    let stop: &mut dyn FnMut() -> bool = match stopcondition {
        Some(s) => *s,
        None => &mut never,
    };
    let mut progress = |n: u32, arena: &[Item], plate: &[usize]| {
        if progressind.is_none() && on_packed.is_none() {
            return;
        }
        if let Some(&last) = plate.last() {
            let last_packed = &arena[last];
            if let Some(p) = progressind.as_mut() {
                p(n, &last_packed.name);
            }
            if let Some(o) = on_packed.as_mut() {
                o(last_packed.bin_id(), last_packed.priority());
            }
        }
    };
    let mut scb = SelectionCallbacks {
        stop,
        progress: &mut progress,
    };
    pack_items(
        &mut arena,
        &range,
        &bin,
        &pconfig,
        &excluded_items,
        &mut arranger,
        &mut scb,
    )?;
    arranger.item_count = 0;

    for itm in &mut arena[..ns + ne] {
        itm.set_inflation(0);
    }
    arena.truncate(ns);
    Ok(arena)
}

/// `to_circle(center, points)`: a circle bed when every point is within `10 * SCALED_EPSILON`
/// of the average distance from the centre, else a radius of NaN.
#[must_use]
pub fn to_circle(center: Point, points: &[Point]) -> CircleBed {
    let mut vertex_distances = Vec::with_capacity(points.len());
    let mut avg_dist = 0.0;
    for pt in points {
        let dx = pt.x.wrapping_sub(center.x) as f64;
        let dy = pt.y.wrapping_sub(center.y) as f64;
        let d = orca_crt::sqrt(dx * dx + dy * dy);
        vertex_distances.push(d);
        avg_dist += d;
    }
    avg_dist /= vertex_distances.len() as f64;
    let mut ret = CircleBed {
        center,
        radius: avg_dist,
    };
    for el in vertex_distances {
        if orca_crt::fabs(el - avg_dist) > 10.0 * SCALED_EPSILON {
            ret = CircleBed::default();
            break;
        }
    }
    ret
}

/// `to_nestbin(bed)`.
fn to_nestbin(bed: &Bed) -> Bin {
    match bed {
        Bed::Box(bb) => Bin::Box(Box::new(bb.min, bb.max)),
        Bed::Circle(c) => Bin::Circle(Circle::new(c.center, c.radius)),
        Bed::Polygon(p) => Bin::Shape(ExPolygon::new(p.clone())),
        Bed::Infinite(b) => Bin::Box(Box::infinite(b.center)),
    }
}

/// `call_with_bed(bed, fn)`'s choice of bed for points: none is an infinite bed at the origin,
/// one an infinite bed there; a polygon that fills its bounding box to within 1e-3 is a box, one
/// whose points are all as far from the box's centre is a circle, any other a polygon.
#[must_use]
pub fn bed_from_points(bed: &[Point]) -> Bed {
    match bed.len() {
        0 => Bed::Infinite(InfiniteBed::default()),
        1 => Bed::Infinite(InfiniteBed { center: bed[0] }),
        _ => {
            let bb = BoundingBox::from_points(bed);
            let circ = to_circle(bb.center(), bed);
            let parea = orca_crt::fabs(Polygon::area_of(bed));
            if (1.0 - parea / bb.area()) < 1e-3 {
                Bed::Box(bb)
            } else if !circ.radius.is_nan() {
                Bed::Circle(circ)
            } else {
                Bed::Polygon(Polygon::new(bed.to_vec()))
            }
        }
    }
}

/// `arrange(items, excludes, bed points, params)`: the bed chosen from its points
/// ([`bed_from_points`]), then [`arrange`].
///
/// # Errors
///
/// As [`arrange`].
pub fn arrange_on_points(
    arrangables: &mut [ArrangePolygon],
    excludes: &[ArrangePolygon],
    bed: &[Point],
    params: &ArrangeParams,
    cb: &mut ArrangeCallbacks<'_>,
) -> Result<(), ArrangeError> {
    arrange(arrangables, excludes, &bed_from_points(bed), params, cb)
}

/// `arrange(arrangables, excludes, bed, params)`: the polygons arranged; each item's
/// translation, rotation, bed (`bed_idx`, -1 when it fits nowhere) and packing order (`itemid`)
/// written back to the polygon of its index.
///
/// # Errors
///
/// [`ArrangeError::RotationsNotTranslated`] for `allow_rotations` (nothing changed), and the
/// exceptions of the placers and the selection.
pub fn arrange(
    arrangables: &mut [ArrangePolygon],
    excludes: &[ArrangePolygon],
    bed: &Bed,
    params: &ArrangeParams,
    cb: &mut ArrangeCallbacks<'_>,
) -> Result<(), ArrangeError> {
    if params.allow_rotations {
        return Err(ArrangeError::RotationsNotTranslated);
    }
    let mut items = Vec::with_capacity(arrangables.len());
    for a in arrangables.iter() {
        process_arrangeable(a, &mut items);
    }
    let mut fixeditems = Vec::new();
    for f in excludes {
        process_arrangeable(f, &mut fixeditems);
    }
    for itm in &mut fixeditems {
        itm.inflate(scaled(-2.0 * EPSILON));
    }
    let items = arrange_items(items, fixeditems, to_nestbin(bed), params, cb)?;
    for (i, it) in items.iter().enumerate() {
        let a = &mut arrangables[i];
        a.translation = it.translation();
        a.rotation = it.rotation();
        a.bed_idx = it.bin_id();
        a.itemid = it.itemid;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: Coord, y: Coord, s: Coord) -> ArrangePolygon {
        ArrangePolygon {
            poly: ExPolygon::from_points(vec![
                Point::new(x, y),
                Point::new(x + s, y),
                Point::new(x + s, y + s),
                Point::new(x, y + s),
            ]),
            extrude_ids: vec![1],
            bed_idx: 0,
            ..ArrangePolygon::default()
        }
    }

    #[test]
    fn two_squares_land_on_the_first_bed_apart() {
        let mm = 1_000_000;
        let mut items = vec![square(0, 0, 20 * mm), square(0, 0, 20 * mm)];
        let bed = Bed::Box(BoundingBox::from_corners(
            Point::new(0, 0),
            Point::new(200 * mm, 200 * mm),
        ));
        let params = ArrangeParams::default();
        arrange(
            &mut items,
            &[],
            &bed,
            &params,
            &mut ArrangeCallbacks::default(),
        )
        .unwrap();
        assert_eq!(items[0].bed_idx, 0);
        assert_eq!(items[1].bed_idx, 0);
        assert_ne!(items[0].translation, items[1].translation);
    }

    /// libnest2d's hull of three points is empty, so the final alignment of a triangle alone on a
    /// plate goes out of Clipper's range in `~_NofitPolyPlacer`, which ends the C++ process.
    #[test]
    #[should_panic(expected = "std::terminate: Coordinate outside allowed range")]
    fn a_lone_triangle_ends_the_run_in_the_final_alignment() {
        let mm = 1_000_000;
        let mut items = vec![ArrangePolygon {
            poly: ExPolygon::from_points(vec![
                Point::new(0, 0),
                Point::new(20 * mm, 0),
                Point::new(0, 20 * mm),
            ]),
            extrude_ids: vec![1],
            bed_idx: 0,
            ..ArrangePolygon::default()
        }];
        let bed = Bed::Box(BoundingBox::from_corners(
            Point::new(0, 0),
            Point::new(200 * mm, 200 * mm),
        ));
        let params = ArrangeParams::default();
        let _ = arrange(
            &mut items,
            &[],
            &bed,
            &params,
            &mut ArrangeCallbacks::default(),
        );
    }

    #[test]
    fn filaments_of_far_apart_temperatures_are_incompatible() {
        assert!(!is_filaments_compatible(&[0, 1]));
        assert!(is_filaments_compatible(&[0, 2, -1]));
    }

    #[test]
    fn rotations_are_refused_before_anything_changes() {
        let mut items = vec![square(0, 0, 10)];
        let params = ArrangeParams {
            allow_rotations: true,
            ..ArrangeParams::default()
        };
        let r = arrange(
            &mut items,
            &[],
            &Bed::Infinite(InfiniteBed::default()),
            &params,
            &mut ArrangeCallbacks::default(),
        );
        assert_eq!(r, Err(ArrangeError::RotationsNotTranslated));
        assert_eq!(items[0].bed_idx, 0);
        assert_eq!(items[0].translation, Point::new(0, 0));
    }
}
