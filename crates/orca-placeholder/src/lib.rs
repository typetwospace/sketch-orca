// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: PlaceholderParser::evaluate_boolean_expression and process_macro of
// src/libslic3r/PlaceholderParser.cpp of OrcaSlicer v2.4.2, translated on 2026-11-07. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! Orca's placeholder parser: the conditions of presets and of custom G-code.
//!
//! [`evaluate_boolean_expression`] is `PlaceholderParser::evaluate_boolean_expression`: the
//! expressions of the presets' compatibility conditions (`compatible_printers_condition`,
//! `compatible_prints_condition`), evaluated against a configuration and an optional override as
//! the C++ engine evaluates them, with its errors' texts (`PlaceholderParserError`, whose `what()`
//! the presets stage prints or logs). Phase O4's `orca-gcode` adds the macro language of the
//! custom G-code templates (`process()`), whose rules join the grammar at
//! [`parser::MacroRules`].
//!
//! - [`parser`]: the grammar (Boost.Spirit's semantics: white-space skipping, backtracking,
//!   `>` expectations and their error positions), the variable lookup (the override first, then
//!   the configuration) and `process_error_message` with its tag table.
//! - [`expr`]: the values and their operators.
//! - [`number`]: Spirit's `strict_double` and `int_`.
//! - [`regex`]: `boost::regex` and `regex_match` for the patterns of `=~`, `!~` and `one_of`.
//!
//! Where the C++ code's behaviour is undefined or rests on code not ported (a line width, which
//! `Flow::extrusion_width` gives; an error at a null iterator; a regular expression past what
//! [`regex`] takes), the result is [`Error::Unsupported`], so the job declines rather than
//! guesses (`parity/HAZARDS.md`, orca-placeholder). The records of the probe file
//! `placeholder.sop1`, written by the C++ engine's own parser, prove the rest (`tests/probes.rs`).

use std::collections::BTreeMap;

use orca_config::OptionValue;

pub mod expr;
pub mod number;
pub mod parser;
pub mod regex;

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-placeholder";

/// The options an expression's variables are looked up in: a `DynamicConfig`'s `option(key)`.
pub trait Symbols {
    /// The option named `key`, if the configuration has it.
    fn option(&self, key: &str) -> Option<&OptionValue>;
}

impl Symbols for BTreeMap<String, OptionValue> {
    fn option(&self, key: &str) -> Option<&OptionValue> {
        self.get(key)
    }
}

impl Symbols for orca_config::config::Config {
    fn option(&self, key: &str) -> Option<&OptionValue> {
        self.get(key)
    }
}

/// What evaluating an expression gave besides a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// `Slic3r::PlaceholderParserError`: its `what()`, ending in a newline.
    Parse(String),
    /// A case the translation does not take: the job declines.
    Unsupported(String),
}

/// `PlaceholderParser::evaluate_boolean_expression(templ, config, config_override)`: whether
/// `templ` evaluates to true. A template that does not parse as an expression at all gives false,
/// as the C++ macro processor outputs nothing then.
///
/// # Errors
///
/// [`Error::Parse`] where the C++ code throws `PlaceholderParserError`, with its message;
/// [`Error::Unsupported`] where its behaviour is undefined or not ported.
pub fn evaluate_boolean_expression(
    templ: &[u8],
    config: &dyn Symbols,
    config_override: Option<&dyn Symbols>,
) -> Result<bool, Error> {
    let mut p = parser::Parser::new(templ, config, config_override);
    match p.start() {
        Ok(out) => Ok(out == b"true"),
        Err(expr::Fail::Unsupported(u)) => Err(Error::Unsupported(u)),
        Err(expr::Fail::Expect { at, tag }) => match parser::error_message(templ, at, &tag) {
            Ok(mut m) => {
                // process_macro
                if !m.ends_with('\n') && !m.ends_with('\r') {
                    m.push('\n');
                }
                Err(Error::Parse(m))
            }
            Err(expr::Fail::Unsupported(u)) => Err(Error::Unsupported(u)),
            Err(expr::Fail::Expect { .. }) => unreachable!("error_message fails only unsupported"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BTreeMap<String, OptionValue> {
        let mut c = BTreeMap::new();
        c.insert(
            "printer_notes".to_string(),
            OptionValue::String("PRINTER_VENDOR_PRUSA3D\nPRINTER_MODEL_MK3S".into()),
        );
        c.insert(
            "nozzle_diameter".to_string(),
            OptionValue::Floats {
                values: vec![0.4],
                nullable: false,
            },
        );
        c.insert(
            "single_extruder_multi_material".to_string(),
            OptionValue::Bool(true),
        );
        c
    }

    fn eval(t: &str) -> Result<bool, Error> {
        evaluate_boolean_expression(t.as_bytes(), &config(), None)
    }

    fn parse_error(t: &str) -> String {
        match eval(t) {
            Err(Error::Parse(m)) => m,
            other => panic!("{t}: {other:?}"),
        }
    }

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn vendor_conditions_evaluate() {
        assert_eq!(eval("printer_notes=~/.*PRINTER_MODEL_MK3S.*/"), Ok(true));
        assert_eq!(
            eval("printer_notes=~/.*PRINTER_VENDOR_PRUSA3D.*/ and nozzle_diameter[0]==0.4"),
            Ok(true)
        );
        assert_eq!(
            eval("printer_notes!~/.*MK4.*/ and ! single_extruder_multi_material"),
            Ok(false)
        );
        assert_eq!(eval("nozzle_diameter[0] == 0.6"), Ok(false));
        assert_eq!(eval("true ? 1 > 0 : 2"), Ok(true));
        assert_eq!(
            eval("interpolate_table(1.5, (1,10), (2,20)) == 15"),
            Ok(true)
        );
        assert_eq!(eval("one_of(\"b\", \"a\", \"b\")"), Ok(true));
        assert_eq!(eval("digits(3.14159, 6, 2) == \"  3.14\""), Ok(true));
        // Nothing parses: the macro processor outputs nothing, which is not "true".
        assert_eq!(eval(")"), Ok(false));
    }

    #[test]
    fn errors_have_the_cpp_texts() {
        assert_eq!(
            parse_error("foo == 1"),
            "Parsing error at line 1: Not a variable name\nfoo == 1\n^\n"
        );
        assert_eq!(
            parse_error("1 +"),
            "Parsing error at line 1. Expecting an expression.\n1 +\n   ^\n"
        );
        assert_eq!(
            parse_error("1 == 1 x"),
            "Parsing error at line 1. Unknown syntax error\n1 == 1 x\n       ^\n"
        );
        assert_eq!(
            parse_error("1\n+ \"a\" * 2"),
            "Parsing error at line 2: Cannot multiply with non-numeric type.\n+ \"a\" * 2\n  ^\n"
        );
        assert_eq!(
            parse_error("2"),
            "Parsing error at line 1: Not a boolean expression\n2\n^\n"
        );
        // A skipped branch's floor() still refuses its empty value.
        assert_eq!(
            parse_error("true ? true : floor(2.5) == 2"),
            "Parsing error at line 1: Cannot floor a non-numeric value.\ntrue ? true : floor(2.5) == 2\n                    ^\n"
        );
    }
}
