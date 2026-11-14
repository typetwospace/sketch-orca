// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! Phase O3 (process() and the layers answer for domain A): libslic3r's Print and PrintObject:
//! their steps' state and invalidation, Print::apply, and PrintObject::slice with the elephant foot
//! compensation and the polyholes.
//!
//! Set up empty by orca-o3-skeleton (2026-11-13). The pieces of the orca-o3 wave fill it: print-1
//! (Print's and PrintObject's state, invalidation and the polyholes' set-up), print-2
//! (Print::apply), print-3 (PrintObject::slice with the elephant foot and the polyholes). Each
//! piece names here what it adds and the C++ it carries. README.md lists the C++ files and how the
//! crate is proven.
//!
//! print-1 (2026-11-14) adds [`print`] (Print's queries: the extruders, the skirt, brim and wipe
//! tower, the shrinkage compensation, `clear`; Print.cpp), [`print_object`] (PrintObject's
//! constructor, `set_instances`, its layers and support layers, PrintInstance's and
//! FuzzySkinPaintedRegion's members; PrintObject.cpp and Print.cpp), [`invalidation`] (the
//! options' and steps' invalidation of both, as tables; Print.cpp and PrintObject.cpp) and
//! [`polyholes`] (`create_polyholes`; PrintObject.cpp). Their calls are free functions over the
//! print and the object, which orca-layer declares; a print object's back-pointer to its print is
//! an argument. Nothing replays here.

pub mod invalidation;
pub mod polyholes;
pub mod print;
pub mod print_object;
