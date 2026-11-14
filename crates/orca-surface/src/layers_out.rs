// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries no OrcaSlicer code: the scaffolding the translated PrintObject steps of this crate
// (crate::detect_surfaces, crate::vertical_shells, crate::horizontal_shells) run on, written on
// 2026-11-14 by orca-o3-surfaces-2. The helpers it holds that belong in other crates say so.

//! How a `PrintObject` step that changes its layers reaches them, the print and the object at
//! once, and how its `tbb::parallel_for` loops run.
//!
//! A step of the C++ `PrintObject` reads its layers through `m_layers`, the object's config and
//! regions through `this`, and the print's config and flows through `m_print`, while it changes
//! the layers' regions. In Rust the layers live in the object and the object in the print, so
//! [`LayersOut`] takes the object's layers out for the length of a step and puts them back when
//! it is dropped, a panic included (`NotPorted`, `CanceledException`). In between,
//! [`LayersOut::parts`] gives the layers to change, and the object and the print to read: the
//! flows read only the object's and the print's configs and regions, never the layers, so nothing
//! the step reads is missing. orca-o3-surfaces-3's `prepare_infill` runs the steps in turn, each
//! taking the layers out itself.
//!
//! [`map_range`] is the `tbb::parallel_for` of a step whose iterations each read the layers and
//! change one layer only, which none of the others reads (each step's header says why that holds
//! of its loops): `orca_tbb::parallel_for` over the same `blocked_range` with the auto
//! partitioner, each index's change computed from the unchanged layers into its own slot, and
//! the changes applied afterwards in index order. The result is the C++ loop's, whatever order
//! the leaves run in. A `FlowError` the C++ throws from a leaf (oneTBB's exception) is kept from
//! the first leaf that gives one, in the order the leaves run; the leaves after it compute
//! nothing more, as oneTBB cancels the loop's other tasks, and nothing is applied.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use orca_config::print_config::SupportType;
use orca_geom::clipper_utils::{
    CLIPPER_SAFETY_OFFSET, DEFAULT_JOIN_TYPE, DEFAULT_MITER_LIMIT, offset_ex_expolygons,
};
use orca_geom::expolygon::{ExPolygon, ExPolygons};
use orca_index::stop::not_ported;
use orca_layer::layer::{Layer, LayerPtrs};
use orca_layer::print::Print;
use orca_layer::print_object::PrintObject;
use orca_tbb::{BlockedRange, Partitioner, parallel_for};

/// An object's layers taken out of it for the length of a step; dropping it puts them back.
pub struct LayersOut<'p> {
    print: &'p mut Print,
    object_idx: usize,
    layers: LayerPtrs,
}

impl<'p> LayersOut<'p> {
    /// Takes the layers of `print.objects[object_idx]` out, leaving the object none.
    ///
    /// # Panics
    /// When the print has no object `object_idx`.
    pub fn new(print: &'p mut Print, object_idx: usize) -> Self {
        let layers = std::mem::take(&mut print.objects[object_idx].layers);
        Self {
            print,
            object_idx,
            layers,
        }
    }

    /// The layers to change, and the object (without its layers) and the print to read.
    pub fn parts(&mut self) -> (&mut [Layer], &PrintObject, &Print) {
        let print: &Print = self.print;
        (&mut self.layers, &print.objects[self.object_idx], print)
    }
}

impl Drop for LayersOut<'_> {
    fn drop(&mut self) {
        self.print.objects[self.object_idx].layers = std::mem::take(&mut self.layers);
    }
}

/// `tbb::parallel_for(tbb::blocked_range<size_t>(begin, end, grain), body)` for a body whose
/// iteration `i` gives `f(i)` from state no iteration changes: the results in index order, or the
/// error of the first leaf, in the order the leaves run, to give one.
///
/// # Errors
/// The first error `f` gives.
pub(crate) fn map_range<T, E, F>(begin: usize, end: usize, grain: usize, f: F) -> Result<Vec<T>, E>
where
    T: Send + Sync,
    E: Send,
    F: Fn(usize) -> Result<T, E> + Sync,
{
    let slots: Vec<OnceLock<T>> = (begin..end).map(|_| OnceLock::new()).collect();
    let error: Mutex<Option<E>> = Mutex::new(None);
    let failed = AtomicBool::new(false);
    parallel_for(
        BlockedRange::new(begin, end, grain),
        Partitioner::Auto,
        |range: BlockedRange<usize>| {
            for i in range.begin()..range.end() {
                if failed.load(Ordering::Acquire) {
                    // oneTBB cancels the loop's other tasks once one has thrown.
                    return;
                }
                match f(i) {
                    Ok(value) => {
                        let _ = slots[i - begin].set(value);
                    }
                    Err(e) => {
                        let mut first = error.lock().unwrap_or_else(PoisonError::into_inner);
                        if first.is_none() {
                            *first = Some(e);
                        }
                        failed.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        },
    );
    if let Some(e) = error.into_inner().unwrap_or_else(PoisonError::into_inner) {
        return Err(e);
    }
    Ok(slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or_else(|| unreachable!("every index of the loop ran"))
        })
        .collect())
}

/// `union_safety_offset_ex(const ExPolygons &)`: the inline `offset_ex(expolygons,
/// ClipperSafetyOffset)` of ClipperUtils.hpp, with the default miter join and limit. Belongs in
/// orca-geom, which carries only the Polygons form.
#[must_use]
pub(crate) fn union_safety_offset_ex_expolygons(expolygons: &[ExPolygon]) -> ExPolygons {
    offset_ex_expolygons(
        expolygons,
        CLIPPER_SAFETY_OFFSET,
        DEFAULT_JOIN_TYPE,
        DEFAULT_MITER_LIMIT,
    )
}

/// `is_auto(support_type)` (PrintConfig.hpp): normal(auto) or tree(auto). Belongs in
/// orca-config.
#[must_use]
pub(crate) fn is_auto(support_type: SupportType) -> bool {
    support_type == SupportType::stNormalAuto || support_type == SupportType::stTreeAuto
}

/// Stops with `NotPorted` in `function` for an object with supports (`has_support()`): the
/// supports family, or tree-supports for a tree support type, as the corpus names them.
pub(crate) fn supports_stop(object: &PrintObject, function: &'static str) -> ! {
    match object.config().support_type.value {
        SupportType::stTree | SupportType::stTreeAuto => not_ported(
            function,
            "tree-supports: an object with supports (has_support(); phase O5)",
        ),
        SupportType::stNormal | SupportType::stNormalAuto => not_ported(
            function,
            "supports: an object with supports (has_support(); phase O5)",
        ),
    }
}

/// `print.config().nozzle_diameter.get_at(filament_id - 1)`: the `int` difference converted to
/// `size_t`, so filament 0 asks past the end and gets the first nozzle. Belongs in orca-layer,
/// whose own `nozzle_diameter_at` is private.
///
/// # Panics
/// With `NotPorted` for an empty `nozzle_diameter` (`get_at` reads its `front()`).
#[must_use]
#[allow(clippy::cast_sign_loss)]
pub(crate) fn nozzle_diameter_of_filament(
    print: &Print,
    filament_id: i32,
    function: &'static str,
) -> f64 {
    let nozzle_diameter = &print.config().nozzle_diameter;
    if nozzle_diameter.values.is_empty() {
        not_ported(
            function,
            "nozzle_diameter is an empty list (get_at reads its front)",
        )
    }
    *nozzle_diameter.get_at(filament_id.wrapping_sub(1) as isize as usize)
}

#[cfg(test)]
mod tests {
    use super::map_range;

    #[test]
    fn map_range_keeps_the_index_order() {
        let got: Result<Vec<usize>, ()> = map_range(2, 40, 3, |i| Ok(i * i));
        assert_eq!(got, Ok((2..40).map(|i| i * i).collect::<Vec<_>>()));
        let empty: Result<Vec<usize>, ()> = map_range(5, 5, 1, Ok);
        assert_eq!(empty, Ok(Vec::new()));
    }

    #[test]
    fn map_range_gives_an_error() {
        let got: Result<Vec<usize>, usize> =
            map_range(0, 100, 1, |i| if i >= 50 { Err(i) } else { Ok(i) });
        let e = got.unwrap_err();
        assert!(e >= 50);
    }
}
