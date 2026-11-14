// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: parse_int_list (lines 541-562), the loop of
// _BBS_3MF_Importer::_load_model_from_file over the archive's other entries (1872-1995), the
// struct CutObjectInfo (903-914; CutObjectBase is in model.rs since 2026-11-11), and
// _is_svg_shape_file (2377-2380), _extract_cut_information_from_archive (2559-2616),
// _extract_layer_heights_profile_config_from_archive, _extract_layer_config_ranges_from_archive
// and _extract_brim_ear_points_from_archive (2824-3013) and _extract_embossed_svg_shape_file
// (3182-3206), of src/libslic3r/Format/bbs_3mf.cpp of OrcaSlicer v2.4.2, translated on
// 2026-11-11. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The archive's other entries, read after the root model, each by its name: the layer heights
//! profile, the height ranges, the brim ears, the cut information, `model_settings.config`
//! ([`super::config`]) and the svg files of embossed shapes. The rest is logged as skipped.
//!
//! The command line's load has no `LoadConfig`, `LoadAuxiliary` or `Restore`, so the entries
//! those read are not reached: the project's settings, the embedded presets, the custom G-code
//! per height, `slice_info.config`, the filament sequence, the auxiliary files and the
//! `Metadata/` G-code, thumbnails and calibration files. Their branches stand with their
//! conditions as the C++ writes them, so that the chain takes the same branch for every name, and
//! each answers [`super::ReadError::NotPorted`] ([`super::LOAD_CONFIG_STAGE`]) where the C++
//! would read.
//!
//! Each entry is extracted into a `std::string`, or expat's buffer, of the size its central
//! directory record claims. Here nothing is allocated from that claim: [`extract_claimed`] first
//! tells what miniz would do, then allocates what the data holds (`parity/HAZARDS.md`, the 3MF
//! table). The bytes past the data are the string's NULs, counted ([`ClaimedString`]); in expat's
//! buffer they are uninitialized in the C++, and one NUL here.
//!
//! What the readers throw (`std::stoi`'s exceptions, Boost.PropertyTree's errors and
//! `set_deserialize`'s) is not caught in the C++ between the reader and `load_model_from_file`'s
//! caller: here it is the `Err` with its `what()`, which `load_from_archive` turns into
//! [`super::ReadError::Message`].

use std::convert::Infallible;
use std::sync::Arc;

use orca_eigen::Vec3f;
use orca_mesh::log::{Severity, log};
use orca_xml::{PtreeError, read_xml};
use orca_zip::reader::{FileStat, ZipArchive};

use super::names::{
    AUXILIARY_DIR, BBS_LAYER_HEIGHTS_PROFILE_FILE, BBS_MODEL_CONFIG_FILE, BBS_PROJECT_CONFIG_FILE,
    BRIM_EAR_POINTS_FILE, CALIBRATION_INFO_EXTENSION, CUSTOM_GCODE_PER_PRINT_Z_FILE,
    CUT_INFORMATION_FILE, FILAMENT_SEQUENCE_FILE, GCODE_EXTENSION, LAYER_CONFIG_RANGES_FILE,
    METADATA_DIR, MODEL_FOLDER, PROJECT_EMBEDDED_FILAMENT_PRESETS_FILE,
    PROJECT_EMBEDDED_PRINT_PRESETS_FILE, PROJECT_EMBEDDED_PRINTER_PRESETS_FILE,
    PROJECT_EMBEDDED_SLICE_PRESETS_FILE, SLICE_INFO_CONFIG_FILE, THUMBNAIL_EXTENSION,
};
use super::{Importer, LOAD_CONFIG_STAGE, LOAD_MODEL_FROM_FILE, ReadError, XmlHandlers};
use crate::config::ModelConfig;
use crate::emboss::ClaimedString;
use crate::model::{BrimPoint, CutObjectBase, LayerConfigRanges, Model};

/// `CutObjectInfo::Connector`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Connector {
    pub(super) volume_id: i32,
    pub(super) ty: i32,
    pub(super) r_tolerance: f32,
    pub(super) h_tolerance: f32,
}

/// `CutObjectInfo`: the cut id ([`CutObjectBase`], `ObjectID.hpp`, in [`crate::model`]) and the
/// connectors.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CutObjectInfo {
    pub(super) id: CutObjectBase,
    pub(super) connectors: Vec<Connector>,
}

/// `parse_int_list(value)`: the integers in the text, split at runs of spaces and commas, each
/// read whole by `boost::lexical_cast<int>` (one it refuses is dropped, `:553-556`), then sorted
/// and made unique.
pub(super) fn parse_int_list(value: &str) -> Vec<i32> {
    let mut out = Vec::new();
    if value.is_empty() {
        return out;
    }
    // boost::split(tokens, value, boost::is_any_of(" ,"), boost::token_compress_on)
    for token in value.as_bytes().split(|&c| c == b' ' || c == b',') {
        if token.is_empty() {
            continue;
        }
        if let Some(v) = super::lexical_cast::lexical_cast_int(token) {
            out.push(v);
        }
    }
    orca_stl::sort(&mut out);
    orca_stl::select::unique_vec(&mut out);
    out
}

/// `std::stoi(str)` on `str.c_str()`, with the texts MSVC throws: "invalid stoi argument" or
/// "stoi argument out of range".
pub(super) fn stoi(s: &[u8]) -> Result<i32, String> {
    let s = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())];
    match orca_fmt::parse::stoi(s, 10) {
        Ok((v, _)) => Ok(v),
        Err(orca_fmt::parse::StoError::InvalidArgument) => Err("invalid stoi argument".to_owned()),
        Err(orca_fmt::parse::StoError::OutOfRange) => Err("stoi argument out of range".to_owned()),
    }
}

/// `boost::algorithm::iequals(name, s)` in the classic locale.
fn iequals(name: &[u8], s: &str) -> bool {
    name.eq_ignore_ascii_case(s.as_bytes())
}

/// `boost::algorithm::istarts_with(name, s)` in the classic locale.
fn istarts_with(name: &[u8], s: &str) -> bool {
    name.len() >= s.len() && name[..s.len()].eq_ignore_ascii_case(s.as_bytes())
}

/// `boost::algorithm::iends_with(name, s)` in the classic locale.
fn iends_with(name: &[u8], s: &str) -> bool {
    name.len() >= s.len() && name[name.len() - s.len()..].eq_ignore_ascii_case(s.as_bytes())
}

/// `_is_svg_shape_file(name)`: in `3D/` and ending in `.svg`, case and all.
fn is_svg_shape_file(name: &[u8]) -> bool {
    name.starts_with(MODEL_FOLDER.as_bytes()) && name.ends_with(b".svg")
}

/// `boost::split(out, s, boost::is_any_of(seps), boost::token_compress_off)`.
fn split_any<'a>(s: &'a [u8], seps: &[u8]) -> Vec<&'a [u8]> {
    s.split(|c| seps.contains(c)).collect()
}

/// `std::atoi(s.c_str())`.
fn atoi(s: &[u8]) -> i32 {
    orca_fmt::parse::atoi(s)
}

/// `std::atof(s.c_str())`.
fn atof(s: &[u8]) -> f64 {
    orca_fmt::parse::atof(s)
}

/// A Boost.PropertyTree error's `what()`.
fn ptree_what(e: &PtreeError) -> String {
    String::from_utf8_lossy(e.what()).into_owned()
}

/// What miniz does with an extraction into a buffer of the claimed size, known before anything
/// is allocated.
enum Plan {
    /// The extraction fails: the entry is not there, is encrypted or compressed some other way,
    /// is larger than the buffer, or its data cannot fill its own claim.
    Fails,
    /// The extraction succeeds without writing (a directory, or no compressed data).
    Untouched,
    /// The extraction writes this many bytes, if the data holds them.
    Writes(u64),
}

/// The plan for extracting the entry `index` into `claim` bytes, following
/// `mz_zip_reader_extract_to_mem_no_alloc`'s checks (`orca-zip`'s `begin_extract` and the size
/// test). The claim the entry makes for itself is checked against its data without keeping the
/// data: a stored entry's bytes lie in the archive, so they are not more than its size; a
/// deflated entry is inflated once and its bytes counted, so that only data that inflates to its
/// claim is given a buffer. Inflating to the claim is what `mz_zip_reader_extract_to_mem` needs
/// to succeed, so no extraction that succeeds is refused here.
fn plan(archive: &mut ZipArchive, index: u32, claim: u64) -> Plan {
    let mut st = FileStat::default();
    if !archive.reader_file_stat(index, &mut st) {
        return Plan::Fails;
    }
    if st.m_is_directory || st.m_comp_size == 0 {
        return Plan::Untouched;
    }
    // MZ_ZIP_GENERAL_PURPOSE_BIT_FLAG_IS_ENCRYPTED, _USES_STRONG_ENCRYPTION, _COMPRESSED_PATCH_FLAG
    if st.m_bit_flag & (1 | 64 | 32) != 0 {
        return Plan::Fails;
    }
    if st.m_method != 0 && st.m_method != 8 {
        return Plan::Fails;
    }
    // MZ_ZIP_BUF_TOO_SMALL
    if claim < st.m_uncomp_size {
        return Plan::Fails;
    }
    let n = st.m_uncomp_size;
    if st.m_method == 0 {
        if n > archive.m_archive_size {
            return Plan::Fails;
        }
    } else {
        let mut count = 0u64;
        let mut tally = |_: u64, chunk: &[u8]| -> Result<usize, Infallible> {
            count += chunk.len() as u64;
            Ok(if count > n { 0 } else { chunk.len() })
        };
        let Ok(_) = archive.reader_extract_to_callback(index, &mut tally, 0);
        if count != n {
            return Plan::Fails;
        }
    }
    Plan::Writes(n)
}

/// `mz_zip_reader_locate_file(archive, name, nullptr, 0)`, the index or `None`, as
/// `mz_zip_reader_extract_file_to_mem` finds the entry it extracts.
pub(super) fn locate(archive: &mut ZipArchive, name: &[u8]) -> Option<u32> {
    u32::try_from(archive.reader_locate_file(name, None, 0)).ok()
}

/// `std::string buffer(claim, 0)` and `mz_zip_reader_extract_to_mem(archive, index, buffer,
/// claim, 0)` (or `mz_zip_reader_extract_file_to_mem` with the index it finds by the name): the
/// string, or `None` when the extraction fails. Nothing is allocated from the claim
/// ([`plan`]).
pub(super) fn extract_claimed(
    archive: &mut ZipArchive,
    index: Option<u32>,
    claim: u64,
) -> Option<ClaimedString> {
    let index = index?;
    match plan(archive, index, claim) {
        Plan::Fails => None,
        Plan::Untouched => Some(ClaimedString {
            bytes: Vec::new(),
            nuls: claim,
        }),
        Plan::Writes(n) => {
            let mut bytes = vec![0u8; usize::try_from(n).ok()?];
            if !archive.reader_extract_to_mem(index, &mut bytes, 0) {
                return None;
            }
            Some(ClaimedString {
                bytes,
                nuls: claim - n,
            })
        }
    }
}

/// `config_ranges[{min_z, max_z}]`: the range's settings, made empty when no range is equivalent
/// under the pair's `<`.
fn range_entry(ranges: &mut LayerConfigRanges, key: (f64, f64)) -> &mut ModelConfig {
    // std::pair's operator<, as written
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    let less = |a: &(f64, f64), b: &(f64, f64)| a.0 < b.0 || (!(b.0 < a.0) && a.1 < b.1);
    let pos = orca_stl::select::lower_bound_by(ranges, &key, |(k, _), key| less(k, key));
    if pos == ranges.len() || less(&key, &ranges[pos].0) {
        ranges.insert(pos, (key, ModelConfig::new()));
    }
    &mut ranges[pos].1
}

impl Importer {
    /// The loop over the archive's entries after the root model (`_load_model_from_file`, lines
    /// 1872-1995), with no progress callback. `Ok(false)` where the C++ returns false; `Err` for
    /// an exception that escapes, or a branch not ported.
    pub(super) fn extract_other_entries(
        &mut self,
        model: &mut Model,
        archive: &mut ZipArchive,
        num_entries: u32,
    ) -> Result<bool, ReadError> {
        let dont_load_config = !self.m_load_config;
        let mut stat = FileStat::default();
        // we then loop again the entries to read other files stored in the archive
        for i in 0..num_entries {
            if archive.reader_file_stat(i, &mut stat) {
                let name: Vec<u8> = stat
                    .m_filename
                    .iter()
                    .map(|&c| if c == b'\\' { b'/' } else { c })
                    .collect();
                let shown = String::from_utf8_lossy(&name).into_owned();

                log(
                    Severity::Info,
                    &format!(
                        "{LOAD_MODEL_FROM_FILE}:1886extract {}th file {shown}, total={num_entries}",
                        i + 1
                    ),
                );

                if name.windows(4).any(|w| w == b"/../")
                    || name.starts_with(b"../")
                    || (name.len() >= 3 && name.ends_with(b"/.."))
                    || name == b".."
                {
                    log(
                        Severity::Warning,
                        &format!(
                            "{LOAD_MODEL_FROM_FILE}, find file path including path traversal, \
                            not valid, skip it\n"
                        ),
                    );
                    continue;
                }

                if iequals(&name, BBS_LAYER_HEIGHTS_PROFILE_FILE) {
                    // extract slic3r layer heights profile file
                    self.extract_layer_heights_profile_config_from_archive(archive, &stat);
                } else if iequals(&name, LAYER_CONFIG_RANGES_FILE) {
                    // extract slic3r layer config ranges file
                    self.extract_layer_config_ranges_from_archive(archive, &stat)
                        .map_err(ReadError::Message)?;
                } else if iequals(&name, BRIM_EAR_POINTS_FILE) {
                    // extract slic3r config file
                    self.extract_brim_ear_points_from_archive(archive, &stat)
                        .map_err(ReadError::Message)?;
                }
                //BBS: disable SLA related files currently
                //BBS: project setting file
                if !dont_load_config && iequals(&name, BBS_PROJECT_CONFIG_FILE) {
                    // extract slic3r print config file
                    return Err(ReadError::NotPorted(LOAD_CONFIG_STAGE));
                } else if iequals(&name, CUT_INFORMATION_FILE) {
                    // extract object cut info
                    self.extract_cut_information_from_archive(archive, &stat)
                        .map_err(ReadError::Message)?;
                }
                //BBS: project embedded presets
                else if !dont_load_config
                    && (istarts_with(&name, PROJECT_EMBEDDED_PRINT_PRESETS_FILE)
                        || istarts_with(&name, PROJECT_EMBEDDED_SLICE_PRESETS_FILE)
                        || istarts_with(&name, PROJECT_EMBEDDED_FILAMENT_PRESETS_FILE)
                        || istarts_with(&name, PROJECT_EMBEDDED_PRINTER_PRESETS_FILE)
                        || iequals(&name, CUSTOM_GCODE_PER_PRINT_Z_FILE))
                {
                    // the four kinds of embedded presets, then the custom G-code per height
                    return Err(ReadError::NotPorted(LOAD_CONFIG_STAGE));
                } else if iequals(&name, BBS_MODEL_CONFIG_FILE) {
                    // extract slic3r model config file
                    if !self
                        .extract_xml_from_archive_stat(model, archive, &stat, XmlHandlers::Config)
                        .map_err(ReadError::Message)?
                        && self.m_is_bbl_3mf
                    {
                        self.add_error("Archive does not contain a valid model config");
                        return Ok(false);
                    }
                } else if is_svg_shape_file(&name) {
                    self.extract_embossed_svg_shape_file(&name, archive, &stat);
                } else if !dont_load_config
                    && (iequals(&name, SLICE_INFO_CONFIG_FILE)
                        || iequals(&name, FILAMENT_SEQUENCE_FILE))
                {
                    // slice_info.config (with m_parsing_slice_info set), the filament sequence
                    return Err(ReadError::NotPorted(LOAD_CONFIG_STAGE));
                } else if istarts_with(&name, AUXILIARY_DIR) {
                    // extract auxiliary directory to temp directory, do nothing for restore
                    if self.m_load_aux && !self.m_load_restore {
                        return Err(ReadError::NotPorted(LOAD_CONFIG_STAGE));
                    }
                } else if !dont_load_config
                    && istarts_with(&name, METADATA_DIR)
                    && (iends_with(&name, GCODE_EXTENSION)
                        || iends_with(&name, THUMBNAIL_EXTENSION)
                        || iends_with(&name, CALIBRATION_INFO_EXTENSION))
                {
                    //load gcode files, the pattern and plate thumbnails, the pattern config files
                    return Err(ReadError::NotPorted(LOAD_CONFIG_STAGE));
                } else {
                    log(
                        Severity::Info,
                        &format!(
                            "{LOAD_MODEL_FROM_FILE}:1991, {shown} skipped, \
                            already parsed or a directory or not supported\n"
                        ),
                    );
                }
            }
        }
        Ok(true)
    }

    /// `_extract_cut_information_from_archive(archive, stat, ...)`.
    ///
    /// `CutObjectBase(ObjectID(id), check_sum, connectors_cnt)` reads its three attributes in an
    /// order C++ leaves unspecified. MSVC reads them right to left, `connectors_cnt` first, then
    /// `check_sum`, then `id`, so the first that fails is the error (probed; `parity/HAZARDS.md`,
    /// the 3MF table).
    fn extract_cut_information_from_archive(
        &mut self,
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) -> Result<(), String> {
        if stat.m_uncomp_size > 0 {
            let index = locate(archive, &stat.m_filename);
            let Some(buffer) = extract_claimed(archive, index, stat.m_uncomp_size) else {
                self.add_error("Error while reading cut information data to buffer");
                return Ok(());
            };

            // wrap returned xml to istringstream
            let objects_tree = read_xml(&buffer.text()).map_err(|e| e.what())?;

            let objects = objects_tree
                .get_child(b"objects")
                .map_err(|e| ptree_what(&e))?;
            for (_, object_tree) in objects.children() {
                let obj_idx = object_tree.get_or::<i32>(b"<xmlattr>.id", -1);
                if obj_idx <= 0 {
                    self.add_error("Found invalid object id");
                    continue;
                }

                if self.m_cut_object_infos.contains_key(&obj_idx) {
                    self.add_error("Found duplicated cut_object_id");
                    continue;
                }

                let mut cut_id = CutObjectBase::default();
                let mut connectors = Vec::new();

                for (key, sub_tree) in object_tree.children() {
                    if key == b"cut_id" {
                        let get =
                            |path: &[u8]| sub_tree.get::<u64>(path).map_err(|e| ptree_what(&e));
                        // the call's arguments, as MSVC evaluates them: right to left
                        let connectors_cnt = get(b"<xmlattr>.connectors_cnt")?;
                        let check_sum = get(b"<xmlattr>.check_sum")?;
                        let id = get(b"<xmlattr>.id")?;
                        cut_id = CutObjectBase {
                            id,
                            check_sum,
                            connectors_cnt,
                        };
                    }
                    if key == b"connectors" {
                        for (ckey, connector_tree) in sub_tree.children() {
                            if ckey != b"connector" {
                                continue;
                            }
                            let int = |path: &[u8]| {
                                connector_tree.get::<i32>(path).map_err(|e| ptree_what(&e))
                            };
                            let float = |path: &[u8]| {
                                connector_tree.get::<f32>(path).map_err(|e| ptree_what(&e))
                            };
                            let connector = Connector {
                                volume_id: int(b"<xmlattr>.volume_id")?,
                                ty: int(b"<xmlattr>.type")?,
                                r_tolerance: float(b"<xmlattr>.r_tolerance")?,
                                h_tolerance: float(b"<xmlattr>.h_tolerance")?,
                            };
                            connectors.push(connector);
                        }
                    }
                }

                self.m_cut_object_infos
                    .entry(obj_idx)
                    .or_insert(CutObjectInfo {
                        id: cut_id,
                        connectors,
                    });
            }
        }
        Ok(())
    }

    /// `_extract_layer_heights_profile_config_from_archive(archive, stat)`.
    fn extract_layer_heights_profile_config_from_archive(
        &mut self,
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) {
        if stat.m_uncomp_size > 0 {
            let Some(buffer) =
                extract_claimed(archive, Some(stat.m_file_index), stat.m_uncomp_size)
            else {
                self.add_error("Error while reading layer heights profile data to buffer");
                return;
            };
            let mut buffer = buffer.text();

            if buffer.last() == Some(&b'\n') {
                buffer.pop();
            }

            for object in split_any(&buffer, b"\n") {
                let object_data = split_any(object, b"|");
                if object_data.len() != 2 {
                    self.add_error("Error while reading object data");
                    continue;
                }

                let object_data_id = split_any(object_data[0], b"=");
                if object_data_id.len() != 2 {
                    self.add_error("Error while reading object id");
                    continue;
                }

                let object_id = atoi(object_data_id[1]);
                if object_id == 0 {
                    self.add_error("Found invalid object id");
                    continue;
                }

                if self.m_layer_heights_profiles.contains_key(&object_id) {
                    self.add_error("Found duplicated layer heights profile");
                    continue;
                }

                let object_data_profile = split_any(object_data[1], b";");
                if object_data_profile.len() <= 4 || !object_data_profile.len().is_multiple_of(2) {
                    self.add_error("Found invalid layer heights profile");
                    continue;
                }

                let profile: Vec<f64> = object_data_profile.iter().map(|v| atof(v)).collect();

                self.m_layer_heights_profiles.insert(object_id, profile);
            }
        }
    }

    /// `_extract_layer_config_ranges_from_archive(archive, stat, ...)`.
    fn extract_layer_config_ranges_from_archive(
        &mut self,
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) -> Result<(), String> {
        if stat.m_uncomp_size > 0 {
            let index = locate(archive, &stat.m_filename);
            let Some(buffer) = extract_claimed(archive, index, stat.m_uncomp_size) else {
                self.add_error("Error while reading layer config ranges data to buffer");
                return Ok(());
            };

            // wrap returned xml to istringstream
            let objects_tree = read_xml(&buffer.text()).map_err(|e| e.what())?;

            let objects = objects_tree
                .get_child(b"objects")
                .map_err(|e| ptree_what(&e))?;
            for (_, object_tree) in objects.children() {
                let obj_idx = object_tree.get_or::<i32>(b"<xmlattr>.id", -1);
                if obj_idx <= 0 {
                    self.add_error("Found invalid object id");
                    continue;
                }

                if self.m_layer_config_ranges.contains_key(&obj_idx) {
                    self.add_error("Found duplicated layer config range");
                    continue;
                }

                let mut config_ranges = LayerConfigRanges::new();

                for (key, range_tree) in object_tree.children() {
                    if key != b"range" {
                        continue;
                    }
                    let get = |path: &[u8]| range_tree.get::<f64>(path).map_err(|e| ptree_what(&e));
                    let min_z = get(b"<xmlattr>.min_z")?;
                    let max_z = get(b"<xmlattr>.max_z")?;

                    // get Z range information
                    let mut config = ModelConfig::new();

                    for (okey, option) in range_tree.children() {
                        if okey != b"option" {
                            continue;
                        }
                        let opt_key = option
                            .get::<Vec<u8>>(b"<xmlattr>.opt_key")
                            .map_err(|e| ptree_what(&e))?;
                        let value = option.data();

                        config
                            .set_deserialize(
                                &String::from_utf8_lossy(&opt_key),
                                &String::from_utf8_lossy(value),
                            )
                            .map_err(|e| e.what())?;
                    }

                    // assign_config: the range's settings replaced
                    *range_entry(&mut config_ranges, (min_z, max_z)) = config;
                }

                if !config_ranges.is_empty() {
                    self.m_layer_config_ranges
                        .entry(obj_idx)
                        .or_insert(config_ranges);
                }
            }
        }
        Ok(())
    }

    /// `_extract_brim_ear_points_from_archive(archive, stat)`.
    ///
    /// Version 0 reads the points four numbers at a time with no test that four are left: a last
    /// group of fewer is read past the list's end in the C++ (undefined); here it is dropped
    /// (`parity/HAZARDS.md`, the 3MF table).
    fn extract_brim_ear_points_from_archive(
        &mut self,
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) -> Result<(), String> {
        if stat.m_uncomp_size > 0 {
            let Some(buffer) =
                extract_claimed(archive, Some(stat.m_file_index), stat.m_uncomp_size)
            else {
                self.add_error("Error while reading brim ear points data to buffer");
                return Ok(());
            };
            let mut buffer = buffer.text();

            if buffer.last() == Some(&b'\n') {
                buffer.pop();
            }

            let mut objects: Vec<Vec<u8>> = split_any(&buffer, b"\n")
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect();

            // Info on format versioning - see bbs_3mf.hpp
            let mut version = 0;
            let key = b"brim_points_format_version=";
            if !objects.is_empty() && objects[0].windows(key.len()).any(|w| w == key) {
                objects[0].drain(..key.len()); // removes the string
                version = stoi(&objects[0])?;
                objects.remove(0); // pop the header
            }

            for object in &objects {
                let object_data = split_any(object, b"|");

                if object_data.len() != 2 {
                    self.add_error("Error while reading object data");
                    continue;
                }

                let object_data_id = split_any(object_data[0], b"=");
                if object_data_id.len() != 2 {
                    self.add_error("Error while reading object id");
                    continue;
                }

                let object_id = atoi(object_data_id[1]);
                if object_id == 0 {
                    self.add_error("Found invalid object id");
                    continue;
                }

                if self.m_brim_ear_points.contains_key(&object_id) {
                    self.add_error("Found duplicated brim ear points");
                    continue;
                }

                let object_data_points = split_any(object_data[1], b" ");

                let mut brim_ear_points = Vec::new();
                if version == 0 {
                    #[allow(clippy::cast_possible_truncation)]
                    for p in object_data_points.chunks_exact(4) {
                        brim_ear_points.push(BrimPoint {
                            pos: Vec3f::new(
                                atof(p[0]) as f32,
                                atof(p[1]) as f32,
                                atof(p[2]) as f32,
                            ),
                            head_front_radius: atof(p[3]) as f32,
                        });
                    }
                }

                if !brim_ear_points.is_empty() {
                    self.m_brim_ear_points
                        .entry(object_id)
                        .or_insert(brim_ear_points);
                }
            }
        }
        Ok(())
    }

    /// `_extract_embossed_svg_shape_file(filename, archive, stat)`: the file kept by its name,
    /// one before of the same name replaced.
    fn extract_embossed_svg_shape_file(
        &mut self,
        filename: &[u8],
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) {
        let Some(file) = extract_claimed(archive, Some(stat.m_file_index), stat.m_uncomp_size)
        else {
            self.add_error("Error while reading svg shape for emboss");
            return;
        };

        // store for case svg is loaded before volume
        self.m_path_to_emboss_shape_files
            .insert(filename.to_vec(), Arc::new(file));

        // find embossed volume, for case svg is loaded after volume: the volumes are made after
        // this loop (piece 3), so the model's objects have none yet and none is found.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values read from bbs_3mf.cpp:541-562 (tokens split at runs of ' ' and ',', a
    // refused token dropped, sorted, unique), 2377-2380 and Boost's iequals and istarts_with.
    #[test]
    fn lists_and_names() {
        assert_eq!(parse_int_list(""), Vec::<i32>::new());
        assert_eq!(
            parse_int_list("3, 1,,2 x 1 +4 -0 2147483648"),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(parse_int_list(" ,"), Vec::<i32>::new());
        assert!(iequals(
            b"metadata/MODEL_settings.config",
            BBS_MODEL_CONFIG_FILE
        ));
        assert!(!iequals(
            b"Metadata/model_settings.config2",
            BBS_MODEL_CONFIG_FILE
        ));
        assert!(istarts_with(b"AUXILIARIES/a.png", AUXILIARY_DIR));
        assert!(iends_with(b"Metadata/plate_1.PNG", THUMBNAIL_EXTENSION));
        assert!(is_svg_shape_file(b"3D/a/b.svg"));
        assert!(!is_svg_shape_file(b"3d/a.svg"));
        assert!(!is_svg_shape_file(b"3D/a.SVG"));
        assert_eq!(stoi(b" -12x"), Ok(-12));
        assert_eq!(stoi(b"\0 1"), Err("invalid stoi argument".to_owned()));
        assert_eq!(
            stoi(b"2147483648"),
            Err("stoi argument out of range".to_owned())
        );
    }

    // Expected values read from std::map<std::pair<double, double>, ModelConfig>::operator[]:
    // ranges in order, an equivalent key (0 and -0) found again.
    #[test]
    fn ranges_by_their_heights() {
        let mut r = LayerConfigRanges::new();
        range_entry(&mut r, (2.0, 3.0));
        range_entry(&mut r, (0.0, 1.0));
        range_entry(&mut r, (0.0, 0.5));
        range_entry(&mut r, (-0.0, 1.0))
            .set_deserialize("layer_height", "0.1")
            .unwrap();
        let keys: Vec<(f64, f64)> = r.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec![(0.0, 0.5), (0.0, 1.0), (2.0, 3.0)]);
        assert!(r[1].1.has("layer_height"));
        assert!(r[1].0.0.is_sign_positive());
    }

    /// One entry of a test archive: its name, method, the time word (a stored entry whose
    /// method and time read as one zero word must claim its own size, miniz.c's open check),
    /// the bytes written as its data, and the sizes its headers claim.
    struct Entry<'a> {
        name: &'a [u8],
        method: u16,
        time: u16,
        data: Vec<u8>,
        crc: u32,
        comp: u32,
        uncomp: u32,
    }

    impl<'a> Entry<'a> {
        /// A stored entry that claims what it holds.
        fn stored(name: &'a [u8], text: &[u8]) -> Self {
            let n = u32::try_from(text.len()).unwrap();
            Entry {
                name,
                method: 0,
                time: 0,
                data: text.to_vec(),
                crc: orca_zip::crc::mz_crc32(orca_zip::crc::MZ_CRC32_INIT, text),
                comp: n,
                uncomp: n,
            }
        }

        /// A deflated entry (one stored deflate block) that claims `uncomp` bytes.
        fn deflated(name: &'a [u8], text: &[u8], uncomp: u32) -> Self {
            let len = u16::try_from(text.len()).unwrap();
            let mut data = vec![1u8];
            data.extend_from_slice(&len.to_le_bytes());
            data.extend_from_slice(&(!len).to_le_bytes());
            data.extend_from_slice(text);
            Entry {
                name,
                method: 8,
                time: 0,
                comp: u32::try_from(data.len()).unwrap(),
                data,
                crc: orca_zip::crc::mz_crc32(orca_zip::crc::MZ_CRC32_INIT, text),
                uncomp,
            }
        }
    }

    /// The archive's bytes: each local header and its data, the central directory, its end.
    fn pack(entries: &[Entry]) -> Vec<u8> {
        fn header(out: &mut Vec<u8>, e: &Entry) {
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&e.method.to_le_bytes());
            out.extend_from_slice(&e.time.to_le_bytes());
            out.extend_from_slice(&0x5b21u16.to_le_bytes()); // date
            out.extend_from_slice(&e.crc.to_le_bytes());
            out.extend_from_slice(&e.comp.to_le_bytes());
            out.extend_from_slice(&e.uncomp.to_le_bytes());
            out.extend_from_slice(&u16::try_from(e.name.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
        }
        let mut out = Vec::new();
        let mut offsets = Vec::new();
        for e in entries {
            offsets.push(u32::try_from(out.len()).unwrap());
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            header(&mut out, e);
            out.extend_from_slice(e.name);
            out.extend_from_slice(&e.data);
        }
        let cdir_ofs = u32::try_from(out.len()).unwrap();
        for (e, ofs) in entries.iter().zip(&offsets) {
            out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version made by
            header(&mut out, e);
            out.extend_from_slice(&[0u8; 6]); // comment, disk, internal attributes
            out.extend_from_slice(&0u32.to_le_bytes()); // external attributes
            out.extend_from_slice(&ofs.to_le_bytes());
            out.extend_from_slice(e.name);
        }
        let cdir_size = u32::try_from(out.len()).unwrap() - cdir_ofs;
        let n = u16::try_from(entries.len()).unwrap();
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]); // this disk, the directory's disk
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&cdir_size.to_le_bytes());
        out.extend_from_slice(&cdir_ofs.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment
        out
    }

    /// The test's scratch folder.
    fn scratch(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("orca-model-claims-{}-{tag}", std::process::id()))
    }

    /// The archive written to a scratch file and opened as the importer opens it.
    fn open(tag: &str, bytes: &[u8]) -> ZipArchive {
        let dir = scratch(tag);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.zip");
        std::fs::write(&path, bytes).unwrap();
        let mut archive = ZipArchive::default();
        assert!(
            orca_zip::extension::open_zip_reader(&mut archive, &path),
            "{tag}: the archive opens"
        );
        // The reader holds the file open; Windows keeps the folder until it is closed.
        archive
    }

    fn close(tag: &str, mut archive: ZipArchive) {
        orca_zip::extension::close_zip_reader(&mut archive);
        std::fs::remove_dir_all(scratch(tag)).unwrap();
    }

    fn stat(archive: &mut ZipArchive, index: u32) -> FileStat {
        let mut st = FileStat::default();
        assert!(archive.reader_file_stat(index, &mut st));
        st
    }

    // Expected values read from miniz's mz_zip_reader_extract_to_mem_no_alloc
    // (deps_src/miniz/miniz.c: a directory or an entry of no compressed data succeeds without
    // writing; a buffer below the entry's own claim is MZ_ZIP_BUF_TOO_SMALL; a stored entry reads
    // its claim from the archive; a deflated one must inflate to its claim exactly,
    // MZ_ZIP_UNEXPECTED_DECOMPRESSED_SIZE) and std::string buffer(claim, 0), as the records of
    // orca-3mf-probe-3 (3mf-2-claim-*, 3mf-2-dup-cut-*) show them.
    #[test]
    fn claims_without_allocating_from_them() {
        let text = b"object_id=1|0;0.2;1;0.2;2;0.2\n";
        let n = u32::try_from(text.len()).unwrap();

        // A deflated entry that claims what it inflates to; a buffer larger than the claim keeps
        // NULs past the bytes, and a smaller one fails.
        let mut a = open("deflated", &pack(&[Entry::deflated(b"h", text, n)]));
        let got = extract_claimed(&mut a, Some(0), u64::from(n)).unwrap();
        assert_eq!((got.bytes.as_slice(), got.nuls), (&text[..], 0));
        let got = extract_claimed(&mut a, Some(0), u64::from(n) + 4).unwrap();
        assert_eq!((got.bytes.as_slice(), got.nuls), (&text[..], 4));
        assert!(extract_claimed(&mut a, Some(0), u64::from(n) - 1).is_none());
        assert!(extract_claimed(&mut a, None, u64::from(n)).is_none());
        close("deflated", a);

        // A deflated over-claim (4 bytes more than it inflates to) and an under-claim fail.
        let mut a = open(
            "over",
            &pack(&[
                Entry::deflated(b"over", text, n + 4),
                Entry::deflated(b"under", text, n - 4),
            ]),
        );
        let over = stat(&mut a, 0).m_uncomp_size;
        assert_eq!(over, u64::from(n) + 4);
        let under = stat(&mut a, 1).m_uncomp_size;
        // The plan fails them before any buffer: a plan that allocated from the claim would fail
        // later, at the read, with the same answer, so the extractions alone cannot tell.
        assert!(matches!(plan(&mut a, 0, over), Plan::Fails));
        assert!(matches!(plan(&mut a, 1, under), Plan::Fails));
        assert!(extract_claimed(&mut a, Some(0), over).is_none());
        assert!(extract_claimed(&mut a, Some(1), under).is_none());
        assert!(extract_claimed(&mut a, Some(1), over).is_none());
        close("over", a);

        // A stored entry whose uncompressed size claims past the archive (its time word makes
        // miniz's open take it): refused before any buffer, as the read past the end fails.
        let mut past = Entry::stored(b"past", text);
        past.time = 0x6000;
        past.uncomp = 1 << 20;
        let bytes = pack(&[past]);
        let mut a = open("past", &bytes);
        let claim = stat(&mut a, 0).m_uncomp_size;
        assert!(claim > a.m_archive_size);
        assert!(matches!(plan(&mut a, 0, claim), Plan::Fails));
        assert!(extract_claimed(&mut a, Some(0), claim).is_none());
        close("past", a);

        // A stored entry whose two sizes claim up to the archive's end (local header offset 0,
        // less its 30 bytes): its buffer is planned, and the read runs past the archive by the
        // name's length, which fails.
        let probe_len = pack(&[Entry::stored(b"end", text)]).len();
        let mut end = Entry::stored(b"end", text);
        end.comp = u32::try_from(probe_len - 30).unwrap();
        end.uncomp = end.comp;
        let bytes = pack(&[end]);
        assert_eq!(bytes.len(), probe_len);
        let mut a = open("end", &bytes);
        let claim = stat(&mut a, 0).m_uncomp_size;
        assert!(matches!(plan(&mut a, 0, claim), Plan::Writes(c) if c == claim));
        assert!(extract_claimed(&mut a, Some(0), claim).is_none());
        close("end", a);
    }

    // As the test above. An entry of no compressed data is reached by a reader through another
    // entry's claim: the stat of one entry of a name, the extraction of the other that
    // mz_zip_reader_extract_file_to_mem finds by the name (3mf-2-dup-cut-empty-second,
    // 3mf-2-dup-cut-longer-first, 3mf-2-dup-cut-longer-second).
    #[test]
    fn claims_of_another_entry() {
        let long = b"<objects><object id=\"1\"/><object id=\"2\"/></objects>";
        let short = b"<objects><object id=\"1\"/></objects>";
        let mut a = open(
            "dup",
            &pack(&[
                Entry::stored(b"d", long),
                Entry::stored(b"d", short),
                Entry::stored(b"e", b""),
            ]),
        );
        let long_claim = stat(&mut a, 0).m_uncomp_size;
        let short_claim = stat(&mut a, 1).m_uncomp_size;

        // The empty entry: the string is the claim's NULs, nothing written.
        assert!(matches!(plan(&mut a, 2, long_claim), Plan::Untouched));
        let got = extract_claimed(&mut a, Some(2), long_claim).unwrap();
        assert_eq!((got.bytes.len(), got.nuls), (0, long_claim));
        let got = extract_claimed(&mut a, Some(2), 0).unwrap();
        assert!(got.is_empty());

        // The longer entry's claim, the shorter entry's bytes: the bytes, then the NULs counted.
        let got = extract_claimed(&mut a, Some(1), long_claim).unwrap();
        assert_eq!(got.bytes.as_slice(), &short[..]);
        assert_eq!(got.nuls, long_claim - short_claim);
        assert_eq!(got.len(), long_claim);

        // The shorter entry's claim, the longer entry's bytes: MZ_ZIP_BUF_TOO_SMALL.
        assert!(extract_claimed(&mut a, Some(0), short_claim).is_none());

        // The name finds one of the two, as miniz's sorted directory does.
        assert!(matches!(locate(&mut a, b"d"), Some(0 | 1)));
        assert_eq!(locate(&mut a, b"x"), None);
        close("dup", a);
    }
}
