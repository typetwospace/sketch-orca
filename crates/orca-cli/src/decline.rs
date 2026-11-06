// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The decline: sketch-orca-rs does only what the parity harness has certified it to do the C++
//! engine's way. A valid job is checked before any of its work, against `parity/parity.json` as
//! this program was built with it: a job that reaches a family (a step of the engine, as the
//! harness names them) that this program has no code for, or that `parity certify` has not
//! certified, gets the result code `not-ported`, exit code -8 (the stock command line's
//! `CLI_UNSUPPORTED_OPERATION`, as `unsupported`), and `missing`, those families in
//! parity.json's order. It is refused before any output file is written: the result and the
//! progress file are the run's only files.
//!
//! The families a command always reaches are those of `scripts/lib/parity/corpus.ts`
//! (`familiesFor`) that no setting turns off: model-load and placement for every command, info
//! for info, slice-mesh for layers and slice, and for slice the internal concentric infill, the
//! print steps every slice runs, and the G-code export and its processor. The front end
//! (version, config-def and intake) is decided before a job's work starts, so it is never
//! missing; for now every info, layers and slice job is declined.

use crate::common::Failure;
use crate::job::Command;

/// parity/parity.json as this program was built with it.
const PARITY_JSON: &str = include_str!("../../../parity/parity.json");

/// The families this program has the code for. A family is done only when it is here and
/// parity.json certifies it.
pub const PORTED: &[&str] = &["version", "config-def", "intake"];

/// The families every job of a command reaches, whatever its settings.
fn reached(command: Command) -> &'static [&'static str] {
    match command {
        Command::Info => &["model-load", "placement", "info"],
        Command::Layers => &["model-load", "placement", "slice-mesh"],
        Command::Slice => &[
            "model-load",
            "placement",
            "slice-mesh",
            "infill-concentricinternal",
            "gcode-export",
            "gcode-processor",
            "step-estimate-curled-extrusions",
            "step-prepare-infill",
            "step-simplify-path",
            "step-detect-overhangs-for-lift",
            "step-simplify-wall",
            "step-simplify-infill",
            "step-skirt-brim",
            "step-conflict-check",
        ],
    }
}

/// parity.json's families, in its order, with whether each is certified.
#[must_use]
pub fn families() -> Vec<(String, bool)> {
    let doc = orca_json::parse(PARITY_JSON.as_bytes(), orca_json::Order::Insertion)
        .expect("parity/parity.json is JSON");
    doc.get("families")
        .and_then(orca_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?.to_string();
                    let certified =
                        f.get("certified").and_then(orca_json::Value::as_bool) == Some(true);
                    Some((name, certified))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The families a job of `command` reaches that are not done (ported here and certified), in
/// parity.json's order; one parity.json does not list comes last, in the order of [`reached`].
#[must_use]
pub fn missing(command: Command) -> Vec<String> {
    let all = families();
    let reach = reached(command);
    let done = |name: &str| PORTED.contains(&name) && all.iter().any(|(n, c)| n == name && *c);
    let mut out: Vec<String> = all
        .iter()
        .filter(|(n, _)| reach.contains(&n.as_str()) && !done(n))
        .map(|(n, _)| n.clone())
        .collect();
    for name in reach {
        if !all.iter().any(|(n, _)| n == name) {
            out.push((*name).to_string());
        }
    }
    out
}

/// The decline of a job whose command reaches `missing`.
#[must_use]
pub fn not_ported(command_name: &str, missing: Vec<String>) -> Failure {
    Failure {
        code: "not-ported".to_string(),
        message: format!(
            "sketch-orca-rs cannot do this {command_name} job yet: it reaches steps that are not ported ({})",
            missing.join(", ")
        ),
        object_index: -1,
        exit_code: -8,
        missing,
    }
}

/// `Ok` when this program does every family the job reaches, else its decline.
///
/// # Errors
///
/// The [`not_ported`] failure.
pub fn check(command: Command, command_name: &str) -> Result<(), Failure> {
    let m = missing(command);
    if m.is_empty() {
        Ok(())
    } else {
        Err(not_ported(command_name, m))
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, PORTED, check, families, missing};

    #[test]
    fn every_family_named_is_one_of_parity_json() {
        let all = families();
        assert!(all.len() > 50);
        for c in [Command::Info, Command::Layers, Command::Slice] {
            for m in missing(c) {
                assert!(all.iter().any(|(n, _)| *n == m), "{m}");
            }
        }
        for p in PORTED {
            assert!(all.iter().any(|(n, _)| n == p), "{p}");
        }
    }

    #[test]
    fn every_job_is_declined_for_now_naming_the_steps_in_parity_json_order() {
        assert_eq!(missing(Command::Info), ["model-load", "placement", "info"]);
        assert_eq!(
            missing(Command::Layers),
            ["model-load", "placement", "slice-mesh"]
        );
        let slice = missing(Command::Slice);
        assert_eq!(slice.len(), 14);
        assert_eq!(
            slice[..4],
            [
                "model-load",
                "placement",
                "slice-mesh",
                "infill-concentricinternal"
            ]
        );
        assert_eq!(
            slice.last().map(String::as_str),
            Some("step-conflict-check")
        );
        let f = check(Command::Info, "info").unwrap_err();
        assert_eq!(
            (f.code.as_str(), f.exit_code, f.object_index),
            ("not-ported", -8, -1)
        );
        assert_eq!(
            f.message,
            "sketch-orca-rs cannot do this info job yet: it reaches steps that are not ported (model-load, placement, info)"
        );
    }
}
