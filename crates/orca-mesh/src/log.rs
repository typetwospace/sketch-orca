// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The records admesh and `TriangleMesh` log through `BOOST_LOG_TRIVIAL(severity) << ...`.
//!
//! The C++ engine sends them to Boost.Log, whose one sink (`cli/main.cpp`, `log_to_stderr`) prints
//! `[<severity>] <message>` on stderr for the records at or above the command line's level
//! (`SKETCH_ORCA_LOG_LEVEL`, 1 by default: error and fatal). Here a record goes to the sink set
//! with [`set_sink`], and nowhere until one is set: the program that runs the crate sets one that
//! prints as the engine prints. The messages are the C++ engine's, word for word.

use std::sync::Mutex;

/// `boost::log::trivial::severity_level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// `trace`.
    Trace,
    /// `debug`.
    Debug,
    /// `info`.
    Info,
    /// `warning`.
    Warning,
    /// `error`.
    Error,
    /// `fatal`.
    Fatal,
}

impl Severity {
    /// The name Boost.Log prints for the level.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Severity::Trace => "trace",
            Severity::Debug => "debug",
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }
}

/// Where the records go.
pub type Sink = Box<dyn FnMut(Severity, &str) + Send>;

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Sets where the records go (`None`: nowhere, as at the start) and returns the sink set before.
pub fn set_sink(sink: Option<Sink>) -> Option<Sink> {
    let mut guard = SINK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::mem::replace(&mut *guard, sink)
}

/// One record: `BOOST_LOG_TRIVIAL(severity) << message`.
pub fn log(severity: Severity, message: &str) {
    let mut guard = SINK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(sink) = guard.as_mut() {
        sink(severity, message);
    }
}
