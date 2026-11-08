// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: class BuildVolume of src/libslic3r/BuildVolume.hpp and BuildVolume.cpp
// (the constructor, object_state, volume_state_bbox and type_name), and
// ModelInstance::calc_print_volume_state with ModelObject::update_instances_print_volume_state of
// Model.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The print volume, and whether an object's instances fit in it.
//!
//! The bed's outline is classified as a rectangle (its area within `SCALED_EPSILON²` of its
//! bounding box's), a circle (a RANSAC circle every vertex lies within 0.005 mm of and no edge
//! midpoint cuts more than 3 mm into), or else a convex or custom polygon (by its convex hull's
//! area), and each class tests an object its own way: a rectangle by the transformed hull's
//! bounding box, the others by the mesh's vertices and the edges that cross the bed.
//!
//! The extruder areas of a printer with several (`extruder_printable_area`) are classified as the
//! C++ classifies them and held, but nothing here reads them: the print volume check of the
//! command line does not.

use orca_eigen::{Transform3f, Vec2d, Vec3d, Vec3f};
use orca_geom::bounding_box::{BoundingBox, BoundingBoxf, BoundingBoxf3, get_extents_f};
use orca_geom::clipper_utils::{DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, expand};
use orca_geom::convex_hull::{
    convex_hull, decompose_convex_polygon_top_bottom, inside_convex_polygon,
};
use orca_geom::point::{EPSILON, SCALED_EPSILON, sqr};
use orca_geom::polygon::Polygon;
use orca_mesh::admesh::IndexedTriangleSet;
use orca_mesh::log::{Severity, log};

use crate::circle::{Circled, circle_ransac, smallest_enclosing_circle_welzl};
use crate::model::{ModelObject, PrintVolumeState};

/// `BuildVolume_Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildVolumeType {
    /// `Invalid` (-1).
    Invalid,
    /// `Rectangle` (0).
    Rectangle,
    /// `Circle` (1).
    Circle,
    /// `Convex` (2).
    Convex,
    /// `Custom` (3).
    Custom,
}

impl BuildVolumeType {
    /// The enum's value.
    #[must_use]
    pub fn value(self) -> i8 {
        match self {
            BuildVolumeType::Invalid => -1,
            BuildVolumeType::Rectangle => 0,
            BuildVolumeType::Circle => 1,
            BuildVolumeType::Convex => 2,
            BuildVolumeType::Custom => 3,
        }
    }

    /// `BuildVolume::type_name(type)`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BuildVolumeType::Invalid => "Invalid",
            BuildVolumeType::Rectangle => "Rectangle",
            BuildVolumeType::Circle => "Circle",
            BuildVolumeType::Convex => "Convex",
            BuildVolumeType::Custom => "Custom",
        }
    }
}

/// `BuildVolume::ObjectState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectState {
    /// `Inside` (0).
    Inside,
    /// `Colliding` (1).
    Colliding,
    /// `Outside` (2).
    Outside,
    /// `Below` (3).
    Below,
}

impl ObjectState {
    /// The enum's value.
    #[must_use]
    pub fn value(self) -> u8 {
        match self {
            ObjectState::Inside => 0,
            ObjectState::Colliding => 1,
            ObjectState::Outside => 2,
            ObjectState::Below => 3,
        }
    }
}

/// `BuildVolume::SceneEpsilon`.
pub const SCENE_EPSILON: f64 = EPSILON;
/// `BuildVolume::BedEpsilon`.
pub const BED_EPSILON: f64 = 3.0 * EPSILON;

/// `BuildVolume::BuildExtruderVolume`.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildExtruderVolume {
    /// `same_with_bed`.
    pub same_with_bed: bool,
    /// `type`.
    pub volume_type: BuildVolumeType,
    /// `bbox`.
    pub bbox: BoundingBox,
    /// `bboxf`.
    pub bboxf: BoundingBoxf3,
    /// `circle`.
    pub circle: Circled,
}

impl Default for BuildExtruderVolume {
    fn default() -> Self {
        BuildExtruderVolume {
            same_with_bed: false,
            volume_type: BuildVolumeType::Invalid,
            bbox: BoundingBox::new(),
            bboxf: BoundingBoxf3::default(),
            circle: Circled::default(),
        }
    }
}

/// A convex hull's top and bottom chains (`decompose_convex_polygon_top_bottom`).
pub type Decomposition = (Vec<Vec2d>, Vec<Vec2d>);

/// `BuildVolume`.
#[derive(Clone, Debug)]
pub struct BuildVolume {
    m_bed_shape: Vec<Vec2d>,
    m_max_print_height: f64,
    m_extruder_shapes: Vec<Vec<Vec2d>>,
    m_extruder_volumes: Vec<BuildExtruderVolume>,
    m_type: BuildVolumeType,
    m_polygon: Polygon,
    m_convex_hull: Polygon,
    m_bbox: BoundingBox,
    m_area: f64,
    m_bboxf: BoundingBoxf3,
    m_circle: Circled,
    m_top_bottom_convex_hull_decomposition_scene: Decomposition,
    m_top_bottom_convex_hull_decomposition_bed: Decomposition,
}

/// `get_extents(printable_area)` of the unscaled outline.
fn extents_f(pts: &[Vec2d]) -> BoundingBoxf {
    get_extents_f(pts)
}

/// The circle test of the constructor: every vertex within 0.005 of the circle and no edge
/// midpoint more than 3 inside it.
fn is_circle(shape: &[Vec2d], circle: &Circled) -> bool {
    let mut prev = shape[shape.len() - 1];
    for &p in shape {
        if ((p - circle.center).norm() - circle.radius).abs() > 0.005
            || circle.radius - ((prev + p) * 0.5 - circle.center).norm() > 3.0
        {
            return false;
        }
        prev = p;
    }
    true
}

/// `0.5 * (bbox.min + bbox.max)` and `0.5 * bbox.size().norm()`, as doubles.
fn bbox_circle(bbox: &BoundingBox) -> Circled {
    let min = Vec2d::new(bbox.min.x as f64, bbox.min.y as f64);
    let max = Vec2d::new(bbox.max.x as f64, bbox.max.y as f64);
    let size = bbox.size();
    Circled {
        center: (min + max) * 0.5,
        radius: Vec2d::new(size.x as f64, size.y as f64).norm() * 0.5,
    }
}

/// `scaled<double>(circle)`: centre and radius divided by `SCALING_FACTOR`.
fn scaled_circle(c: &Circled) -> Circled {
    Circled {
        center: c.center / orca_geom::point::SCALING_FACTOR,
        radius: c.radius / orca_geom::point::SCALING_FACTOR,
    }
}

impl BuildVolume {
    /// `BuildVolume(printable_area, printable_height, extruder_areas, extruder_printable_heights)`.
    #[must_use]
    pub fn new(
        printable_area: &[Vec2d],
        printable_height: f64,
        extruder_areas: &[Vec<Vec2d>],
        extruder_printable_heights: &[f64],
    ) -> Self {
        let m_polygon = Polygon::new_scale(printable_area);
        let m_convex_hull = convex_hull(m_polygon.points.clone());
        let m_bbox = m_convex_hull.bounding_box();
        let m_area = m_polygon.area();
        let bboxf = extents_f(printable_area);
        let m_bboxf = BoundingBoxf3::from_corners(
            Vec3d::new(bboxf.min.x(), bboxf.min.y(), 0.0),
            Vec3d::new(bboxf.max.x(), bboxf.max.y(), printable_height),
        );
        let mut bv = BuildVolume {
            m_bed_shape: printable_area.to_vec(),
            m_max_print_height: printable_height,
            m_extruder_shapes: extruder_areas.to_vec(),
            m_extruder_volumes: Vec::new(),
            m_type: BuildVolumeType::Invalid,
            m_polygon,
            m_convex_hull,
            m_bbox,
            m_area,
            m_bboxf,
            m_circle: Circled::default(),
            m_top_bottom_convex_hull_decomposition_scene: (Vec::new(), Vec::new()),
            m_top_bottom_convex_hull_decomposition_bed: (Vec::new(), Vec::new()),
        };
        let bbox_size = bv.m_bbox.size();
        if printable_area.len() >= 4
            && (bv.m_area - (bbox_size.x as f64) * (bbox_size.y as f64)).abs() < sqr(SCALED_EPSILON)
        {
            bv.m_type = BuildVolumeType::Rectangle;
            bv.m_circle = bbox_circle(&bv.m_bbox);
        } else if printable_area.len() > 3 {
            bv.m_circle = circle_ransac(printable_area, 20).0;
            if is_circle(printable_area, &bv.m_circle) {
                bv.m_type = BuildVolumeType::Circle;
                bv.m_circle = scaled_circle(&bv.m_circle);
            }
        }
        if printable_area.len() >= 3 && bv.m_type == BuildVolumeType::Invalid {
            bv.m_circle = smallest_enclosing_circle_welzl(&bv.m_convex_hull.points);
            bv.m_type = if (bv.m_convex_hull.area() - bv.m_area) < sqr(SCALED_EPSILON) {
                BuildVolumeType::Convex
            } else {
                BuildVolumeType::Custom
            };
            let convex_decomposition = |src: &Polygon, epsilon: f64| {
                let expanded = expand(
                    std::slice::from_ref(src),
                    epsilon as f32,
                    DEFAULT_JOIN_TYPE,
                    DEFAULT_MITER_LIMIT,
                );
                // `.front()` of an empty result is undefined in the C++ (a hull of fewer than
                // three points): an empty polygon here (`parity/HAZARDS.md`, orca-model).
                let src = expanded.into_iter().next().unwrap_or_default();
                let pts: Vec<Vec2d> = src
                    .points
                    .iter()
                    .map(|pt| {
                        Vec2d::new(pt.x as f64, pt.y as f64) * orca_geom::point::SCALING_FACTOR
                    })
                    .collect();
                decompose_convex_polygon_top_bottom(&pts)
            };
            bv.m_top_bottom_convex_hull_decomposition_scene =
                convex_decomposition(&bv.m_convex_hull, SCENE_EPSILON);
            bv.m_top_bottom_convex_hull_decomposition_bed =
                convex_decomposition(&bv.m_convex_hull, BED_EPSILON);
        }
        if !bv.m_extruder_shapes.is_empty() {
            for index in 0..bv.m_extruder_shapes.len() {
                let extruder_shape = bv.m_extruder_shapes[index].clone();
                let mut ev = BuildExtruderVolume::default();
                if extruder_shape.is_empty() {
                    log(
                        Severity::Warning,
                        &format!("Found invalid extruder_printable_area of index {index}"),
                    );
                    bv.m_extruder_shapes.clear();
                    return bv;
                }
                // Past the heights' end the C++ reads out of bounds.
                let height = *extruder_printable_heights
                    .get(index)
                    .expect("a height for each extruder area");
                if extruder_shape.as_slice() == printable_area && height == printable_height {
                    ev.same_with_bed = true;
                    ev.volume_type = bv.m_type;
                    ev.bbox = bv.m_bbox;
                    ev.bboxf = bv.m_bboxf;
                    ev.circle = bv.m_circle;
                } else {
                    let poly = Polygon::new_scale(&extruder_shape);
                    let poly_area = poly.area();
                    ev.bbox = poly.bounding_box();
                    let temp = extents_f(&extruder_shape);
                    ev.bboxf = BoundingBoxf3::from_corners(
                        Vec3d::new(temp.min.x(), temp.min.y(), 0.0),
                        Vec3d::new(temp.max.x(), temp.max.y(), height),
                    );
                    let s = ev.bbox.size();
                    if extruder_shape.len() >= 4
                        && (poly_area - (s.x as f64) * (s.y as f64)).abs() < sqr(SCALED_EPSILON)
                    {
                        ev.volume_type = BuildVolumeType::Rectangle;
                        ev.circle = bbox_circle(&ev.bbox);
                    } else if extruder_shape.len() > 3 {
                        ev.circle = circle_ransac(&extruder_shape, 20).0;
                        if is_circle(&extruder_shape, &ev.circle) {
                            ev.volume_type = BuildVolumeType::Circle;
                            ev.circle = scaled_circle(&ev.circle);
                        }
                    }
                    if bv.m_type == BuildVolumeType::Invalid {
                        ev.same_with_bed = true;
                        ev.volume_type = bv.m_type;
                        ev.bbox = bv.m_bbox;
                        ev.bboxf = bv.m_bboxf;
                        ev.circle = bv.m_circle;
                    }
                    ev.bboxf.min[2] = -f64::MAX;
                }
                bv.m_extruder_volumes.push(ev);
            }
        }
        log(
            Severity::Debug,
            &format!(
                "BuildVolume printable_area clasified as: {}",
                bv.m_type.name()
            ),
        );
        bv
    }

    /// `type()`.
    #[must_use]
    pub fn volume_type(&self) -> BuildVolumeType {
        self.m_type
    }

    /// `bed_shape()`.
    #[must_use]
    pub fn bed_shape(&self) -> &[Vec2d] {
        &self.m_bed_shape
    }

    /// `printable_height()`.
    #[must_use]
    pub fn printable_height(&self) -> f64 {
        self.m_max_print_height
    }

    /// `polygon()`: the scaled outline.
    #[must_use]
    pub fn polygon(&self) -> &Polygon {
        &self.m_polygon
    }

    /// `bounding_volume()`.
    #[must_use]
    pub fn bounding_volume(&self) -> BoundingBoxf3 {
        self.m_bboxf
    }

    /// `bounding_volume2d()`.
    #[must_use]
    pub fn bounding_volume2d(&self) -> BoundingBoxf {
        BoundingBoxf::from_corners(
            Vec2d::new(self.m_bboxf.min.x(), self.m_bboxf.min.y()),
            Vec2d::new(self.m_bboxf.max.x(), self.m_bboxf.max.y()),
        )
    }

    /// `circle()`: scaled for a round bed.
    #[must_use]
    pub fn circle(&self) -> Circled {
        self.m_circle
    }

    /// The scaled convex hull, its bounding box and the outline's area.
    #[must_use]
    pub fn convex_hull(&self) -> (&Polygon, BoundingBox, f64) {
        (&self.m_convex_hull, self.m_bbox, self.m_area)
    }

    /// The top and bottom decompositions of the hull, with the scene's and the bed's epsilon.
    #[must_use]
    pub fn decompositions(&self) -> (&Decomposition, &Decomposition) {
        (
            &self.m_top_bottom_convex_hull_decomposition_scene,
            &self.m_top_bottom_convex_hull_decomposition_bed,
        )
    }

    /// The extruder areas' volumes.
    #[must_use]
    pub fn extruder_volumes(&self) -> &[BuildExtruderVolume] {
        &self.m_extruder_volumes
    }

    /// `object_state(its, trafo, may_be_below_bed, ignore_bottom)`.
    #[must_use]
    pub fn object_state(
        &self,
        its: &IndexedTriangleSet,
        trafo: &Transform3f,
        may_be_below_bed: bool,
        ignore_bottom: bool,
    ) -> ObjectState {
        match self.m_type {
            BuildVolumeType::Rectangle => {
                let mut build_volume = inflated3(&self.m_bboxf, SCENE_EPSILON);
                if self.m_max_print_height == 0.0 {
                    build_volume.max[2] = f64::MAX;
                }
                if ignore_bottom {
                    build_volume.min[2] = -f64::MAX;
                }
                let min = build_volume.min.cast::<f32>();
                let max = build_volume.max.cast::<f32>();
                object_state_templ(its, trafo, may_be_below_bed, true, |pt: &Vec3f| {
                    pt.x() >= min.x()
                        && pt.x() <= max.x()
                        && pt.y() >= min.y()
                        && pt.y() <= max.y()
                        && pt.z() >= min.z()
                        && pt.z() <= max.z()
                })
            }
            BuildVolumeType::Circle => {
                let center =
                    self.m_circle.center.cast::<f32>() * (orca_geom::point::SCALING_FACTOR as f32);
                let radius = ((self.m_circle.radius + SCENE_EPSILON) as f32)
                    * (orca_geom::point::SCALING_FACTOR as f32);
                let contains = move |p: &Vec3f| {
                    let d = orca_eigen::Vec2f::new(p.x(), p.y()) - center;
                    d.squared_norm() <= radius * radius
                };
                if self.m_max_print_height == 0.0 {
                    object_state_templ(its, trafo, may_be_below_bed, true, contains)
                } else {
                    let z = self.m_max_print_height + SCENE_EPSILON;
                    object_state_templ(its, trafo, may_be_below_bed, true, |pt: &Vec3f| {
                        f64::from(pt.z()) < z && contains(pt)
                    })
                }
            }
            BuildVolumeType::Convex | BuildVolumeType::Custom => {
                let dec = &self.m_top_bottom_convex_hull_decomposition_scene;
                let convex = self.m_type == BuildVolumeType::Convex;
                let inside = |pt: &Vec3f| {
                    inside_convex_polygon(dec, Vec2d::new(f64::from(pt.x()), f64::from(pt.y())))
                };
                if self.m_max_print_height == 0.0 {
                    object_state_templ(its, trafo, may_be_below_bed, convex, inside)
                } else {
                    let z = self.m_max_print_height + SCENE_EPSILON;
                    object_state_templ(its, trafo, may_be_below_bed, convex, |pt: &Vec3f| {
                        f64::from(pt.z()) < z && inside(pt)
                    })
                }
            }
            BuildVolumeType::Invalid => ObjectState::Inside,
        }
    }

    /// `volume_state_bbox(volume_bbox, ignore_bottom)`: for a rectangular bed.
    #[must_use]
    pub fn volume_state_bbox(
        &self,
        volume_bbox: &BoundingBoxf3,
        ignore_bottom: bool,
    ) -> ObjectState {
        let mut build_volume = inflated3(&self.m_bboxf, SCENE_EPSILON);
        if self.m_max_print_height == 0.0 {
            build_volume.max[2] = f64::MAX;
        }
        if ignore_bottom {
            build_volume.min[2] = -f64::MAX;
        }
        if build_volume.max.z() <= -SCENE_EPSILON {
            ObjectState::Below
        } else if build_volume.contains(volume_bbox.min) && build_volume.contains(volume_bbox.max) {
            ObjectState::Inside
        } else if build_volume.intersects(volume_bbox) {
            ObjectState::Colliding
        } else {
            ObjectState::Outside
        }
    }
}

/// `BoundingBox3Base::inflated(delta)`.
fn inflated3(bb: &BoundingBoxf3, delta: f64) -> BoundingBoxf3 {
    let mut out = *bb;
    out.offset(delta);
    out
}

/// `object_state_templ(its, trafo, may_be_below_bed, convex, is_inside)`.
fn object_state_templ(
    its: &IndexedTriangleSet,
    trafo: &Transform3f,
    may_be_below_bed: bool,
    convex: bool,
    is_inside: impl Fn(&Vec3f) -> bool,
) -> ObjectState {
    let mut num_inside = 0usize;
    let mut num_above = 0usize;
    let mut inside: bool;
    let mut outside: bool;
    let world_min_z = (-SCENE_EPSILON) as f32;
    if may_be_below_bed {
        let mut sides: Vec<i32> = Vec::with_capacity(its.vertices.len());
        let sign = |pt: &Vec3f| {
            if pt.z() > world_min_z {
                1
            } else if pt.z() < world_min_z {
                -1
            } else {
                0
            }
        };
        let mut below_outside = false;
        for v in &its.vertices {
            let mut pt = trafo.mul_point(v);
            let s = sign(&pt);
            sides.push(s);
            if s >= 0 {
                num_above += 1;
                if is_inside(&pt) {
                    num_inside += 1;
                }
            } else if convex && !below_outside {
                pt[2] = 0.0;
                if !is_inside(&pt) {
                    below_outside = true;
                }
            }
        }
        if num_above == 0 {
            return ObjectState::Below;
        }
        inside = num_inside > 0;
        outside = num_inside < num_above;
        // `! (inside && outside) && (! (inside && ! below_outside) || ! convex)` in the C++.
        if num_above < its.vertices.len()
            && (!inside || !outside)
            && (!inside || below_outside || !convex)
        {
            for tri in &its.indices {
                let s = [
                    sides[tri[0] as usize],
                    sides[tri[1] as usize],
                    sides[tri[2] as usize],
                ];
                if orca_stl::min(s[0], orca_stl::min(s[1], s[2])) < 0
                    && orca_stl::max(s[0], orca_stl::max(s[1], s[2])) > 0
                {
                    let mut iprev = 2usize;
                    for iedge in 0..3 {
                        if s[iprev] * s[iedge] == -1 {
                            let p1 = trafo.mul_point(&its.vertices[tri[iprev] as usize]);
                            let p2 = trafo.mul_point(&its.vertices[tri[iedge] as usize]);
                            let t = (world_min_z - p1.z()) / (p2.z() - p1.z());
                            let q = Vec3f::new(
                                p1.x() + (p2.x() - p1.x()) * t,
                                p1.y() + (p2.y() - p1.y()) * t,
                                world_min_z,
                            );
                            if is_inside(&q) {
                                inside = true;
                            } else {
                                outside = true;
                            }
                        }
                        iprev = iedge;
                    }
                    if inside && outside {
                        break;
                    }
                }
            }
        }
    } else {
        num_above = its.vertices.len();
        for v in &its.vertices {
            let pt = trafo.mul_point(v);
            if is_inside(&pt) {
                num_inside += 1;
            }
        }
        inside = num_inside > 0;
        outside = num_inside < num_above;
    }
    if inside {
        if outside {
            ObjectState::Colliding
        } else {
            ObjectState::Inside
        }
    } else {
        ObjectState::Outside
    }
}

/// `BoundingBoxf::inflated(delta).overlap(other)`.
fn overlap_inflated(a: &BoundingBoxf, delta: f64, b: &BoundingBoxf) -> bool {
    let mut a = *a;
    a.offset(delta);
    !(a.max.x() < b.min.x()
        || a.min.x() > b.max.x()
        || a.max.y() < b.min.y()
        || a.min.y() > b.max.y())
}

impl ModelObject {
    /// `ModelInstance::calc_print_volume_state(build_volume)` of the instance: inside when every
    /// part (but those below the bed, and those whose hull's box is flat) is inside, fully outside
    /// when none is, partly outside otherwise.
    #[must_use]
    pub fn calc_print_volume_state(
        &self,
        instance_idx: usize,
        build_volume: &BuildVolume,
    ) -> PrintVolumeState {
        const INSIDE: u32 = 1;
        const OUTSIDE: u32 = 2;
        let instance = &self.instances[instance_idx];
        let mut inside_outside = 0u32;
        for vol in &self.volumes {
            if !vol.is_model_part() {
                continue;
            }
            let bb = vol.get_convex_hull().bounding_box();
            let size = bb.size();
            if size.x() == 0.0 || size.y() == 0.0 || size.z() == 0.0 {
                let f = |v: f64| {
                    orca_fmt::ostream::f64_to_string(&mut orca_fmt::ostream::Stream::default(), v)
                        .unwrap_or_default()
                };
                log(
                    Severity::Info,
                    &format!(
                        "Slic3r::ModelInstance::calc_print_volume_state, object {}'s vol {} is empty, skip it, box: {{{}, {}, {}}} to {{{}, {}, {}}}",
                        self.name,
                        vol.name,
                        f(bb.min.x()),
                        f(bb.min.y()),
                        f(bb.min.z()),
                        f(bb.max.x()),
                        f(bb.max.y()),
                        f(bb.max.z())
                    ),
                );
                continue;
            }
            let matrix = instance.get_matrix().mul(vol.get_matrix());
            let bboxt = vol.get_convex_hull().transformed_bounding_box(&matrix);
            let bbox2d = BoundingBoxf::from_corners(
                Vec2d::new(bboxt.min.x(), bboxt.min.y()),
                Vec2d::new(bboxt.max.x(), bboxt.max.y()),
            );
            let state =
                if !overlap_inflated(&build_volume.bounding_volume2d(), SCENE_EPSILON, &bbox2d) {
                    ObjectState::Outside
                } else {
                    match build_volume.volume_type() {
                        BuildVolumeType::Rectangle => build_volume.volume_state_bbox(&bboxt, true),
                        _ => build_volume.object_state(
                            &vol.mesh().its,
                            &matrix.cast::<f32>(),
                            true,
                            true,
                        ),
                    }
                };
            match state {
                ObjectState::Inside => inside_outside |= INSIDE,
                ObjectState::Outside => inside_outside |= OUTSIDE,
                ObjectState::Below => {}
                ObjectState::Colliding => inside_outside |= INSIDE | OUTSIDE,
            }
        }
        if inside_outside == (INSIDE | OUTSIDE) {
            PrintVolumeState::PartlyOutside
        } else if inside_outside == INSIDE {
            PrintVolumeState::Inside
        } else {
            PrintVolumeState::FullyOutside
        }
    }

    /// `ModelInstance::update_print_volume_state(build_volume)` of the instance.
    pub fn update_print_volume_state(
        &mut self,
        instance_idx: usize,
        build_volume: &BuildVolume,
    ) -> PrintVolumeState {
        let state = self.calc_print_volume_state(instance_idx, build_volume);
        self.instances[instance_idx].print_volume_state = state;
        state
    }

    /// `update_instances_print_volume_state(build_volume)`: how many instances are inside.
    pub fn update_instances_print_volume_state(&mut self, build_volume: &BuildVolume) -> u32 {
        let mut num_printable = 0;
        for i in 0..self.instances.len() {
            if self.update_print_volume_state(i, build_volume) == PrintVolumeState::Inside {
                num_printable += 1;
            }
        }
        num_printable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_bed_is_a_rectangle_and_a_round_one_a_circle() {
        let square = [
            Vec2d::new(0.0, 0.0),
            Vec2d::new(256.0, 0.0),
            Vec2d::new(256.0, 256.0),
            Vec2d::new(0.0, 256.0),
        ];
        let bv = BuildVolume::new(&square, 250.0, &[], &[]);
        assert_eq!(bv.volume_type(), BuildVolumeType::Rectangle);
        let round: Vec<Vec2d> = (0..72)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::PI / 36.0;
                Vec2d::new(90.0 * orca_crt::cos(a), 90.0 * orca_crt::sin(a))
            })
            .collect();
        let bv = BuildVolume::new(&round, 200.0, &[], &[]);
        assert_eq!(bv.volume_type(), BuildVolumeType::Circle);
        let l = [
            Vec2d::new(0.0, 0.0),
            Vec2d::new(200.0, 0.0),
            Vec2d::new(200.0, 100.0),
            Vec2d::new(100.0, 100.0),
            Vec2d::new(100.0, 200.0),
            Vec2d::new(0.0, 200.0),
        ];
        let bv = BuildVolume::new(&l, 200.0, &[], &[]);
        assert_eq!(bv.volume_type(), BuildVolumeType::Custom);
    }
}
