// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The edges of orca-paths that no probe record holds (added 2026-11-13 by paths-1): the inputs
//! that stop with `NotPorted` (undefined behaviour in the C++, `src/stop.rs`), and the quirks the
//! translation keeps that are easiest to see on a hand-made input: the two versions' one-segment
//! comparisons and `ClosestPointInRadiusLookup`'s equal-range order.

use std::panic::{AssertUnwindSafe, catch_unwind};

use orca_geom::point::Point;
use orca_geom::polyline::Polyline;
use orca_paths::NotPorted;
use orca_paths::lines::ClosestPointInRadiusLookup;
use orca_paths::points::reorder_by_shortest_traverse;
use orca_paths::polylines::chain_polylines;
use orca_paths::segments::{
    chain_segments_greedy_constrained_reversals, chain_segments_greedy_constrained_reversals2,
};
use orca_paths::stop::EMPTY_POLYLINE;

/// Runs `f`, which must stop with `NotPorted` naming `function` and `condition`.
fn stops(function: &str, condition: &str, f: impl FnOnce()) {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(hook);
    let payload = caught.expect_err("no stop");
    let stop = payload
        .downcast::<NotPorted>()
        .expect("a panic that is not NotPorted");
    assert_eq!((stop.function, stop.condition), (function, condition));
}

#[test]
fn an_empty_polyline_stops() {
    let start = Point::new(0, 0);
    stops("chain_polylines", EMPTY_POLYLINE, || {
        let _ = chain_polylines(vec![Polyline::new(Vec::new())], Some(&start));
    });
    stops("reorder_by_shortest_traverse", EMPTY_POLYLINE, || {
        let mut pls = vec![
            Polyline::new(vec![Point::new(1, 1)]),
            Polyline::new(Vec::new()),
        ];
        reorder_by_shortest_traverse(&mut pls);
    });
}

/// One segment from (0, 0) to (10, 0), started near `start`, by the first and second versions.
fn one_segment(start: Point, could_reverse: bool) -> (bool, bool) {
    let end = |_: usize, first: bool| {
        if first {
            Point::new(0, 0)
        } else {
            Point::new(10, 0)
        }
    };
    let v1 = chain_segments_greedy_constrained_reversals(end, |_| could_reverse, 1, Some(&start));
    let v2 = chain_segments_greedy_constrained_reversals2(end, |_| could_reverse, 1, Some(&start));
    assert_eq!((v1.len(), v1[0].0, v2.len(), v2[0].0), (1, 0, 1, 0));
    (v1[0].1, v2[0].1)
}

#[test]
fn one_segment_turns_as_each_version_does() {
    // The first version reverses when the last point is nearer and the segment may reverse.
    assert_eq!(one_segment(Point::new(9, 0), true), (true, false));
    assert_eq!(one_segment(Point::new(9, 0), false), (false, false));
    // The second reverses when the first point is nearer, whether or not it may.
    assert_eq!(one_segment(Point::new(1, 0), true), (false, true));
    assert_eq!(one_segment(Point::new(1, 0), false), (false, true));
}

#[test]
fn the_lookup_keeps_equal_points_in_insertion_order() {
    let mut lookup = ClosestPointInRadiusLookup::new(100, |v: &(u32, Point)| Some(v.1));
    let p = Point::new(500, 500);
    lookup.insert((1, p));
    lookup.insert((2, p));
    lookup.insert((3, Point::new(560, 500)));
    // The nearest of equally near values is the first inserted (the test is a strict `<`).
    assert_eq!(lookup.find(p).0, Some(&(1, p)));
    assert!(lookup.erase(&(1, p)));
    assert_eq!(lookup.find(p).0, Some(&(2, p)));
    assert!(!lookup.erase(&(1, p)));
    assert!(lookup.erase(&(2, p)));
    assert_eq!(lookup.find(p).0, Some(&(3, Point::new(560, 500))));
    // Beyond the search radius nothing is found.
    assert_eq!(lookup.find(Point::new(5000, 5000)).0, None);
}
