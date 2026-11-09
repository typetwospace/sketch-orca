// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! XML as expat 2.5.0 parses it, for 3MF files.
//!
//! The C++ engine parses a 3MF file's XML with expat 2.5.0 as its dependency build compiles it
//! (`src/deps/EXPAT/expat` in OrcaSlicer's tree, not `deps_src/expat`): without `XML_DTD`,
//! `XML_NS`, `XML_CONTEXT_BYTES` or `XML_ATTR_INFO`, with `XML_Char` a `char` holding UTF-8. This
//! crate is that parser, translated by the o2-model task (2026-11-08) as `bbs_3mf.cpp` drives
//! it: [`Parser::new`] is `XML_ParserCreate(NULL)`, [`Parser::parse`] is `XML_Parse`,
//! [`Parser::get_buffer`] and [`Parser::parse_buffer`] are `XML_GetBuffer` and `XML_ParseBuffer`,
//! a [`Handler`] gets the start element, end element and character data callbacks in the C's
//! order and splits, and [`error_string`] is `XML_ErrorString`.
//!
//! | Module | What it does | C source |
//! | --- | --- | --- |
//! | `parse` | The parser: buffers, processors, the prolog's declarations, start tags and their attributes, entities, CDATA sections, positions and stopping | `xmlparse.c` |
//! | `role` | The prolog's state machine | `xmlrole.c` |
//! | `tok` | The tokenizer for each encoding, the encodings' conversions to UTF-8, encoding detection and the XML declaration | `xmltok.c`, `xmltok_impl.inc`, `xmltok_ns.inc` |
//! | `tables` | Byte types and naming bitmaps | `asciitab.h`, `iasciitab.h`, `latin1tab.h`, `utf8tab.h`, `nametab.h` |
//! | `error` | `enum XML_Error` and its texts | `expat.h`, `xmlparse.c` |
//!
//! The records of the probe file `xml.sop1` hold it to the C (`tests/probes.rs`).

mod error;
mod parse;
mod role;
mod tables;
mod tok;

pub use error::{XmlError, error_string};
pub use parse::{Ctl, Handler, Parser, Parsing, Status};

/// The crate's name, as Cargo has it.
pub const NAME: &str = "orca-xml";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_the_package_name() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }

    struct Rec(Vec<String>);

    impl Handler for Rec {
        fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
            let atts: Vec<String> = atts
                .iter()
                .map(|a| String::from_utf8_lossy(a).into())
                .collect();
            self.0.push(format!(
                "S {} {} {:?}",
                String::from_utf8_lossy(name),
                ctl.specified_attribute_count(),
                atts
            ));
        }
        fn end_element(&mut self, _: &mut Ctl, name: &[u8]) {
            self.0.push(format!("E {}", String::from_utf8_lossy(name)));
        }
        fn character_data(&mut self, _: &mut Ctl, s: &[u8]) {
            self.0.push(format!("C {:?}", String::from_utf8_lossy(s)));
        }
    }

    #[test]
    fn a_small_document_calls_back_in_order() {
        let doc = b"<?xml version=\"1.0\"?>\n<!DOCTYPE a [<!ATTLIST a d CDATA 'x'>\
<!ENTITY e 'v&#65;'>]><a k=' 1 '>t&amp;&e;<![CDATA[c]]>\r\n<b/></a>";
        let mut p = Parser::new();
        let mut r = Rec(Vec::new());
        assert_eq!(p.parse(&mut r, doc, true), Status::Ok);
        assert_eq!(
            r.0,
            [
                "S a 2 [\"k\", \" 1 \", \"d\", \"x\"]",
                "C \"t\"",
                "C \"&\"",
                "C \"vA\"",
                "C \"c\"",
                "C \"\\n\"",
                "S b 0 []",
                "E b",
                "E a",
            ]
        );
    }

    #[test]
    fn an_error_has_its_code_line_and_column() {
        let mut p = Parser::new();
        let mut r = Rec(Vec::new());
        assert_eq!(p.parse(&mut r, b"<a>\n  <b></c></a>", true), Status::Error);
        assert_eq!(p.error_code(), XmlError::TagMismatch);
        assert_eq!((p.current_line_number(), p.current_column_number()), (2, 7));
        assert_eq!(error_string(p.error_code().code()), Some("mismatched tag"));
        assert_eq!(error_string(0), None);
    }
}
