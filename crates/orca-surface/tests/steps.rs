// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The surface steps of surfaces-2 on small objects built in the test (added 2026-11-14 by
//! orca-o3-surfaces-2): stacks of squares whose expected surface types follow from the C++ by
//! hand. They check the behaviour each step is for and the scaffolding around it (the layers put
//! back, `typed_slices`, the `NotPorted` stop for supports); the steps' parity with the C++
//! engine is proven by orca-o3-dumps' step dumps, as `parity/sop1/surfaces.md` says.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use orca_config::StaticPrintConfig;
use orca_config::print_config::{EnsureVerticalShellThickness, PrintRegionConfig};
use orca_eigen::Transform3d;
use orca_geom::clipper_utils::{DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, offset_ex_expolygons};
use orca_geom::expolygon::ExPolygon;
use orca_geom::point::{Point, scale_};
use orca_geom::polygon::Polygon;
use orca_index::stop::NotPorted;
use orca_layer::layer::Layer;
use orca_layer::print::Print;
use orca_layer::print_object::{PrintObject, PrintObjectRegions};
use orca_layer::print_region::PrintRegion;
use orca_layer::surface::{Surface, SurfaceType};
use orca_surface::detect_surfaces::detect_surfaces_type;
use orca_surface::horizontal_shells::{
    clip_fill_surfaces, combine_infill, discover_horizontal_shells,
};
use orca_surface::layer_region::LayerRegionFill;
use orca_surface::layers_out::LayersOut;
use orca_surface::vertical_shells::discover_vertical_shells;

/// A square from (0, 0) of `side` mm, counter-clockwise.
#[allow(clippy::cast_possible_truncation)]
fn square(side: f64) -> ExPolygon {
    let s = scale_(side) as i64;
    ExPolygon {
        contour: Polygon::new(vec![
            Point::new(0, 0),
            Point::new(s, 0),
            Point::new(s, s),
            Point::new(0, s),
        ]),
        holes: Vec::new(),
    }
}

/// A print of one object of one region, its layers the squares `sides` (mm) bottom to top, each
/// `height` mm thick; each layer's slices are its square (internal), its lslices the square, its
/// fill area the square shrunk by half a millimetre.
#[allow(clippy::cast_possible_truncation)]
fn print_of(sides: &[f64], height: f64, region: impl FnOnce(&mut PrintRegionConfig)) -> Print {
    let mut print = Print::new();
    let mut rc = PrintRegionConfig::default();
    region(&mut rc);
    let rc_hash = rc.hash();
    let mut object = PrintObject::with_members(0, Transform3d::identity());
    object.shared_regions = Some(Arc::new(PrintObjectRegions {
        all_regions: vec![PrintRegion::new(rc, rc_hash, 0)],
        ..PrintObjectRegions::default()
    }));
    for (id, &side) in sides.iter().enumerate() {
        let print_z = height * (id + 1) as f64;
        let mut layer = Layer::new(id, height, print_z, print_z - 0.5 * height);
        let island = square(side);
        layer.lslices = vec![island.clone()];
        let layerm = layer.add_region(0);
        layerm.slices.surfaces = vec![Surface::with_expolygon(
            SurfaceType::Internal,
            island.clone(),
        )];
        layerm.fill_expolygons = offset_ex_expolygons(
            &[island],
            -scale_(0.5) as f32,
            DEFAULT_JOIN_TYPE,
            DEFAULT_MITER_LIMIT,
        );
        object.layers.push(layer);
    }
    print.objects.push(object);
    print
}

/// The surface types of layer `idx`'s slices, in order.
fn slice_types(print: &Print, idx: usize) -> Vec<SurfaceType> {
    print.objects[0].layers[idx].regions[0]
        .slices
        .surfaces
        .iter()
        .map(|s| s.surface_type)
        .collect()
}

/// The surface types of layer `idx`'s fill surfaces, in order.
fn fill_types(print: &Print, idx: usize) -> Vec<SurfaceType> {
    print.objects[0].layers[idx].regions[0]
        .fill_surfaces
        .surfaces
        .iter()
        .map(|s| s.surface_type)
        .collect()
}

/// The area (mm²) of layer `idx`'s slices of type `t`.
fn slice_area(print: &Print, idx: usize, t: SurfaceType) -> f64 {
    print.objects[0].layers[idx].regions[0]
        .slices
        .surfaces
        .iter()
        .filter(|s| s.surface_type == t)
        .map(|s| s.area() * 1e-12)
        .sum()
}

#[test]
fn a_stack_is_bottom_internal_top() {
    let mut print = print_of(&[20.; 3], 0.2, |_| {});
    detect_surfaces_type(&mut print, 0).unwrap();
    let object = &print.objects[0];
    assert!(object.typed_slices);
    assert_eq!(object.layers.len(), 3);
    assert_eq!(slice_types(&print, 0), [SurfaceType::Bottom]);
    assert_eq!(slice_types(&print, 1), [SurfaceType::Internal]);
    assert_eq!(slice_types(&print, 2), [SurfaceType::Top]);
    // The fill surfaces are the slices clipped by the fill area, typed as the slices.
    assert_eq!(fill_types(&print, 1), [SurfaceType::Internal]);
    let fill = &print.objects[0].layers[1].regions[0].fill_surfaces.surfaces[0];
    assert!((fill.area() * 1e-12 - 19. * 19.).abs() < 0.01);
}

#[test]
fn a_single_layer_is_bottom() {
    // Top and bottom overlap whole on the first layer, which is always bottom.
    let mut print = print_of(&[20.], 0.2, |_| {});
    detect_surfaces_type(&mut print, 0).unwrap();
    assert_eq!(slice_types(&print, 0), [SurfaceType::Bottom]);
}

#[test]
fn an_overhang_is_a_bottom_bridge() {
    let mut print = print_of(&[10., 20., 20.], 0.2, |_| {});
    detect_surfaces_type(&mut print, 0).unwrap();
    let bridge = slice_area(&print, 1, SurfaceType::BottomBridge);
    let internal = slice_area(&print, 1, SurfaceType::Internal);
    assert!((bridge - 300.).abs() < 1., "bottom bridge of {bridge} mm2");
    assert!((internal - 100.).abs() < 1., "internal of {internal} mm2");
    assert_eq!(slice_area(&print, 1, SurfaceType::Top), 0.);
}

#[test]
fn an_extra_bridge_layer_doubles_the_bridge() {
    let mut print = print_of(&[10., 20., 20., 20.], 0.2, |_| {});
    print.objects[0].config.enable_extra_bridge_layer.value =
        orca_config::print_config::EnableExtraBridgeLayer::eblApplyToAll;
    detect_surfaces_type(&mut print, 0).unwrap();
    // The internal surface over layer 1's bridge becomes a second bridge, retyped to a bottom
    // bridge for now; the part over layer 1's own internal stays internal.
    let bridge = slice_area(&print, 2, SurfaceType::BottomBridge);
    let internal = slice_area(&print, 2, SurfaceType::Internal);
    assert!(
        bridge > 250. && bridge < 301.,
        "second bridge of {bridge} mm2"
    );
    assert!(
        (bridge + internal - 400.).abs() < 1.,
        "{bridge} + {internal} mm2"
    );
    for idx in 0..4 {
        assert!(!slice_types(&print, idx).contains(&SurfaceType::InternalAfterExternalBridge));
    }
}

#[test]
fn spiral_vase_mode_classifies_the_bottom_layers_only() {
    let mut print = print_of(&[20.; 5], 0.2, |rc| rc.bottom_shell_layers.value = 2);
    print.config.spiral_mode.value = true;
    detect_surfaces_type(&mut print, 0).unwrap();
    assert_eq!(slice_types(&print, 0), [SurfaceType::Bottom]);
    // The last of the bottom layers becomes top; the layers above internal.
    assert_eq!(slice_types(&print, 1), [SurfaceType::Top]);
    for idx in 2..5 {
        assert_eq!(slice_types(&print, idx), [SurfaceType::Internal]);
    }
}

#[test]
fn supports_stop_and_the_layers_come_back() {
    let mut print = print_of(&[20.; 3], 0.2, |_| {});
    print.objects[0].config.enable_support.value = true;
    let got = catch_unwind(AssertUnwindSafe(|| detect_surfaces_type(&mut print, 0)));
    let payload = got.expect_err("an object with supports stops");
    let stop = payload
        .downcast::<NotPorted>()
        .expect("the stop is NotPorted");
    assert_eq!(stop.function, "PrintObject::detect_surfaces_type");
    assert!(
        stop.condition.starts_with("supports: "),
        "{}",
        stop.condition
    );
    let object = &print.objects[0];
    assert_eq!(object.layers.len(), 3, "the layers are put back");
    assert!(!object.typed_slices);
}

#[test]
fn horizontal_shells_spread_tops_and_bottoms() {
    let mut print = print_of(&[20.; 6], 0.2, |rc| {
        rc.top_shell_layers.value = 2;
        rc.bottom_shell_layers.value = 2;
        rc.top_shell_thickness.value = 0.;
        rc.bottom_shell_thickness.value = 0.;
        rc.ensure_vertical_shell_thickness.value = EnsureVerticalShellThickness::evstNone;
        rc.extra_solid_infills.value = "3".to_string();
    });
    detect_surfaces_type(&mut print, 0).unwrap();
    discover_horizontal_shells(&mut print, 0).unwrap();
    assert_eq!(fill_types(&print, 0), [SurfaceType::Bottom]);
    assert_eq!(fill_types(&print, 1), [SurfaceType::InternalSolid]);
    // "3": every 3rd layer (1-based) gets solid infill: index 2.
    assert_eq!(fill_types(&print, 2), [SurfaceType::InternalSolid]);
    assert_eq!(fill_types(&print, 3), [SurfaceType::Internal]);
    assert_eq!(fill_types(&print, 4), [SurfaceType::InternalSolid]);
    assert_eq!(fill_types(&print, 5), [SurfaceType::Top]);
}

#[test]
fn vertical_shells_make_the_neighbours_solid() {
    let mut print = print_of(&[20.; 6], 0.2, |rc| {
        rc.top_shell_layers.value = 2;
        rc.bottom_shell_layers.value = 2;
        rc.top_shell_thickness.value = 0.;
        rc.bottom_shell_thickness.value = 0.;
        rc.ensure_vertical_shell_thickness.value = EnsureVerticalShellThickness::evstAll;
    });
    detect_surfaces_type(&mut print, 0).unwrap();
    discover_vertical_shells(&mut print, 0).unwrap();
    assert_eq!(fill_types(&print, 0), [SurfaceType::Bottom]);
    assert_eq!(fill_types(&print, 1), [SurfaceType::InternalSolid]);
    assert_eq!(fill_types(&print, 2), [SurfaceType::Internal]);
    assert_eq!(fill_types(&print, 3), [SurfaceType::Internal]);
    assert_eq!(fill_types(&print, 4), [SurfaceType::InternalSolid]);
    assert_eq!(fill_types(&print, 5), [SurfaceType::Top]);
    // With evstAll the horizontal shells leave the region alone.
    let before: Vec<Vec<SurfaceType>> = (0..6).map(|i| fill_types(&print, i)).collect();
    discover_horizontal_shells(&mut print, 0).unwrap();
    let after: Vec<Vec<SurfaceType>> = (0..6).map(|i| fill_types(&print, i)).collect();
    assert_eq!(before, after);
}

#[test]
fn combined_infill_goes_to_the_top_of_each_run() {
    let mut print = print_of(&[20.; 6], 0.1, |rc| {
        rc.infill_combination.value = true;
        rc.infill_combination_max_layer_height.value = 0.;
        rc.infill_combination_max_layer_height.percent = false;
    });
    detect_surfaces_type(&mut print, 0).unwrap();
    combine_infill(&mut print, 0).unwrap();
    // The layers above the first, 0.1 mm each, make runs of up to the 0.4 mm nozzle: 1-4 and 5.
    let top_of_run = &print.objects[0].layers[4].regions[0].fill_surfaces.surfaces;
    let combined: Vec<&Surface> = top_of_run
        .iter()
        .filter(|s| s.thickness_layers == 4)
        .collect();
    assert_eq!(combined.len(), 1);
    assert_eq!(combined[0].surface_type, SurfaceType::Internal);
    assert!((combined[0].thickness - 0.4).abs() < 1e-9);
    for idx in 1..4 {
        assert!(
            fill_types(&print, idx).contains(&SurfaceType::InternalVoid),
            "layer {idx}: {:?}",
            fill_types(&print, idx)
        );
    }
}

#[test]
fn clip_fill_surfaces_is_off_in_the_engine() {
    let mut print = print_of(&[20.; 3], 0.2, |_| {});
    detect_surfaces_type(&mut print, 0).unwrap();
    let before: Vec<Vec<SurfaceType>> = (0..3).map(|i| fill_types(&print, i)).collect();
    clip_fill_surfaces(&mut print, 0).unwrap();
    let after: Vec<Vec<SurfaceType>> = (0..3).map(|i| fill_types(&print, i)).collect();
    assert_eq!(before, after);
}

#[test]
fn prepare_fill_surfaces_retypes() {
    let mut print = print_of(&[20.; 3], 0.2, |rc| {
        rc.top_shell_layers.value = 0;
        rc.bottom_shell_layers.value = 0;
        rc.sparse_infill_density.value = 100.;
    });
    detect_surfaces_type(&mut print, 0).unwrap();
    {
        let mut out = LayersOut::new(&mut print, 0);
        let (layers, object, print) = out.parts();
        assert!(object.layers.is_empty(), "the layers are out of the object");
        for layer in layers.iter_mut() {
            layer.regions[0].prepare_fill_surfaces(object, print);
        }
    }
    // Tops and bottoms become internal, and at 100 % density internal becomes internal solid.
    for idx in 0..3 {
        assert_eq!(fill_types(&print, idx), [SurfaceType::InternalSolid]);
    }
}

#[test]
fn infill_area_threshold_is_the_solid_spacing_squared() {
    let print = print_of(&[20.], 0.2, |_| {});
    let object = &print.objects[0];
    let layer = &object.layers[0];
    let got = layer.regions[0]
        .infill_area_threshold(layer, object, &print)
        .unwrap();
    let spacing = layer.regions[0]
        .flow(
            layer,
            object,
            &print,
            orca_layer::flow::FlowRole::SolidInfill,
        )
        .unwrap()
        .scaled_spacing() as f64;
    assert_eq!(got.to_bits(), (spacing * spacing).to_bits());
}

#[test]
fn trim_surfaces_clips_the_slices() {
    let mut print = print_of(&[20.], 0.2, |_| {});
    let layerm = &mut print.objects[0].layers[0].regions[0];
    layerm.trim_surfaces(&[square(10.).contour]);
    let slices = &layerm.slices.surfaces;
    assert_eq!(slices.len(), 1);
    assert_eq!(slices[0].surface_type, SurfaceType::Internal);
    assert!((slices[0].area() * 1e-12 - 100.).abs() < 1e-6);
}
