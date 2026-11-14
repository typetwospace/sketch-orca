// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! [`NotPorted`]: what orca-model answers where a call reaches a family outside domain A (added
//! by orca-o3-front-2, 2026-11-13).
//!
//! Domain A has no painted facets, so the code that builds a `TriangleSelector` from a part's
//! painting (phase O5's) is not translated: [`crate::facets::FacetsAnnotation::get_facets_strict`]
//! returns this error naming the family `painted-facets` of `scripts/lib/parity/corpus.ts`
//! (`FAMILIES`). A job that reaches the family is declined once its objects are placed, before any
//! caller gets here, so the error is a guard, not an answer. It is a returned error, not the panic
//! payload of orca-index's `NotPorted` (orca-model, an O2 crate, does not depend on orca-index).

use core::fmt;

/// The family of painted parts (support, seam, colour or fuzzy-skin painting).
pub const PAINTED_FACETS: &str = "painted-facets";

/// A call that reaches a family outside domain A: the C++ function and the family's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotPorted {
    /// The C++ function, as `Class::method`.
    pub function: &'static str,
    /// The family, as `scripts/lib/parity/corpus.ts` names it.
    pub family: &'static str,
}

impl fmt::Display for NotPorted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not ported: {} reaches {} (outside domain A)",
            self.function, self.family
        )
    }
}

impl std::error::Error for NotPorted {}
