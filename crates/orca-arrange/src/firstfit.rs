// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Libnest2D code: libnest2d as OrcaSlicer v2.4.2 carries it in deps_src/libnest2d,
// translated on 2026-11-07 from include/libnest2d/selections/firstfit.hpp
// (_FirstFitSelection::packItems), selections/selection_boilerplate.hpp
// (remove_unpackable_items) and nester.hpp (PlacementStrategyLike, _Nester::execute). Copyright
// (C) Tamas Meszaros, modified by Prusa Research, Bambu Lab and the OrcaSlicer contributors, under
// the GNU Lesser General Public License, version 3 (licenses/LGPL-3.0.txt).

//! libnest2d's first-fit selection as Orca changed it: each item, in the hooks' sort order, goes
//! to the plate where the sum of every plate's score with it is lowest (or the first plate that
//! takes it, past `MAX_NUM_PLATES`), and a new plate is opened when none takes it.
//!
//! - Fixed items go to the plate of their bin (a negative bin is plate 0) and are preloaded into
//!   that plate's placer when it opens; the excluded regions are preloaded instead when an item
//!   fails on a plate with nothing packed, once per item.
//! - First, every item is tried alone on an empty plate with the excluded regions; one that does
//!   not fit (or has no area) is marked unfit (`BIN_ID_UNFIT`) and skipped.
//! - A placer is cleared (and destroyed) by `finalAlign`, which moves the items it holds: the
//!   check placer when the check ends, a placer when items are cleared from it, and every placer,
//!   in order, when the selection ends. Clearing items lets an exception through; destroying a
//!   placer runs `finalAlign` in its destructor, where an exception calls `std::terminate`, so
//!   the translation panics there ([`destroy`]): a triangle alone on a plate is one such input
//!   (`parity/HAZARDS.md`).
//! - The placers live in a `std::vector` reserved for as many placers as items. The placer type is
//!   not nothrow-movable (`Slic3r::Point` declares its copy constructor), so when the vector
//!   grows past that, MSVC copies the placers and destroys the old ones in order, each running
//!   `finalAlign` on the items it shares with its copy. That is kept, with MSVC's growth (half
//!   again).
//! - Where the C++ preloads `fixed_bins[placers.size() - 1]` past the end of `fixed_bins` (an
//!   item that fails twice on a plate with nothing packed, past the plates of fixed items), it
//!   reads outside the vector, which is undefined; this translation stops with
//!   [`ArrangeError::Undefined`] there.

use crate::ArrangeError;
use crate::item::{BIN_ID_UNFIT, Item, LARGE_COST_TO_REJECT, MAX_NUM_PLATES};
use crate::placer::{Bin, Hooks, NfpPlacer, PackResult, PlacerConfig};
use orca_geom::clipper::ClipperException;

/// `progress_`, given the number of items packed so far, the arena, and the packed items of the
/// plate the last item went to (`lastResult()[lastPackedBinId()]`).
pub type ProgressFn<'a> = dyn FnMut(u32, &[Item], &[usize]) + 'a;

/// What the selection calls back on: the stop condition and the progress of each packed item.
pub struct SelectionCallbacks<'a> {
    /// `stopcond_`.
    pub stop: &'a mut dyn FnMut() -> bool,
    /// `progress_`.
    pub progress: &'a mut ProgressFn<'a>,
}

/// The vector growth of MSVC's `_Calculate_growth`: half again, or the size asked for.
fn msvc_growth(capacity: usize, new_size: usize) -> usize {
    let geometric = capacity + capacity / 2;
    if geometric < new_size {
        new_size
    } else {
        geometric
    }
}

/// `~_NofitPolyPlacer()`: `clearItems()`, whose `finalAlign` moves the items. The destructor is
/// `noexcept`, so an exception there ends the process (`std::terminate`): a panic here, whether
/// the exception is an error of the translation or one thrown as a panic (Clipper's), so that no
/// caller catches what the C++ cannot.
///
/// # Panics
///
/// When `finalAlign` fails.
fn destroy<F: Clone + Send + Sync>(p: &NfpPlacer<F>, arena: &mut [Item]) {
    let aligned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.final_align(arena)));
    let what = match aligned {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(payload) => match payload.downcast_ref::<ClipperException>() {
            Some(e) => e.what().to_owned(),
            None => match payload.downcast_ref::<String>() {
                Some(s) => s.clone(),
                None => payload
                    .downcast_ref::<&str>()
                    .map_or("a panic", |s| s)
                    .to_owned(),
            },
        },
    };
    panic!("std::terminate: {what} left ~_NofitPolyPlacer");
}

/// `remove_unpackable_items`: each item tried, as a copy, on a placer with only the excluded
/// regions; the original is marked unfit when it has no area or the copy did not pack. The check
/// placer is then destroyed (`finalAlign`).
///
/// # Errors
///
/// The exceptions of the placer.
#[allow(clippy::too_many_arguments)]
pub fn remove_unpackable_items<H: Hooks>(
    arena: &mut [Item],
    store: &[usize],
    bin: &Bin,
    pconfig: &PlacerConfig<H::ObjFn>,
    excluded_items: &[usize],
    hooks: &mut H,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), ArrangeError> {
    let mut p = NfpPlacer::new(bin.clone(), pconfig.clone());
    p.preload(arena, excluded_items, hooks);
    let mut k = 0;
    while k < store.len() && !stop() {
        let it = store[k];
        let mut cpy = arena[it].clone();
        let result = p.trypack(arena, &mut cpy, &[], hooks)?;
        if arena[it].area() <= 0.0 || !result.packed {
            arena[it].set_bin_id(BIN_ID_UNFIT);
        }
        k += 1;
    }
    destroy(&p, arena);
    Ok(())
}

/// `_FirstFitSelection::packItems(first, last, bin, pconfig)`, with `pconfig.m_excluded_items`
/// already holding the excluded regions (their arena indices).
///
/// # Errors
///
/// The exceptions of the placers, and [`ArrangeError::Undefined`] where the C++ reads past
/// `fixed_bins`.
pub fn pack_items<H: Hooks>(
    arena: &mut [Item],
    range: &[usize],
    bin: &Bin,
    pconfig: &PlacerConfig<H::ObjFn>,
    excluded_items: &[usize],
    hooks: &mut H,
    cb: &mut SelectionCallbacks<'_>,
) -> Result<(), ArrangeError> {
    let mut store: Vec<usize> = Vec::with_capacity(range.len());
    let mut placers: Vec<NfpPlacer<H::ObjFn>> = Vec::new();
    let mut placers_capacity = range.len();
    let mut fixed_bins: Vec<Vec<usize>> = Vec::new();
    for &i in range {
        if arena[i].is_fixed() {
            if arena[i].bin_id() < 0 {
                arena[i].set_bin_id(0);
            }
            let binidx = arena[i].bin_id() as usize;
            while fixed_bins.len() <= binidx {
                fixed_bins.push(Vec::new());
            }
            fixed_bins[binidx].push(i);
        } else {
            store.push(i);
        }
    }
    {
        let a: &[Item] = arena;
        let h: &H = hooks;
        orca_stl::sort_by(&mut store, |x, y| h.sort_less(&a[*x], &a[*y]));
    }

    let mut item_id: i32 = 0;
    remove_unpackable_items(arena, &store, bin, pconfig, excluded_items, hooks, cb.stop)?;

    let mut k = 0;
    while k < store.len() && !(cb.stop)() {
        let it = store[k];
        if arena[it].bin_id() == BIN_ID_UNFIT {
            k += 1;
            continue;
        }
        let remaining = &store[k + 1..];
        let mut was_packed = false;
        let mut best_bed_id: i32 = -1;
        let mut bed_id_firstfit: i32 = -1;
        let mut score_all_plates_best = f64::MAX;
        let mut result_best = PackResult::default();
        let mut result_firstfit = PackResult::default();
        let mut j: i32 = 0;
        while !was_packed && !(cb.stop)() {
            while (j as usize) < placers.len() && !was_packed && !(cb.stop)() {
                let mut item = std::mem::take(&mut arena[it]);
                let r = placers[j as usize].trypack(arena, &mut item, remaining, hooks);
                arena[it] = item;
                let result = r?;
                let score = result.score;
                let mut score_all_plates = score;
                for p in &placers {
                    score_all_plates += p.score;
                }
                if (0.0..LARGE_COST_TO_REJECT).contains(&score) {
                    if bed_id_firstfit == -1 {
                        bed_id_firstfit = j;
                        result_firstfit = result;
                    }
                    if score_all_plates < score_all_plates_best {
                        score_all_plates_best = score_all_plates;
                        best_bed_id = j;
                        result_best = result;
                    }
                }
                j += 1;
            }
            if best_bed_id == MAX_NUM_PLATES {
                if bed_id_firstfit == MAX_NUM_PLATES {
                    arena[it].set_bin_id(BIN_ID_UNFIT);
                    break;
                }
                best_bed_id = bed_id_firstfit;
                result_best = result_firstfit;
            }

            if best_bed_id >= 0 {
                was_packed = true;
                j = best_bed_id;
                arena[it].set_bin_id(j);
                arena[it].itemid = item_id;
                item_id += 1;
                let ju = j as usize;
                placers[ju].accept(arena, it, &result_best);
                (cb.progress)(item_id as u32, arena, &placers[ju].items);
            }

            if was_packed && arena[it].has_tried_with_excluded {
                let ju = j as usize;
                placers[ju].clear_items_if(arena, |itm| itm.is_fixed() && !itm.is_wipe_tower)?;
                if fixed_bins.len() >= placers.len() {
                    let fb = &fixed_bins[placers.len() - 1];
                    placers[ju].preload(arena, fb, hooks);
                }
            }
            let placer_not_packed = !was_packed
                && !placers.is_empty()
                && j as usize == placers.len()
                && placers[j as usize - 1].packed_size(arena) == 0;
            if placer_not_packed {
                let last = j as usize - 1;
                if !arena[it].has_tried_with_excluded {
                    arena[it].has_tried_with_excluded = true;
                    placers[last]
                        .clear_items_if(arena, |itm| itm.is_fixed() && !itm.is_wipe_tower)?;
                    placers[last].preload(arena, excluded_items, hooks);
                    j -= 1;
                    continue;
                }
                placers[last].clear_items_if(arena, |itm| itm.is_fixed() && !itm.is_wipe_tower)?;
                let Some(fb) = fixed_bins.get(placers.len() - 1) else {
                    return Err(ArrangeError::Undefined(
                        "fixed_bins[placers.size() - 1] read past the end of fixed_bins",
                    ));
                };
                placers[last].preload(arena, fb, hooks);
            }

            if !was_packed {
                if placers.len() == placers_capacity {
                    for p in &placers {
                        destroy(p, arena);
                    }
                    placers_capacity = msvc_growth(placers_capacity, placers.len() + 1);
                }
                let mut p = NfpPlacer::new(bin.clone(), pconfig.clone());
                p.plate_id = (placers.len()) as i32;
                placers.push(p);
                if fixed_bins.len() >= placers.len() {
                    let idx = placers.len() - 1;
                    let fb = &fixed_bins[idx];
                    placers[idx].preload(arena, fb, hooks);
                }
                j = (placers.len() - 1) as i32;
            }
        }
        k += 1;
    }

    for p in &placers {
        destroy(p, arena);
    }
    Ok(())
}
