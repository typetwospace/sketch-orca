// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: _BBS_3MF_Base and _BBS_3MF_Importer of src/libslic3r/Format/bbs_3mf.cpp
// (lines 724-932, 1073-1442, 1656-1745, 1825-1871, 2381-2557, 3350-3438, 3517-4036, 4136-4249,
// 4808-4866 and 5279-5298) and load_bbs_3mf (8952-8966), with LoadStrategy's operator& of its
// header bbs_3mf.hpp, of OrcaSlicer v2.4.2, translated on 2026-11-10; the importer's members for
// the config entries and the loop's place after the root model (1728, 1860-1871 and 1996), on
// 2026-11-11; the lookup of the sub-model relationships (1746-1756), the importer's members for
// the sub-models and the shared meshes, and the call to the assembly (1996-1997), on 2026-11-11.
// Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer
// authors (Prusa Research), under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).

//! The 3MF reader, `load_bbs_3mf`, as `Model::read_from_file` calls it for the command line:
//! `LoadModel | AddDefaultInstances`, no configuration, plates, presets or progress.
//!
//! Its port is whole, in three pieces. Piece 1, here: the container (the archive, opened with
//! `orca-zip`), its relationships (`_rels/.rels`, parsed with `orca-xml`) and the root model
//! (`3D/3dmodel.model` or what the relationships name): its unit, objects with their meshes and
//! components, build items and their transforms, and metadata, from which the model's information
//! and the generator's version (`m_is_bbl_3mf`) come. Piece 2: the loop over the archive's other
//! entries ([`entries`]): `Metadata/model_settings.config` ([`config`]: the objects' and parts'
//! metadata, mesh statistics and text and shape configurations, the plates and their instances,
//! the assembly items), the cut information, the layer heights profiles and height ranges, the
//! brim ears and the svg files of embossed shapes. Piece 3: the sub-model files the root model's
//! own relationships name (`3D/_rels/3dmodel.model.rels`, looked up after the relationships and
//! read before the root model: [`objects`]), and the assembly into the model after the loop and
//! the archive's closing ([`assemble`]: the volumes made from the meshes and what the loop read,
//! the plates and the instances). Nothing of a piece is left to a later one.
//!
//! The branches of the loop that only a load with `LoadConfig`, `LoadAuxiliary` or `Restore`
//! takes stop with a [`ReadError::NotPorted`] naming [`LOAD_CONFIG_STAGE`] (the command line's
//! load takes none of them). `check_painting_version` (3929-3933) is not translated: only code
//! the C++ has commented out calls it.
//!
//! The importer's structs (`CurrentConfig`, `CurrentInstance`, `Metadata`, `ObjectMetadata` with
//! `VolumeMetadata`, and `CutObjectInfo`, 828-914, with their maps' types) are in [`config`] and
//! [`entries`]; `ObjectImporter` (933-1072) is in [`objects`]. At the plates' index check (line
//! 2276) the assembly refuses what the C++ refuses and lets an index of 0 pass, but writes nothing
//! for it where the C++ writes `plate_data_list[-1]`: the plate list is not kept (`config`'s
//! `handle_end_config_plater` and `parity/HAZARDS.md`, the 3MF table, say why).
//!
//! A refusal is what the C++ does: each failure `add_error`s its text, `load_bbs_3mf` logs them
//! at error after the load (`log_errors`), and the load answers `false`. The texts are the C++'s
//! byte for byte, `snprintf`'s 1024-byte buffers included (a longer text is cut at 1023 bytes;
//! cut inside a UTF-8 character, the rest of that character is lost here as `U+FFFD`, where the
//! C++ keeps the bytes). An entry name that is not UTF-8 (one found through `encode_path`, in the
//! local code page) is written with `U+FFFD` the same way, in the parse error's text and in the
//! loop's logs; so are an attribute's or a height range's bytes that are not UTF-8 where they are
//! passed on as text. The info, warning and trace logs are written with their texts, the
//! function and line the C++ writes with `__FUNCTION__` and `__LINE__` included.
//!
//! An exception that escapes `load_model_from_file` in the C++ (`std::stoi`'s in the brim ears
//! and the plates' metadata, Boost.PropertyTree's in the cut information and height ranges,
//! `set_deserialize`'s) skips `log_errors` and reaches `read_from_file`'s caller: here it is
//! [`ReadError::Message`] with its `what()`, and nothing is logged.
//!
//! The model's parse runs in `orca-zip`'s extraction callback, chunk by chunk as miniz hands the
//! inflated bytes to expat. A parse error there throws `FileIOError` through miniz, which is C:
//! the build is `/EHsc`, so the `catch` blocks of `_extract_model_from_archive` around the call
//! are left out, and the exception reaches `_extract_from_archive`'s handler, which adds
//! `e.what()` (`parity/HAZARDS.md`, the 3MF table). Here the callback's `Err` carries the text to
//! that handler the same way.
//!
//! What the command line's load does not take, named and not translated: the restore branches
//! (`LoadStrategy::Restore`, `m_load_restore`: the backup folder taken from the file's name, the
//! object ids taken from the UUIDs at an object's end, the archives at the backup path and
//! `m_origin_file` tried when an entry is missing, `set_backup_path("detach")`), `origin.txt`
//! saved for `LoadAuxiliary`, the version checks (`CheckVersion`, `version_error`), the progress
//! callback (`proFn`), the project's model id and country (`project` is null), and `is_orca_3mf`
//! (null). The backup id an object's UUID gives at a build item (lines 4177-4183, read as
//! hexadecimal from a BBL file's object UUID ending in `OBJECT_UUID_SUFFIX`) goes only to
//! `Model::set_object_backup_id`, which nothing the command line runs reads again: it is not kept
//! (`parity/HAZARDS.md`, the 3MF table), as object ids are not.
//!
//! The C++ keeps pointers to the objects and instances it makes (`CurrentObject::object`,
//! `Instance::instance`); here they are indexes into the model's objects and an object's
//! instances, moved down when `_handle_end_model` deletes an object before them.

mod assemble;
pub mod attrs;
mod config;
mod entries;
pub mod fast_float;
pub mod lexical_cast;
pub mod names;
mod objects;
pub mod spirit;
pub mod text;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use orca_eigen::{Transform3d, Vec3f, Vec3i32};
use orca_mesh::log::{Severity, log};
use orca_xml::{Ctl, Handler, Parser, Status, error_string};
use orca_zip::extension::{close_zip_reader, open_zip_reader};
use orca_zip::reader::{FileStat, ZipArchive};

use crate::emboss::ClaimedString;
use crate::model::{
    BrimPoint, LayerConfigRanges, Model, ModelDesignInfo, ModelInfo, ModelProfileInfo,
};
use crate::read::{ReadError, load_strategy};
use crate::semver::Semver;
use crate::transformation::Transformation;
use crate::utils::{encode_path, path_stem, xml_unescape};
use attrs::{
    get_attribute_value_bool, get_attribute_value_float, get_attribute_value_int,
    get_attribute_value_string, get_transform_from_3mf_specs_string, get_unit_factor,
    is_valid_object_type, zip_unicode_path_extra_field_decode,
};
use names::{
    AUTO_DROP_ATTR, BBL_APPLICATION_TAG, BBL_COPYRIGHT_NORMATIVE_TAG, BBL_COPYRIGHT_TAG,
    BBL_CREATION_DATE_TAG, BBL_DESCRIPTION_TAG, BBL_DESIGNER_COVER_FILE_TAG, BBL_DESIGNER_TAG,
    BBL_DESIGNER_USER_ID_TAG, BBL_LICENSE_TAG, BBL_MODEL_ID_TAG, BBL_MODEL_NAME_TAG,
    BBL_MODIFICATION_TAG, BBL_ORIGIN_TAG, BBL_PROFILE_COVER_TAG, BBL_PROFILE_DESCRIPTION_TAG,
    BBL_PROFILE_TITLE_TAG, BBL_PROFILE_USER_ID_TAG, BBL_PROFILE_USER_NAME_TAG, BBL_REGION_TAG,
    BBS_3MF_VERSION, BBS_3MF_VERSION1, BUILD_TAG, COLOR_ATTR, COLOR_GROUP_TAG, COLOR_TAG,
    COMPONENT_TAG, COMPONENTS_TAG, CUSTOM_FUZZY_SKIN_ATTR, CUSTOM_SEAM_ATTR, CUSTOM_SUPPORTS_ATTR,
    FACE_PROPERTY_ATTR, ID_ATTR, ITEM_TAG, MESH_TAG, METADATA_TAG, MMU_SEGMENTATION_ATTR,
    MODEL_TAG, NAME_ATTR, OBJECT_TAG, OBJECTID_ATTR, ORCASLICER_TAG, PID_ATTR, PPATH_ATTR,
    PRINTABLE_ATTR, PUUID_ATTR, PUUID_LOWER_ATTR, RELATIONSHIP_TAG, RELATIONSHIPS_FILE,
    RELS_TYPE_ATTR, RESOURCES_TAG, TARGET_ATTR, TRANSFORM_ATTR, TRIANGLE_TAG, TRIANGLES_TAG,
    TYPE_ATTR, UNIT_ATTR, V1_ATTR, V2_ATTR, V3_ATTR, VERTEX_TAG, VERTICES_TAG, X_ATTR, Y_ATTR,
    Z_ATTR,
};

/// [`ReadError::NotPorted`]'s stage for an entry only a load with `LoadConfig`, `LoadAuxiliary`
/// or `Restore` reads (the project's settings, presets, slicing information, G-code, thumbnails
/// and auxiliary files), which the command line's load never asks for.
pub const LOAD_CONFIG_STAGE: &str = "3MF reader's stage for the project's configuration and \
    auxiliary entries (LoadConfig, LoadAuxiliary and Restore, which the command line's load does \
    not take)";

/// The function `__FUNCTION__` names in `_load_model_from_file`'s logs.
const LOAD_MODEL_FROM_FILE: &str = "Slic3r::_BBS_3MF_Importer::_load_model_from_file";

/// `LoadStrategy`'s `operator&`: every bit of `flag` set.
fn has(strategy: u32, flag: u32) -> bool {
    strategy & flag == flag
}

/// `::snprintf(buf, 1024, ...)`: the text cut to the buffer's 1023 bytes.
fn snprintf_1024(s: String) -> String {
    if s.len() <= 1023 {
        return s;
    }
    String::from_utf8_lossy(&s.as_bytes()[..1023]).into_owned()
}

/// A C string printed with `%s`: `(null)` for MSVC's `NULL`.
fn c_str_or_null(s: Option<&str>) -> &str {
    s.unwrap_or("(null)")
}

/// `Id`: a sub-model's path (empty for the root model) and an object's id in it.
type Id = (String, i32);

/// `Component`.
#[derive(Clone, Debug)]
struct Component {
    object_id: Id,
    transform: Transform3d,
}

/// `Geometry`: an object's mesh as its model file lists it, with each triangle's painting.
#[derive(Clone, Debug, Default)]
struct Geometry {
    vertices: Vec<Vec3f>,
    triangles: Vec<Vec3i32>,
    custom_supports: Vec<String>,
    custom_seam: Vec<String>,
    mmu_segmentation: Vec<String>,
    fuzzy_skin: Vec<String>,
    // BBS
    face_properties: Vec<String>,
}

impl Geometry {
    /// `empty()`: no vertex or no triangle.
    fn empty(&self) -> bool {
        self.vertices.is_empty() || self.triangles.is_empty()
    }

    /// `reset()`: `face_properties` is not cleared, as in the C++.
    fn reset(&mut self) {
        self.vertices.clear();
        self.triangles.clear();
        self.custom_supports.clear();
        self.custom_seam.clear();
        self.mmu_segmentation.clear();
        self.fuzzy_skin.clear();
    }
}

/// `CurrentObject`.
#[derive(Clone, Debug)]
struct CurrentObject {
    // ID of the object inside the 3MF file, 1 based.
    id: i32,
    // Index of the ModelObject in its respective Model, zero based.
    model_object_idx: i32,
    geometry: Geometry,
    /// `object`: the index of the model's object made for it.
    object: Option<usize>,
    components: Vec<Component>,
    name: String,
    uuid: String,
    pid: i32,
}

impl CurrentObject {
    /// `CurrentObject()`.
    fn new() -> CurrentObject {
        let mut o = CurrentObject {
            id: -1,
            model_object_idx: -1,
            geometry: Geometry::default(),
            object: None,
            components: Vec::new(),
            name: String::new(),
            uuid: String::new(),
            pid: -1,
        };
        o.reset();
        o
    }

    /// `reset()`: `pid` is left as it is, as in the C++.
    fn reset(&mut self) {
        self.id = -1;
        self.model_object_idx = -1;
        self.geometry.reset();
        self.object = None;
        self.components.clear();
        self.uuid.clear();
        self.name.clear();
    }
}

/// `Instance`: an instance (its object's index and its own) and the build item's transform.
#[derive(Clone, Debug)]
struct Instance {
    object: usize,
    instance: usize,
    transform: Transform3d,
}

/// `_BBS_3MF_Importer` (with `_BBS_3MF_Base`'s errors).
#[derive(Default)]
struct Importer {
    m_errors: Vec<String>,
    // Version of the 3mf file
    m_version: u32,
    m_check_version: bool,
    m_load_model: bool,
    m_load_aux: bool,
    m_load_config: bool,
    // backup & restore
    m_load_restore: bool,
    m_backup_path: String,
    // Semantic version of Orca Slicer, that generated this 3MF.
    m_bambuslicer_generator_version: Option<Semver>,
    // Semantic version from the OrcaSlicer metadata tag (if present).
    m_orca_slicer_version: Option<Semver>,
    m_fdm_supports_painting_version: u32,
    m_seam_painting_version: u32,
    m_mm_painting_version: u32,
    m_model_id: String,
    m_contry_code: String,
    m_designer: String,
    m_designer_user_id: String,
    // m_designer_cover: declared in the C++ and never written or read (the cover file goes to
    // model_info.cover_file).
    model_info: ModelInfo,
    m_profile_title: String,
    m_profile_cover: String,
    m_profile_description: String,
    m_profile_user_id: String,
    m_profile_user_name: String,
    // Error code returned by the application side of the parser. In that case the expat may not
    // reliably deliver the error state after returning from XML_Parse() function, thus we keep
    // the error state here.
    m_parse_error: bool,
    m_parse_error_message: String,
    m_unit_factor: f32,
    m_curr_object: Option<CurrentObject>,
    m_current_objects: BTreeMap<Id, CurrentObject>,
    m_index_paths: BTreeMap<i32, String>,
    m_objects: BTreeMap<Id, i32>,
    m_instances: Vec<Instance>,
    m_curr_config: config::CurrentConfig,
    m_objects_metadata: BTreeMap<i32, config::ObjectMetadata>,
    m_cut_object_infos: BTreeMap<i32, entries::CutObjectInfo>,
    m_layer_heights_profiles: BTreeMap<i32, Vec<f64>>,
    m_layer_config_ranges: BTreeMap<i32, LayerConfigRanges>,
    m_brim_ear_points: BTreeMap<i32, Vec<BrimPoint>>,
    /// `m_path_to_emboss_shape_files`: the svg files by their entry's name (as bytes).
    m_path_to_emboss_shape_files: BTreeMap<Vec<u8>, Arc<ClaimedString>>,
    m_curr_metadata_name: String,
    /// `m_curr_characters`: the bytes expat hands over (UTF-8 when whole).
    m_curr_characters: Vec<u8>,
    m_name: String,
    m_sub_model_path: String,
    m_start_part_path: String,
    m_thumbnail_path: String,
    m_thumbnail_middle: String,
    m_thumbnail_small: String,
    m_sub_model_paths: Vec<String>,
    /// `m_shared_meshes`: the part each mesh id was first made into, as its object's index and
    /// its own (the C++'s `ModelVolume*`).
    m_shared_meshes: BTreeMap<i32, (usize, usize)>,
    //BBS: plater related structures
    m_is_bbl_3mf: bool,
    m_parsing_slice_info: bool,
    /// The plates `model_settings.config` makes, each where the C++ allocates a `PlateData`;
    /// `m_plater_data` and `m_curr_plater` hold indexes into it for the C++'s pointers.
    m_plates: Vec<config::PlateData>,
    m_plater_data: BTreeMap<i32, usize>,
    m_curr_plater: Option<usize>,
    m_curr_instance: config::CurrentInstance,
    m_current_color_group: i32,
    m_group_id_to_color: BTreeMap<i32, String>,
    /// The exception a handler of `model_settings.config` threw (its `what()`), kept while the
    /// stopped parse returns, for `_extract_xml_from_archive`'s caller to let escape.
    m_thrown: Option<String>,
}

/// The handlers `_extract_xml_from_archive` is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XmlHandlers {
    /// `_handle_start_relationships_element` and `_handle_end_relationships_element`.
    Relationships,
    /// `_handle_start_config_xml_element` and `_handle_end_config_xml_element`.
    Config,
}

/// The handlers of the root model's parse (`_handle_start_model_xml_element` and the others,
/// with the importer as `userData`).
struct ModelXml<'a> {
    importer: &'a mut Importer,
    model: &'a mut Model,
}

impl Handler for ModelXml<'_> {
    fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
        self.importer
            .handle_start_model_xml_element(ctl, self.model, name, atts);
    }
    fn end_element(&mut self, ctl: &mut Ctl, name: &[u8]) {
        self.importer
            .handle_end_model_xml_element(ctl, self.model, name);
    }
    fn character_data(&mut self, _: &mut Ctl, s: &[u8]) {
        self.importer.handle_xml_characters(s);
    }
}

/// The handlers of a relationships file's parse.
struct RelationshipsXml<'a> {
    importer: &'a mut Importer,
}

impl Handler for RelationshipsXml<'_> {
    fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
        self.importer
            .handle_start_relationships_element(ctl, name, atts);
    }
    fn end_element(&mut self, ctl: &mut Ctl, name: &[u8]) {
        self.importer.handle_end_relationships_element(ctl, name);
    }
    fn character_data(&mut self, _: &mut Ctl, s: &[u8]) {
        self.importer.handle_xml_characters(s);
    }
}

impl Importer {
    /// `_BBS_3MF_Importer()`.
    fn new() -> Importer {
        Importer {
            m_unit_factor: 1.0,
            m_current_color_group: -1,
            ..Importer::default()
        }
    }

    /// `add_error(error)`.
    fn add_error(&mut self, error: &str) {
        self.m_errors.push(error.to_owned());
    }

    /// `clear_errors()`.
    fn clear_errors(&mut self) {
        self.m_errors.clear();
    }

    /// `log_errors()`: each error logged at error.
    fn log_errors(&self) {
        for error in &self.m_errors {
            log(Severity::Error, error);
        }
    }

    /// `load_model_from_file(filename, model, ..., strategy, is_bbl_3mf, is_orca_3mf,
    /// file_version, ...)` with `is_orca_3mf` and `project` null.
    fn load_model_from_file(
        &mut self,
        filename: &str,
        model: &mut Model,
        strategy: u32,
        is_bbl_3mf: &mut bool,
        file_version: &mut Semver,
    ) -> Result<bool, ReadError> {
        self.m_version = 0;
        self.m_fdm_supports_painting_version = 0;
        self.m_seam_painting_version = 0;
        self.m_mm_painting_version = 0;
        self.m_check_version = has(strategy, load_strategy::CHECK_VERSION);
        //BBS: auxiliary data
        self.m_load_model = has(strategy, load_strategy::LOAD_MODEL);
        self.m_load_aux = has(strategy, load_strategy::LOAD_AUXILIARY);
        self.m_load_restore = has(strategy, load_strategy::RESTORE);
        self.m_load_config = has(strategy, load_strategy::LOAD_CONFIG);
        self.m_unit_factor = 1.0;
        self.m_curr_object = None;
        self.m_current_objects.clear();
        self.m_index_paths.clear();
        self.m_objects.clear();
        self.m_instances.clear();
        self.m_curr_config = config::CurrentConfig::default();
        self.m_objects_metadata.clear();
        self.m_layer_heights_profiles.clear();
        self.m_layer_config_ranges.clear();
        self.m_brim_ear_points.clear();
        self.m_curr_metadata_name.clear();
        self.m_curr_characters.clear();
        //BBS: plater data init
        self.m_plater_data.clear();
        self.m_plates.clear();
        self.m_curr_instance = config::CurrentInstance::default();
        self.clear_errors();

        // restore: under Restore the backup folder is the file's name less ".3mf" (not
        // translated: the command line's load is not a restore).
        self.m_backup_path = model.get_backup_path();
        let result = self.load_model_from_file_inner(filename, model);
        *is_bbl_3mf = self.m_is_bbl_3mf;
        // If the OrcaSlicer tag is present, use it as file_version (ignoring the Bambu
        // Application version). Otherwise fall back to the version parsed from the Application
        // tag.
        if let Some(v) = &self.m_orca_slicer_version {
            *file_version = v.clone();
        } else if let Some(v) = &self.m_bambuslicer_generator_version {
            *file_version = v.clone();
        }
        result
    }

    /// `_load_model_from_file(filename, model, ...)`, with the archive closed at the end as its
    /// `close_lock` closes it.
    fn load_model_from_file_inner(
        &mut self,
        filename: &str,
        model: &mut Model,
    ) -> Result<bool, ReadError> {
        //BBS progress point
        // prepare restore
        if self.m_load_restore {
            log(
                Severity::Info,
                &format!("{LOAD_MODEL_FROM_FILE}:1690import 3mf IMPORT_STAGE_RESTORE\n"),
            );
        }
        //BBS progress point
        log(
            Severity::Info,
            &format!(
                "{LOAD_MODEL_FROM_FILE}:1699import 3mf IMPORT_STAGE_OPEN, m_load_restore={}\n",
                i32::from(self.m_load_restore)
            ),
        );
        let mut archive = ZipArchive::default();
        let result = self.load_from_archive(filename, model, &mut archive);
        close_zip_reader(&mut archive);
        result
    }

    /// The body of `_load_model_from_file` from the archive's opening on.
    fn load_from_archive(
        &mut self,
        filename: &str,
        model: &mut Model,
        archive: &mut ZipArchive,
    ) -> Result<bool, ReadError> {
        if !open_zip_reader(archive, Path::new(filename)) {
            self.add_error(&format!("Unable to open the file{filename}"));
            return Ok(false);
        }

        let num_entries = archive.reader_get_num_files();

        self.m_name = path_stem(filename);

        //BBS progress point
        log(
            Severity::Info,
            &format!("{LOAD_MODEL_FROM_FILE}:1735import 3mf IMPORT_STAGE_READ_FILES\n"),
        );
        // BBS: load relationships
        if !self.extract_xml_from_archive(model, archive, RELATIONSHIPS_FILE) {
            return Ok(false);
        }
        if self.m_start_part_path.is_empty() {
            return Ok(false);
        }
        // BBS: load sub models (Production Extension)
        let mut sub_rels = self.m_start_part_path.clone();
        let at = sub_rels.rfind('/').map_or(sub_rels.len(), |i| i + 1);
        sub_rels.insert_str(at, "_rels/");
        sub_rels.push_str(".rels");
        if sub_rels.starts_with('/') {
            sub_rels = sub_rels[1..].to_owned();
        }
        //check whether sub relation file is exist or not
        let sub_index = archive.reader_locate_file(sub_rels.as_bytes(), None, 0);
        // At -1 no submodule files are found: only the one 3dmodel.model is used.
        if sub_index != -1 {
            // Its result is not looked at: its errors stay for log_errors.
            self.extract_xml_from_archive(model, archive, &sub_rels);
            if !self.load_sub_models(filename) {
                return Ok(false);
            }
            // BBS: load root model (the progress callback is null)
        }

        //extract model files
        let start_part_path = self.m_start_part_path.clone();
        if !self.extract_from_archive(
            model,
            archive,
            &start_part_path,
            |me, model, archive, stat| me.extract_model_from_archive(model, archive, stat),
        ) {
            self.add_error("Archive does not contain a valid model");
            return Ok(false);
        }

        if !self.m_designer.is_empty() {
            model.design_info = Some(ModelDesignInfo {
                designer_user_id: self.m_designer_user_id.clone(),
                designer: self.m_designer.clone(),
                ..ModelDesignInfo::default()
            });
        }

        model.profile_info = Some(ModelProfileInfo {
            profile_tile: self.m_profile_title.clone(),
            profile_cover: self.m_profile_cover.clone(),
            profile_description: self.m_profile_description.clone(),
            profile_user_id: self.m_profile_user_id.clone(),
            profile_user_name: self.m_profile_user_name.clone(),
        });

        let mut info = ModelInfo::default();
        info.load(&self.model_info);
        if !self.m_thumbnail_small.is_empty() {
            info.metadata_items
                .entry("Thumbnail_Small".to_owned())
                .or_insert_with(|| self.m_thumbnail_small.clone());
        }
        if !self.m_thumbnail_middle.is_empty() {
            info.metadata_items
                .entry("Thumbnail_Middle".to_owned())
                .or_insert_with(|| self.m_thumbnail_middle.clone());
        }
        model.model_info = Some(info);

        // The project's model id and country go to `project`, which is null here. Orca skips the
        // version check (the lines that set dont_load_config otherwise are commented out).
        if !self.extract_other_entries(model, archive, num_entries)? {
            return Ok(false);
        }

        // lock.close(): the archive is closed here; the caller's close after it finds it closed.
        close_zip_reader(archive);

        self.assemble(model, filename)
    }

    /// `_extract_from_archive(archive, path, extract)`: the entry found by its name as UTF-8, in
    /// the local code page, or by the Unicode path extra field, then `extract` called on it; an
    /// exception it throws (`Err`) is added as an error.
    fn extract_from_archive<F>(
        &mut self,
        model: &mut Model,
        archive: &mut ZipArchive,
        path: &str,
        extract: F,
    ) -> bool
    where
        F: FnOnce(&mut Importer, &mut Model, &mut ZipArchive, &FileStat) -> Result<bool, String>,
    {
        let Some(stat) = locate_entry(archive, path) else {
            self.add_error(&snprintf_1024(format!(
                "File {path} not found from archive"
            )));
            return false;
        };
        match extract(self, model, archive, &stat) {
            Ok(true) => true,
            Ok(false) => false,
            Err(what) => {
                // ensure the zip archive is closed and rethrow the exception
                self.add_error(&what);
                false
            }
        }
    }

    /// `_extract_xml_from_archive(archive, path, _handle_start_relationships_element,
    /// _handle_end_relationships_element)`: the relationships file, found by its name.
    fn extract_xml_from_archive(
        &mut self,
        model: &mut Model,
        archive: &mut ZipArchive,
        path: &str,
    ) -> bool {
        self.extract_from_archive(model, archive, path, |me, model, archive, stat| {
            me.extract_xml_from_archive_stat(model, archive, stat, XmlHandlers::Relationships)
        })
    }

    /// `_extract_xml_from_archive(archive, stat, start_handler, end_handler)`: the entry read
    /// whole into expat's buffer and parsed at once. `Err` is the exception a handler threw.
    ///
    /// `XML_GetBuffer` takes the claimed size as an `int` and refuses one not above 0 or above
    /// 2^30 (the buffer's size doubles from 1024 and overflows `int` past it); that is tested
    /// before anything is allocated, and the buffer is made at the size the parse reads once the
    /// extraction has given the bytes ([`entries::extract_claimed`]). Past the bytes extraction
    /// writes, the C++'s buffer is uninitialized, which only a claim larger than its entry's (two
    /// entries of one name) or an entry with no compressed data leaves: one NUL stands for those
    /// bytes here (`parity/HAZARDS.md`, the 3MF table).
    fn extract_xml_from_archive_stat(
        &mut self,
        model: &mut Model,
        archive: &mut ZipArchive,
        stat: &FileStat,
        handlers: XmlHandlers,
    ) -> Result<bool, String> {
        if stat.m_uncomp_size == 0 {
            self.add_error("Found invalid size");
            return Ok(false);
        }
        let mut parser = Parser::new();
        #[allow(clippy::cast_possible_truncation)]
        let len = stat.m_uncomp_size as i32;
        if len <= 0 || len > 1 << 30 {
            self.add_error("Unable to create buffer");
            return Ok(false);
        }
        let index = entries::locate(archive, &stat.m_filename);
        let Some(data) = entries::extract_claimed(archive, index, stat.m_uncomp_size) else {
            self.add_error("Error while reading config data to buffer");
            return Ok(false);
        };
        #[allow(clippy::cast_sign_loss)]
        let claimed = len as usize;
        let written = data.bytes.len().min(claimed);
        let parse_len = if written < claimed {
            written + 1
        } else {
            claimed
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let parse_len = parse_len as i32;
        let Some(parser_buffer) = parser.get_buffer(parse_len) else {
            self.add_error("Unable to create buffer");
            return Ok(false);
        };
        parser_buffer[..written].copy_from_slice(&data.bytes[..written]);
        parser_buffer[written..].fill(0);
        let status = match handlers {
            XmlHandlers::Relationships => {
                let mut h = RelationshipsXml { importer: self };
                parser.parse_buffer(&mut h, parse_len, true)
            }
            XmlHandlers::Config => {
                let mut h = config::ConfigXml {
                    importer: self,
                    model,
                };
                parser.parse_buffer(&mut h, parse_len, true)
            }
        };
        if let Some(what) = self.m_thrown.take() {
            return Err(what);
        }
        if status == Status::Error {
            let text = error_string(parser.error_code().code());
            let line = parser.current_line_number() as i32;
            self.add_error(&snprintf_1024(format!(
                "Error ({}) while parsing xml file at line {line}",
                c_str_or_null(text)
            )));
            return Ok(false);
        }
        Ok(true)
    }

    /// `_extract_model_from_archive(archive, stat)`: the root model parsed chunk by chunk in the
    /// extraction callback. A parse error is the callback's `FileIOError`, which no handler here
    /// catches (`/EHsc`): it comes back as `Err` for `_extract_from_archive`.
    fn extract_model_from_archive(
        &mut self,
        model: &mut Model,
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) -> Result<bool, String> {
        if stat.m_uncomp_size == 0 {
            self.add_error("Found invalid size");
            return Ok(false);
        }
        let mut parser = Parser::new();
        let filename = String::from_utf8_lossy(&stat.m_filename).into_owned();
        let uncomp_size = stat.m_uncomp_size;
        let res = {
            let mut callback = |file_ofs: u64, buf: &[u8]| -> Result<usize, String> {
                let n = buf.len();
                let status = {
                    let mut h = ModelXml {
                        importer: &mut *self,
                        model: &mut *model,
                    };
                    parser.parse(&mut h, buf, file_ofs + n as u64 == uncomp_size)
                };
                if status == Status::Error || self.m_parse_error {
                    let message = self.parse_error_message(&parser);
                    let line = parser.current_line_number() as i32;
                    return Err(snprintf_1024(format!(
                        "Error ({message}) while parsing '{filename}' at line {line}"
                    )));
                }
                Ok(n)
            };
            archive.reader_extract_to_callback(stat.m_file_index, &mut callback, 0)?
        };
        if !res {
            self.add_error("Error while extracting model data from zip archive");
            return Ok(false);
        }
        Ok(true)
    }

    /// `parse_error_message()`.
    fn parse_error_message(&self, parser: &Parser) -> String {
        if self.m_parse_error {
            // The error was signalled by the user code, not the expat parser.
            if self.m_parse_error_message.is_empty() {
                "Invalid 3MF format".to_owned()
            } else {
                self.m_parse_error_message.clone()
            }
        } else {
            // The error was signalled by the expat parser.
            c_str_or_null(error_string(parser.error_code().code())).to_owned()
        }
    }

    /// `_stop_xml_parser(msg)`.
    fn stop_xml_parser(&mut self, ctl: &mut Ctl, msg: &str) {
        self.m_parse_error = true;
        self.m_parse_error_message = msg.to_owned();
        ctl.stop_parser(false);
    }

    /// `_handle_start_model_xml_element(name, attributes)`.
    fn handle_start_model_xml_element(
        &mut self,
        ctl: &mut Ctl,
        model: &mut Model,
        name: &[u8],
        attributes: &[&[u8]],
    ) {
        let num_attributes = ctl.specified_attribute_count() as u32;
        let a = (attributes, num_attributes);
        let res = if name == MODEL_TAG.as_bytes() {
            self.handle_start_model(a)
        } else if name == RESOURCES_TAG.as_bytes() {
            // do nothing
            true
        } else if name == OBJECT_TAG.as_bytes() {
            self.handle_start_object(a)
        } else if name == COLOR_GROUP_TAG.as_bytes() {
            self.handle_start_color_group(a)
        } else if name == COLOR_TAG.as_bytes() {
            self.handle_start_color(a)
        } else if name == MESH_TAG.as_bytes() {
            self.handle_start_mesh()
        } else if name == VERTICES_TAG.as_bytes() {
            self.handle_start_vertices()
        } else if name == VERTEX_TAG.as_bytes() {
            self.handle_start_vertex(a)
        } else if name == TRIANGLES_TAG.as_bytes() {
            self.handle_start_triangles()
        } else if name == TRIANGLE_TAG.as_bytes() {
            self.handle_start_triangle(a)
        } else if name == COMPONENTS_TAG.as_bytes() {
            self.handle_start_components()
        } else if name == COMPONENT_TAG.as_bytes() {
            self.handle_start_component(a)
        } else if name == BUILD_TAG.as_bytes() {
            // do nothing
            true
        } else if name == ITEM_TAG.as_bytes() {
            self.handle_start_item(model, a)
        } else if name == METADATA_TAG.as_bytes() {
            self.handle_start_metadata(a)
        } else {
            true
        };
        if !res {
            self.stop_xml_parser(ctl, "");
        }
    }

    /// `_handle_end_model_xml_element(name)`: only the model's and the object's ends and the
    /// metadata's do something; the rest answer true.
    fn handle_end_model_xml_element(&mut self, ctl: &mut Ctl, model: &mut Model, name: &[u8]) {
        let res = if name == MODEL_TAG.as_bytes() {
            self.handle_end_model(model)
        } else if name == OBJECT_TAG.as_bytes() {
            self.handle_end_object()
        } else if name == METADATA_TAG.as_bytes() {
            self.handle_end_metadata()
        } else {
            // RESOURCES, COLOR_GROUP, COLOR, MESH, VERTICES, VERTEX, TRIANGLES, TRIANGLE,
            // COMPONENTS, COMPONENT, BUILD and ITEM: do nothing
            true
        };
        if !res {
            self.stop_xml_parser(ctl, "");
        }
    }

    /// `_handle_xml_characters(s, len)`.
    fn handle_xml_characters(&mut self, s: &[u8]) {
        self.m_curr_characters.extend_from_slice(s);
    }

    /// `_handle_start_model(attributes, num_attributes)`.
    fn handle_start_model(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        self.m_unit_factor = get_unit_factor(&get_attribute_value_string(atts, n, UNIT_ATTR));
        true
    }

    /// `_handle_end_model()`.
    fn handle_end_model(&mut self, model: &mut Model) -> bool {
        // BBS: Production Extension
        if !self.m_sub_model_path.is_empty() {
            return true;
        }

        // deletes all non-built or non-instanced objects
        let objects: Vec<i32> = self.m_objects.values().copied().collect();
        for object in objects {
            if object >= model.objects.len() as i32 {
                self.add_error("Unable to find object");
                return false;
            }
            let idx = object as usize;
            if model.objects[idx].instances.is_empty() {
                model.delete_object(idx);
                // The pointers the C++ keeps stay on their objects: move the indexes past it.
                for instance in &mut self.m_instances {
                    if instance.object > idx {
                        instance.object -= 1;
                    }
                }
                for o in self.m_current_objects.values_mut() {
                    if let Some(oi) = o.object.as_mut()
                        && *oi > idx
                    {
                        *oi -= 1;
                    }
                }
            }
        }

        //construct the index maps
        for key in self.m_current_objects.keys() {
            self.m_index_paths
                .entry(key.1)
                .or_insert_with(|| key.0.clone());
        }

        if !self.m_is_bbl_3mf {
            // if the 3mf was not produced by OrcaSlicer and there is only one object,
            // set the object name to match the filename
            if model.objects.len() == 1 {
                model.objects[0].name = self.m_name.clone();
            }
        }

        // applies instances' matrices
        for instance in &self.m_instances {
            // apply the transform to the instance
            let i = &mut model.objects[instance.object].instances[instance.instance];
            apply_transform(i, &instance.transform);
        }
        true
    }

    /// `_handle_start_object(attributes, num_attributes)`.
    fn handle_start_object(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        // reset current object data
        self.m_curr_object = None;
        let object_type = get_attribute_value_string(atts, n, TYPE_ATTR);
        if is_valid_object_type(&object_type) {
            let mut o = CurrentObject::new();
            o.id = get_attribute_value_int(atts, n, ID_ATTR);
            o.name = get_attribute_value_string(atts, n, NAME_ATTR);
            o.uuid = get_attribute_value_string(atts, n, PUUID_ATTR);
            if o.uuid.is_empty() {
                o.uuid = get_attribute_value_string(atts, n, PUUID_LOWER_ATTR);
            }
            o.pid = get_attribute_value_int(atts, n, PID_ATTR);
            self.m_curr_object = Some(o);
        }
        true
    }

    /// `_handle_end_object()`. A BBL object whose UUID ends in `OBJECT_UUID_SUFFIX` takes its
    /// backup id here only under `Restore` (not translated).
    fn handle_end_object(&mut self) -> bool {
        if !self.m_load_model {
            self.m_curr_object = None;
            return true;
        }
        let Some(curr) = self.m_curr_object.take().filter(|o| o.id != -1) else {
            self.add_error("Found invalid object");
            return false;
        };
        let id: Id = (self.m_sub_model_path.clone(), curr.id);
        if let std::collections::btree_map::Entry::Vacant(e) = self.m_current_objects.entry(id) {
            e.insert(curr);
        } else {
            self.add_error("Found object with duplicate id");
            return false;
        }
        true
    }

    /// `_handle_start_color_group(attributes, num_attributes)`.
    fn handle_start_color_group(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        self.m_current_color_group = get_attribute_value_int(atts, n, ID_ATTR);
        true
    }

    /// `_handle_start_color(attributes, num_attributes)`.
    fn handle_start_color(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        let color = get_attribute_value_string(atts, n, COLOR_ATTR);
        self.m_group_id_to_color
            .insert(self.m_current_color_group, color);
        true
    }

    /// `_handle_start_mesh(...)`: reset current geometry.
    fn handle_start_mesh(&mut self) -> bool {
        if let Some(o) = self.m_curr_object.as_mut() {
            o.geometry.reset();
        }
        true
    }

    /// `_handle_start_vertices(...)`: reset current vertices.
    fn handle_start_vertices(&mut self) -> bool {
        if let Some(o) = self.m_curr_object.as_mut() {
            o.geometry.vertices.clear();
        }
        true
    }

    /// `_handle_start_vertex(attributes, num_attributes)`: the vertex in millimetres (`float`),
    /// a missing coordinate 0.
    fn handle_start_vertex(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        let f = self.m_unit_factor;
        if let Some(o) = self.m_curr_object.as_mut() {
            o.geometry.vertices.push(Vec3f::new(
                f * get_attribute_value_float(atts, n, X_ATTR),
                f * get_attribute_value_float(atts, n, Y_ATTR),
                f * get_attribute_value_float(atts, n, Z_ATTR),
            ));
        }
        true
    }

    /// `_handle_start_triangles(...)`: reset current triangles.
    fn handle_start_triangles(&mut self) -> bool {
        if let Some(o) = self.m_curr_object.as_mut() {
            o.geometry.triangles.clear();
        }
        true
    }

    /// `_handle_start_triangle(attributes, num_attributes)`: the triangle's vertex indices (a
    /// missing one 0) and its painting strings.
    fn handle_start_triangle(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        // we are ignoring the following attributes:
        // p1
        // p2
        // p3
        // pid
        // see specifications
        if let Some(o) = self.m_curr_object.as_mut() {
            let g = &mut o.geometry;
            g.triangles.push(Vec3i32::new(
                get_attribute_value_int(atts, n, V1_ATTR),
                get_attribute_value_int(atts, n, V2_ATTR),
                get_attribute_value_int(atts, n, V3_ATTR),
            ));
            g.custom_supports
                .push(get_attribute_value_string(atts, n, CUSTOM_SUPPORTS_ATTR));
            g.custom_seam
                .push(get_attribute_value_string(atts, n, CUSTOM_SEAM_ATTR));
            g.mmu_segmentation
                .push(get_attribute_value_string(atts, n, MMU_SEGMENTATION_ATTR));
            g.fuzzy_skin
                .push(get_attribute_value_string(atts, n, CUSTOM_FUZZY_SKIN_ATTR));
            // BBS
            g.face_properties
                .push(get_attribute_value_string(atts, n, FACE_PROPERTY_ATTR));
        }
        true
    }

    /// `_handle_start_components(...)`: reset current components.
    fn handle_start_components(&mut self) -> bool {
        if let Some(o) = self.m_curr_object.as_mut() {
            o.components.clear();
        }
        true
    }

    /// `_handle_start_component(attributes, num_attributes)`: the component's object, by its
    /// unescaped `p:path` (or the sub-model's path) and id, and its transform.
    fn handle_start_component(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        let path = xml_unescape(&get_attribute_value_string(atts, n, PPATH_ATTR));
        let object_id = get_attribute_value_int(atts, n, OBJECTID_ATTR);
        let transform = get_transform_from_3mf_specs_string(&get_attribute_value_string(
            atts,
            n,
            TRANSFORM_ATTR,
        ));
        let sub_model_path = &self.m_sub_model_path;
        if let Some(o) = self.m_curr_object.as_mut() {
            let id: Id = (
                if sub_model_path.is_empty() {
                    path
                } else {
                    sub_model_path.clone()
                },
                object_id,
            );
            o.components.push(Component {
                object_id: id,
                transform,
            });
        }
        true
    }

    /// `_handle_start_item(attributes, num_attributes)`: a build item, an instance of its object.
    fn handle_start_item(&mut self, model: &mut Model, (atts, n): (&[&[u8]], u32)) -> bool {
        // we are ignoring the following attributes
        // thumbnail
        // partnumber
        // pid
        // pindex
        // see specifications
        let object_id = get_attribute_value_int(atts, n, OBJECTID_ATTR);
        let path = get_attribute_value_string(atts, n, PPATH_ATTR);
        let transform = get_transform_from_3mf_specs_string(&get_attribute_value_string(
            atts,
            n,
            TRANSFORM_ATTR,
        ));
        let printable = get_attribute_value_bool(atts, n, PRINTABLE_ATTR);
        let auto_drop = get_attribute_value_bool(atts, n, AUTO_DROP_ATTR);
        !self.m_load_model
            || self.create_object_instance(
                model, &path, object_id, &transform, printable, auto_drop, 1,
            )
    }

    /// `_handle_start_metadata(attributes, num_attributes)`: a nameless (or empty-named)
    /// metadata keeps the last name.
    fn handle_start_metadata(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        self.m_curr_characters.clear();
        let name = get_attribute_value_string(atts, n, NAME_ATTR);
        if !name.is_empty() {
            self.m_curr_metadata_name = name;
        }
        true
    }

    /// `_handle_end_metadata()`.
    fn handle_end_metadata(&mut self) -> bool {
        // expat's character data is UTF-8, whole once the element ends.
        let chars = String::from_utf8_lossy(&self.m_curr_characters).into_owned();
        let name = self.m_curr_metadata_name.as_str();
        let trace = |what: &str| {
            log(
                Severity::Trace,
                &format!("design_info, load_3mf found {what} = {chars}"),
            );
        };
        if name == BBS_3MF_VERSION || name == BBS_3MF_VERSION1 {
            //m_is_bbl_3mf = true;
            self.m_version = orca_fmt::parse::atoi(chars.as_bytes()) as u32;
        } else if name == BBL_APPLICATION_TAG {
            // Generator application of the 3MF.
            // SLIC3R_APP_KEY - SoftFever_VERSION
            if let Some(rest) = chars.strip_prefix("BambuStudio-") {
                self.m_is_bbl_3mf = true;
                self.m_bambuslicer_generator_version = Semver::parse(rest);
            } else if let Some(rest) = chars.strip_prefix("OrcaSlicer-") {
                self.m_is_bbl_3mf = true;
                self.m_bambuslicer_generator_version = Semver::parse(rest);
            }
        } else if name == ORCASLICER_TAG {
            // OrcaSlicer version tag (written from OrcaSlicer 2.3.2 onwards)
            self.m_orca_slicer_version = Semver::parse(&chars);
            if self.m_orca_slicer_version.is_some() {
                self.m_is_bbl_3mf = true;
            }
        } else if name == BBL_MODEL_ID_TAG {
            self.m_model_id = xml_unescape(&chars);
        } else if name == BBL_MODEL_NAME_TAG {
            trace("model name");
            self.model_info.model_name = xml_unescape(&chars);
        } else if name == BBL_ORIGIN_TAG {
            trace("model name");
            self.model_info.origin = xml_unescape(&chars);
        } else if name == BBL_DESIGNER_TAG {
            trace("designer");
            self.m_designer = xml_unescape(&chars);
        } else if name == BBL_DESIGNER_USER_ID_TAG {
            trace("designer_user_id");
            self.m_designer_user_id = xml_unescape(&chars);
        } else if name == BBL_DESIGNER_COVER_FILE_TAG {
            trace("designer_cover");
            self.model_info.cover_file = xml_unescape(&chars);
        } else if name == BBL_DESCRIPTION_TAG {
            trace("description");
            self.model_info.description = xml_unescape(&chars);
        } else if name == BBL_LICENSE_TAG {
            trace("license");
            self.model_info.license = xml_unescape(&chars);
        } else if name == BBL_COPYRIGHT_TAG {
            trace("CopyRight");
            self.model_info.copyright = xml_unescape(&chars);
        } else if name == BBL_COPYRIGHT_NORMATIVE_TAG {
            trace("Copyright");
            self.model_info.copyright = xml_unescape(&chars);
        } else if name == BBL_REGION_TAG {
            trace("region");
            self.m_contry_code = xml_unescape(&chars);
        } else if name == BBL_PROFILE_TITLE_TAG {
            trace("profile_title");
            self.m_profile_title = xml_unescape(&chars);
        } else if name == BBL_PROFILE_COVER_TAG {
            trace("profile_cover");
            self.m_profile_cover = xml_unescape(&chars);
        } else if name == BBL_PROFILE_DESCRIPTION_TAG {
            trace("profile_description");
            self.m_profile_description = xml_unescape(&chars);
        } else if name == BBL_PROFILE_USER_ID_TAG {
            trace("profile_user_id");
            self.m_profile_user_id = xml_unescape(&chars);
        } else if name == BBL_PROFILE_USER_NAME_TAG {
            trace("profile_user_name");
            self.m_profile_user_name = xml_unescape(&chars);
        } else if name == BBL_CREATION_DATE_TAG || name == BBL_MODIFICATION_TAG {
            // nothing
        }
        if !self.m_curr_metadata_name.is_empty() {
            let value = xml_unescape(&chars);
            log(
                Severity::Info,
                &format!(
                    "load_3mf found metadata key = {}, value = {value}",
                    self.m_curr_metadata_name
                ),
            );
            self.model_info
                .metadata_items
                .insert(self.m_curr_metadata_name.clone(), value);
        }
        true
    }

    /// `_create_object_instance(path, object_id, transform, printable, auto_drop,
    /// recur_counter)`: the first build item of an object makes its model object; each adds an
    /// instance.
    #[allow(clippy::too_many_arguments)]
    fn create_object_instance(
        &mut self,
        model: &mut Model,
        path: &str,
        object_id: i32,
        transform: &Transform3d,
        printable: bool,
        auto_drop: bool,
        recur_counter: u32,
    ) -> bool {
        const MAX_RECURSIONS: u32 = 10;

        // escape from circular aliasing
        if recur_counter > MAX_RECURSIONS {
            self.add_error("Too many recursions");
            return false;
        }

        let id: Id = (path.to_owned(), object_id);
        if !self.m_current_objects.contains_key(&id) {
            self.add_error(&format!("can not find object id {object_id} to builditem"));
            return false;
        }

        if let Some(&object_index) = self.m_objects.get(&id) {
            //add instance
            let oi = object_index as usize;
            let ii = model.objects[oi].add_instance();
            let instance = &mut model.objects[oi].instances[ii];
            instance.printable = printable;
            instance.auto_drop = auto_drop;
            self.m_instances.push(Instance {
                object: oi,
                instance: ii,
                transform: *transform,
            });
        } else {
            //add object
            let object_index = model.objects.len() as i32;
            let oi = model.add_empty_object();
            self.m_objects.insert(id.clone(), object_index);
            let current_object = self
                .m_current_objects
                .get_mut(&id)
                .expect("the object found above");
            current_object.model_object_idx = object_index;
            current_object.object = Some(oi);

            let ii = model.objects[oi].add_instance();
            let instance = &mut model.objects[oi].instances[ii];
            instance.printable = printable;
            instance.auto_drop = auto_drop;
            self.m_instances.push(Instance {
                object: oi,
                instance: ii,
                transform: *transform,
            });
            // A BBL file's object whose UUID ends in OBJECT_UUID_SUFFIX gives its hexadecimal
            // backup id to set_object_backup_id here (lines 4177-4183); nothing the command line
            // runs reads it again, and it is not kept.
        }
        true
    }

    /// `_handle_start_relationships_element(name, attributes)`.
    fn handle_start_relationships_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
        let num_attributes = ctl.specified_attribute_count() as u32;
        let mut res = true;
        if name == RELATIONSHIP_TAG.as_bytes() {
            res = self.handle_start_relationship(atts, num_attributes);
        }
        self.m_curr_characters.clear();
        if !res {
            self.stop_xml_parser(ctl, "");
        }
    }

    /// `_handle_end_relationships_element(name)`: nothing.
    fn handle_end_relationships_element(&mut self, _ctl: &mut Ctl, _name: &[u8]) {}

    /// `_handle_start_relationship(attributes, num_attributes)`: the first 3D model relationship
    /// is the root model, the others sub-models; the thumbnails' paths.
    fn handle_start_relationship(&mut self, atts: &[&[u8]], n: u32) -> bool {
        let path = get_attribute_value_string(atts, n, TARGET_ATTR);
        let ty = get_attribute_value_string(atts, n, RELS_TYPE_ATTR);
        if ty.starts_with("http://schemas.microsoft.com/3dmanufacturing/")
            && ty.ends_with("3dmodel")
        {
            if self.m_start_part_path.is_empty() {
                self.m_start_part_path = path;
            } else {
                self.m_sub_model_paths.push(path);
            }
        } else if ty.starts_with("http://schemas.openxmlformats.org/") && ty.ends_with("thumbnail")
        {
            if path.ends_with(".png") {
                self.m_thumbnail_path = path;
            }
        } else if ty.starts_with("http://schemas.bambulab.com/")
            && ty.ends_with("cover-thumbnail-middle")
        {
            self.m_thumbnail_middle = path;
        } else if ty.starts_with("http://schemas.bambulab.com/")
            && ty.ends_with("cover-thumbnail-small")
        {
            self.m_thumbnail_small = path;
        }
        true
    }
}

/// The lookup of `_extract_from_archive(archive, path, extract, restore)`: the entry found by its
/// name as UTF-8 (a leading `/` dropped), in the local code page, or by the Unicode path extra
/// field, and its stat; `None` where the C++ reports the file not found (the backup archives a
/// restore tries are not translated).
fn locate_entry(archive: &mut ZipArchive, path: &str) -> Option<FileStat> {
    let mut stat = FileStat::default();
    let path2 = path.strip_prefix('/').unwrap_or(path);
    // try utf8 encoding
    let mut index = archive.reader_locate_file(path2.as_bytes(), None, 0);
    if index < 0 {
        // try native encoding
        let native_path = encode_path(path2);
        // c_str(): up to the first NUL.
        let native = match native_path.iter().position(|&c| c == 0) {
            Some(n) => &native_path[..n],
            None => &native_path[..],
        };
        index = archive.reader_locate_file(native, None, 0);
    }
    if index < 0 {
        // try unicode path extra
        let mut extra = vec![0u8; 1024];
        for i in 0..archive.m_total_files {
            let n = archive.reader_get_extra(i, &mut extra) as usize;
            if n > 0 && path2.as_bytes() == zip_unicode_path_extra_field_decode(&extra[..n]) {
                index = i as i32;
                break;
            }
        }
    }
    if index < 0 || !archive.reader_file_stat(index as u32, &mut stat) {
        return None;
    }
    Some(stat)
}

/// `_apply_transform(instance, transform)`: the instance placed by the build item's transform,
/// unless a scale of it is zero.
fn apply_transform(instance: &mut crate::model::ModelInstance, transform: &Transform3d) {
    let t = Transformation::new(transform);
    // invalid scale value, return
    let s = t.get_scaling_factor();
    if !(s.x() != 0.0 && s.y() != 0.0 && s.z() != 0.0) {
        return;
    }
    instance.set_transformation(t);
}

/// `load_bbs_3mf(path, config, config_substitutions, model, plate_data_list, project_presets,
/// is_bbl_3mf, nullptr, file_version, nullptr, strategy, nullptr, 0)`: the file read into `model`;
/// the importer's errors logged at error after the load, unless an exception escaped it.
///
/// # Errors
///
/// [`ReadError::Message`] for an exception that escapes the importer, with its `what()` (the
/// errors added before it are not logged, as the C++ unwinds past `log_errors`);
/// [`ReadError::NotPorted`] for an entry only a load with configuration reads
/// ([`LOAD_CONFIG_STAGE`]), which the command line's load never asks for.
pub fn load_bbs_3mf(
    path: &str,
    model: &mut Model,
    is_bbl_3mf: &mut bool,
    file_version: &mut Semver,
    strategy: u32,
) -> Result<bool, ReadError> {
    // All import should use "C" locales for number formatting: the parsers here are the C
    // locale's.
    let mut importer = Importer::new();
    let res = importer.load_model_from_file(path, model, strategy, is_bbl_3mf, file_version);
    if !matches!(res, Err(ReadError::Message(_))) {
        importer.log_errors();
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values read from bbs_3mf.cpp:1748-1751 (the sub-model relationships' name: "_rels/"
    // after the last '/', ".rels" appended, a leading '/' dropped) and bbs_3mf.hpp:182-186
    // (LoadStrategy's operator& asks for every bit).
    #[test]
    fn strategies_and_names() {
        let s = load_strategy::LOAD_MODEL | load_strategy::ADD_DEFAULT_INSTANCES;
        assert!(has(s, load_strategy::LOAD_MODEL));
        assert!(!has(s, load_strategy::RESTORE));
        assert!(!has(s, load_strategy::LOAD_CONFIG));
        assert!(has(load_strategy::RESTORE, load_strategy::LOAD_CONFIG));
        assert_eq!(snprintf_1024("x".repeat(1100)).len(), 1023);
        assert_eq!(c_str_or_null(None), "(null)");
    }

    // Expected values read from bbs_3mf.cpp:4241-4248: a zero scale keeps the identity.
    #[test]
    fn a_zero_scale_is_not_applied() {
        let mut i = crate::model::ModelInstance::default();
        let mut t = Transform3d::identity();
        t.m.set(0, 0, 0.0);
        t.m.set(0, 3, 5.0);
        apply_transform(&mut i, &t);
        assert_eq!(i.get_offset(), orca_eigen::Vec3d::zero());
        let mut t = Transform3d::identity();
        t.m.set(1, 3, 5.0);
        apply_transform(&mut i, &t);
        assert_eq!(i.get_offset(), orca_eigen::Vec3d::new(0.0, 5.0, 0.0));
    }

    // Expected values read from bbs_3mf.cpp:3935-4036: the version, the generator, the
    // model's information (unescaped once more), every named item by name, the last name
    // repeated by a nameless metadata.
    #[test]
    fn metadata() {
        let mut im = Importer::new();
        let put = |im: &mut Importer, name: &[u8], text: &[u8]| {
            let atts: Vec<&[u8]> = if name.is_empty() {
                vec![]
            } else {
                vec![b"name", name]
            };
            im.handle_start_metadata((&atts, atts.len() as u32));
            im.handle_xml_characters(text);
            im.handle_end_metadata();
        };
        put(&mut im, b"BambuStudio:3mfVersion", b"1x");
        put(&mut im, b"Application", b"BambuStudio-01.10.02.73");
        put(&mut im, b"Title", b"a &amp;amp; b");
        put(&mut im, b"Designer", b"me");
        put(&mut im, b"CopyRight", b"c1");
        put(&mut im, b"Copyright", b"c2");
        put(&mut im, b"", b"again");
        assert_eq!(im.m_version, 1);
        assert!(im.m_is_bbl_3mf);
        assert_eq!(
            im.m_bambuslicer_generator_version,
            Some(Semver {
                major: 1,
                minor: 10,
                patch: 273,
                ..Semver::default()
            })
        );
        assert_eq!(im.model_info.model_name, "a &amp; b");
        assert_eq!(im.m_designer, "me");
        assert_eq!(im.model_info.copyright, "again");
        assert_eq!(
            im.model_info
                .metadata_items
                .get("Copyright")
                .map(String::as_str),
            Some("again")
        );
        assert_eq!(
            im.model_info
                .metadata_items
                .get("CopyRight")
                .map(String::as_str),
            Some("c1")
        );
        assert_eq!(im.model_info.metadata_items.len(), 6);

        let mut orca = Importer::new();
        put(&mut orca, b"OrcaSlicer", b"2.3");
        assert!(orca.m_is_bbl_3mf);
        let mut bad = Importer::new();
        put(&mut bad, b"OrcaSlicer", b"two");
        assert!(!bad.m_is_bbl_3mf);
        put(&mut bad, b"Application", b"PrusaSlicer-2.8");
        assert!(!bad.m_is_bbl_3mf);
    }
}
