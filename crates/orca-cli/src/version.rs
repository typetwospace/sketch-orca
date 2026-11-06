// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! `sketch-orca-rs version`: the fields of the C++ program's `version_json()` (cli/run.cpp), in
//! its order, and `implementation: "rust"` after them, so a client can tell the two programs
//! apart (protocol/schema/version.schema.json). The engine's version is the workspace's, which
//! test/repo.test.ts holds equal to package.json's and cli/CMakeLists.txt's; Orca's are those of
//! orca.lock.json, which tests/version.rs compares.

use orca_json::{Object, Order, Value};

/// `engine`.
pub const ENGINE: &str = "sketch-orca";
/// `engineVersion`: the workspace's version.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// `orcaVersion` (`SoftFever_VERSION`).
pub const ORCA_VERSION: &str = "2.4.2";
/// `orcaCommit`: orca.lock.json's commit.
pub const ORCA_COMMIT: &str = "8500fcdccaa10b5099ac20d252af3a7c560046f1";
/// `orcaTag`: orca.lock.json's tag.
pub const ORCA_TAG: &str = "v2.4.2";
/// `implementation`: which program answered.
pub const IMPLEMENTATION: &str = "rust";

/// The version object: engine, engineVersion, orcaVersion, orcaCommit, orcaTag, implementation.
#[must_use]
pub fn version_json() -> Value {
    let mut j = Object::new(Order::Insertion);
    j.insert("engine", ENGINE);
    j.insert("engineVersion", ENGINE_VERSION);
    j.insert("orcaVersion", ORCA_VERSION);
    j.insert("orcaCommit", ORCA_COMMIT);
    j.insert("orcaTag", ORCA_TAG);
    j.insert("implementation", IMPLEMENTATION);
    Value::Object(j)
}

/// What `version` prints: the object on one line, then a line feed.
#[must_use]
pub fn version_line() -> String {
    version_json().dump(None) + "\n"
}
