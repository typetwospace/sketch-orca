// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: classes ObjectID, ObjectBase (its ids) and ObjectWithTimestamp (its
// timestamps) of src/libslic3r/ObjectID.hpp and ObjectID.cpp, of OrcaSlicer v2.4.2, translated on
// 2026-11-13 by orca-o3-front-2. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! Object ids and timestamps: what PrintApply compares a model against the print's copy of it by.
//!
//! The C++ gives every `ObjectBase` (a model, an object, a part, an instance, a material, their
//! configs and a part's painting) a new id from one counter when it is made, and a copy keeps the
//! id of what it copies (`copy_id`); a clone gets new ones (`assign_new_unique_ids_recursive`).
//! Here [`ObjectID::generate_new_id`] is that counter (`s_last_id`, from 0, so the first id is 1),
//! an atomic shared by the process as the C++'s static is. A model, an object, a part and an
//! instance hold an id ([`crate::model`]); the ids of configs, materials' configs and paintings
//! are not kept, since only asserts read them (`check_model_ids_validity`,
//! `check_model_ids_equal`, which the release build leaves out).
//!
//! A painting's timestamp ([`crate::facets::FacetsAnnotation`]) is 1 when it is made and copied
//! with it; `touch()` sets it to the next value of a second counter (`s_last_timestamp`, from 1,
//! so the first touch gives 2). Only equality of ids and of timestamps is ever read, so the
//! values a test sees depend on what the process made before, as in the C++.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// `ObjectBase::s_last_id`.
static S_LAST_ID: AtomicUsize = AtomicUsize::new(0);

/// `ObjectWithTimestamp::s_last_timestamp`.
static S_LAST_TIMESTAMP: AtomicU64 = AtomicU64::new(1);

/// `ObjectBase::Timestamp`.
pub type Timestamp = u64;

/// `ObjectID`: an object's id, 0 when invalid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectID {
    /// `id`.
    pub id: usize,
}

impl ObjectID {
    /// `ObjectID(id)`.
    #[must_use]
    pub const fn new(id: usize) -> Self {
        ObjectID { id }
    }

    /// `ObjectBase::generate_new_id()`: `ObjectID(++ s_last_id)`.
    #[must_use]
    pub fn generate_new_id() -> Self {
        ObjectID {
            id: S_LAST_ID.fetch_add(1, Ordering::Relaxed).wrapping_add(1),
        }
    }

    /// `valid()`.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.id != 0
    }

    /// `invalid()`.
    #[must_use]
    pub const fn invalid(self) -> bool {
        self.id == 0
    }
}

/// `ObjectWithTimestamp::touch()`'s new value: `++ s_last_timestamp`.
#[must_use]
pub fn next_timestamp() -> Timestamp {
    S_LAST_TIMESTAMP
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_valid_and_distinct() {
        let a = ObjectID::generate_new_id();
        let b = ObjectID::generate_new_id();
        assert!(a.valid() && b.valid());
        assert_ne!(a, b);
        assert!(ObjectID::default().invalid());
    }

    #[test]
    fn a_touch_is_past_the_first_timestamp() {
        assert!(next_timestamp() > 1);
    }
}
