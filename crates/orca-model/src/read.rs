// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: Model::read_from_file of src/libslic3r/Model.cpp and load_stl of
// src/libslic3r/Format/STL.cpp, of OrcaSlicer v2.4.2, translated on 2026-11-07 (the OBJ branch on
// 2026-11-08, the 3MF branch on 2026-11-10, its end, lines 363-369, on 2026-11-11). Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `Model::read_from_file`: a model file read by its extension (compared without case), its
//! objects given the file's path, and an instance added to each that has none when asked.
//!
//! An STL (`.stl`, or `.oltp` with a 256-byte header) is read and repaired by `orca-mesh`, an OBJ
//! by [`crate::obj`]; an object is named after the text behind the path's last `\`, Windows'
//! separator (so a path written with `/` names it by the whole path). A 3MF is read by
//! [`crate::bbs_3mf`], whole: its sub-model files and its assembly into the model included. It
//! refuses with "Loading of a model file failed." and logs why, or lets an exception escape
//! ([`ReadError::Message`]). It answers [`ReadError::NotPorted`] only at an entry that a load with
//! configuration reads ([`crate::bbs_3mf::LOAD_CONFIG_STAGE`]); the command line's load never
//! reaches one. `.svg`, `.drc` and `.amf` still answer [`ReadError::NotPorted`], for the caller
//! to decline. The errors are the C++ exceptions' texts, byte for byte ([`ReadError::what`]).
//!
//! An ASCII STL whose `solid` line passes 255 bytes (a file with CR line ends is one line) overruns
//! the C++ reader's buffer and the engine dies (0xC0000409, `parity/CORPUS.md`, "An engine fault:
//! 20mmbox-CR.stl"); here it is [`ReadError::OverlongSolidLine`], which the caller answers as it
//! answers a fault.

use std::path::Path;

use orca_mesh::admesh::stlinit::StlError;
use orca_mesh::triangle_mesh::TriangleMesh;

use crate::bbs_3mf::load_bbs_3mf;
use crate::model::Model;
use crate::obj::{ObjInfo, load_obj};
use crate::semver::Semver;

/// `LoadStrategy`: the flags `read_from_file` takes.
pub mod load_strategy {
    /// `Default`.
    pub const DEFAULT: u32 = 0;
    /// `AddDefaultInstances`.
    pub const ADD_DEFAULT_INSTANCES: u32 = 1;
    /// `CheckVersion`.
    pub const CHECK_VERSION: u32 = 2;
    /// `LoadModel`.
    pub const LOAD_MODEL: u32 = 4;
    /// `LoadConfig`.
    pub const LOAD_CONFIG: u32 = 8;
    /// `LoadAuxiliary`.
    pub const LOAD_AUXILIARY: u32 = 16;
    /// `Silence`.
    pub const SILENCE: u32 = 32;
    /// `ImperialUnits`.
    pub const IMPERIAL_UNITS: u32 = 64;
    /// `Restore`.
    pub const RESTORE: u32 = 0x10000 | LOAD_MODEL | LOAD_CONFIG | LOAD_AUXILIARY | SILENCE;
}

/// Why a model file was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// An extension no reader takes.
    UnknownFormat,
    /// The reader failed without a message (an STL that cannot be opened or read, or has no
    /// triangle; a 3MF file the reader refuses, its reasons in the log).
    LoadFailed,
    /// The reader failed with its message (the OBJ reader; an exception that escapes the 3MF
    /// reader).
    Message(String),
    /// The file was read and holds no object.
    Empty,
    /// The CR box: an ASCII STL whose `solid` line passes 255 bytes, where the C++ engine dies.
    OverlongSolidLine {
        /// The bytes of the line read before stopping.
        length: usize,
    },
    /// A reader, or a stage of one, not translated yet (`.svg`, `.drc` and `.amf`, and the 3MF
    /// reader's entries that only a load with configuration reads, [`LOAD_CONFIG_STAGE`]): what
    /// is missing, for the caller to decline.
    ///
    /// [`LOAD_CONFIG_STAGE`]: crate::bbs_3mf::LOAD_CONFIG_STAGE
    NotPorted(&'static str),
}

impl ReadError {
    /// The exception's text (`e.what()`), as the C++ throws it; for the CR box, the reason the
    /// engine dies; for a reader not translated, what is missing.
    #[must_use]
    pub fn what(&self) -> String {
        match self {
            ReadError::UnknownFormat => {
                "Unknown file format. Input file must have .stl, .obj, .amf(.xml) extension."
                    .to_owned()
            }
            ReadError::LoadFailed => "Loading of a model file failed.".to_owned(),
            ReadError::Message(m) => m.clone(),
            ReadError::Empty => "The supplied file couldn't be read because it's empty".to_owned(),
            ReadError::OverlongSolidLine { length } => {
                StlError::OverlongSolidLine { length: *length }.to_string()
            }
            ReadError::NotPorted(what) => format!("the {what} is not translated yet"),
        }
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.what())
    }
}

impl std::error::Error for ReadError {}

/// `boost::algorithm::iends_with(s, suffix)` for an ASCII suffix.
fn iends_with(s: &str, suffix: &str) -> bool {
    let (s, suffix) = (s.as_bytes(), suffix.as_bytes());
    s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// The object name `load_stl` gives a path: the text behind its last `\`.
#[must_use]
pub fn object_name_of(path: &str) -> &str {
    match path.rfind('\\') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// `load_stl(path, model, object_name, nullptr, custom_header_length)`: the file read and repaired
/// into one object (named `object_name`, or after the path).
///
/// # Errors
///
/// [`ReadError::LoadFailed`] when the file cannot be opened or read or has no triangle,
/// [`ReadError::OverlongSolidLine`] for the CR box.
pub fn load_stl(
    path: &str,
    model: &mut Model,
    object_name: Option<&str>,
    custom_header_length: i32,
) -> Result<(), ReadError> {
    let mut mesh = TriangleMesh::default();
    match mesh.read_stl_file(Path::new(path), path, true, custom_header_length) {
        Ok(()) => {}
        Err(StlError::OverlongSolidLine { length }) => {
            return Err(ReadError::OverlongSolidLine { length });
        }
        Err(_) => return Err(ReadError::LoadFailed),
    }
    if mesh.empty() {
        return Err(ReadError::LoadFailed);
    }
    let name = object_name
        .unwrap_or_else(|| object_name_of(path))
        .to_owned();
    model.add_object(&name, path, mesh);
    Ok(())
}

/// `Model::read_from_file(input_file, ..., options)`: the file read by its extension, each object
/// given `input_file`, and with `ADD_DEFAULT_INSTANCES` an instance added to each object that has
/// none.
///
/// # Errors
///
/// [`ReadError`]: the exception the C++ throws, or a reader (or a stage of one) not translated
/// yet.
pub fn read_from_file(input_file: &str, options: u32) -> Result<Model, ReadError> {
    let mut model = Model::default();
    if iends_with(input_file, ".stl") {
        load_stl(input_file, &mut model, None, 80)?;
    } else if iends_with(input_file, ".oltp") {
        load_stl(input_file, &mut model, None, 256)?;
    } else if iends_with(input_file, ".obj") {
        // The colours it finds go to the GUI's dialog, which the command line has not.
        let mut obj_info = ObjInfo::default();
        load_obj(input_file, &mut model, &mut obj_info, None).map_err(ReadError::Message)?;
    } else if iends_with(input_file, ".svg") {
        return Err(ReadError::NotPorted(".svg reader"));
    } else if iends_with(input_file, ".drc") {
        return Err(ReadError::NotPorted(".drc reader"));
    } else if iends_with(input_file, ".amf") {
        return Err(ReadError::NotPorted(".amf reader"));
    } else if iends_with(input_file, ".3mf") {
        // is_xxx is is_bbl_3mf here; it and the file's version are not read again.
        let mut is_xxx = false;
        let mut file_version = Semver::default();
        if !load_bbs_3mf(
            input_file,
            &mut model,
            &mut is_xxx,
            &mut file_version,
            options,
        )? {
            return Err(ReadError::LoadFailed);
        }
    } else {
        return Err(ReadError::UnknownFormat);
    }
    if model.objects.is_empty() {
        return Err(ReadError::Empty);
    }
    for o in &mut model.objects {
        o.input_file = input_file.to_owned();
    }
    if options & load_strategy::ADD_DEFAULT_INSTANCES == load_strategy::ADD_DEFAULT_INSTANCES {
        model.add_default_instances();
    }
    // Lines 363-369 check each plate's custom G-code, which only a load with configuration reads
    // (none here), and sort the configuration's substitutions, of which there are none.
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_extensions() {
        assert_eq!(object_name_of("C:\\a\\b\\box.stl"), "box.stl");
        assert_eq!(object_name_of("C:/a/b/box.stl"), "C:/a/b/box.stl");
        assert!(iends_with("X.STL", ".stl"));
        assert_eq!(
            read_from_file("model.step", 0).unwrap_err().what(),
            "Unknown file format. Input file must have .stl, .obj, .amf(.xml) extension."
        );
        assert_eq!(
            read_from_file("C:\\no\\such\\file.stl", 1).unwrap_err(),
            ReadError::LoadFailed
        );
    }
}
