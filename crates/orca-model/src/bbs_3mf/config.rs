// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: _BBS_3MF_Importer's structs CurrentConfig, CurrentInstance, Metadata
// and ObjectMetadata with VolumeMetadata (lines 828-901), PlateData of Format/bbs_3mf.hpp (the
// members a later line reads), the handlers of Metadata/model_settings.config
// (_handle_start_config_xml_element and _handle_end_config_xml_element, 3440-3515, with their
// static callbacks, 5300-5312; the text and shape configurations, 4073-4134; the objects, parts,
// mesh statistics, metadata, plates, instances, filaments, warnings, assembly items and text
// information, 4251-4806), of src/libslic3r/Format/bbs_3mf.cpp of OrcaSlicer v2.4.2, translated
// on 2026-11-11. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! `Metadata/model_settings.config`: the objects' and parts' settings and metadata, their mesh
//! statistics, text and shape configurations, the plates with their instances, and the assembly
//! items, parsed with expat (`orca-xml`) through the importer's handlers.
//!
//! The handlers keep one current object, part and plate, as the C++ does (`m_curr_config`,
//! `m_curr_plater`). A handler that answers false stops the parser (`_stop_xml_parser`), so the
//! parse fails with "parsing aborted". `std::stoi` of a plate's
//! `other_layers_print_sequence_nums` throws from inside the handler; here the handler keeps the
//! exception's text in the importer (`m_thrown`) and stops the parser, and the caller lets it
//! escape the importer as the C++ unwinds it ([`super::ReadError::Message`], no `log_errors`).
//!
//! A plate is made at each `<plate>` and kept, at its end, by its `plater_id` unless one of that
//! id is kept already; a `<plate>` inside another replaces the open one, which is lost (leaked in
//! the C++). The plates live in an arena, the C++'s pointers being indexes into it. Of a plate,
//! only what a later line reads is kept: its id, its instances (`obj_inst_map`) and the four
//! thumbnail names the assembly logs. The rest goes into the plate list, which the command line
//! releases unread (`cli/run.cpp`, `load_model`), and is parsed here as the C++ parses it and then
//! dropped (`parity/HAZARDS.md`, the 3MF table): `locked`, `plate_name`, the plate's `config`
//! (`curr_bed_type`, `print_sequence`, `first_layer_print_sequence`,
//! `other_layers_print_sequence`, `other_layers_print_sequence_nums`, `spiral_mode`,
//! `filament_map_mode`, `filament_map`, `enable_filament_dynamic_map`, `has_filament_switcher`),
//! `filament_maps`, `gcode_file`, `pattern_bbox_file`, `gcode_prediction`, `gcode_weight`,
//! `toolpath_outside`, `is_support_used`, `is_label_object_enabled`, `printer_model_id`,
//! `nozzle_diameters`, `slice_filaments_info` (each `<filament>`'s id, type, colour, use in
//! metres and grams, tray id, use for objects and supports, nozzle group ids, nozzle diameter and
//! volume type) and `warnings` (each `<warning>`'s message and level).
//!
//! Two lines index a part with no bound: `<mesh_stat>` and `<text_info>` test the current part
//! with `volumes.size() - 1 < volume_id` in `size_t`, which an object with no parts passes, and
//! then index its empty list (a null pointer under MSVC: the engine dies). That happens when an
//! `<object>` opens inside a part of another. Here the test refuses it as it refuses a part past
//! the list ("can not find part for mesh_stats", "can not find part for text_info"); no record
//! can hold the C++'s answer, the oracle dying there (`parity/HAZARDS.md`, the 3MF table).
//!
//! `m_parsing_slice_info` is set only around `Metadata/slice_info.config`, which a load without
//! `LoadConfig` never reads, so the branches it takes are not reached here.

use std::collections::BTreeMap;

use orca_mesh::triangle_mesh::RepairedMeshErrors;
use orca_xml::{Ctl, Handler};

use super::Importer;
use super::attrs::{
    get_attribute_value_int, get_attribute_value_string, get_offset_from_3mf_specs_string,
    get_transform_from_3mf_specs_string,
};
use super::entries::{parse_int_list, stoi};
use super::names::{
    ASSEMBLE_ITEM_TAG, ASSEMBLE_TAG, BED_TYPE_ATTR, CONFIG_TAG, ENABLE_FILAMENT_DYNAMIC_MAP_ATTR,
    FILAMENT_COLOR_TAG, FILAMENT_ID_TAG, FILAMENT_MAP_ATTR, FILAMENT_MAP_MODE_ATTR,
    FILAMENT_NOZZLE_DIAMETER_TAG, FILAMENT_NOZZLE_GROUP_ID_TAG, FILAMENT_NOZZLE_VOLUME_TYPE_TAG,
    FILAMENT_TAG, FILAMENT_TRAY_INFO_ID_TAG, FILAMENT_TYPE_TAG, FILAMENT_USED_FOR_OBJECT,
    FILAMENT_USED_FOR_SUPPORT, FILAMENT_USED_G_TAG, FILAMENT_USED_M_TAG,
    FIRST_LAYER_PRINT_SEQUENCE_ATTR, FIRST_TRIANGLE_ID_ATTR, GCODE_FILE_ATTR,
    HAS_FILAMENT_SWITCHER_ATTR, ID_ATTR, IDENTIFYID_ATTR, INSTANCE_TAG, INSTANCEID_ATTR, KEY_ATTR,
    LABEL_OBJECT_ENABLED_ATTR, LAST_TRIANGLE_ID_ATTR, LOCK_ATTR, MESH_STAT_BACKWARDS_EDGES,
    MESH_STAT_DEGENERATED_FACETS, MESH_STAT_EDGES_FIXED, MESH_STAT_FACETS_REMOVED,
    MESH_STAT_FACETS_RESERVED, MESH_STAT_TAG, METADATA_TAG, NO_LIGHT_THUMBNAIL_FILE_ATTR,
    NOZZLE_DIAMETERS_ATTR, OBJECT_ID_ATTR, OBJECT_TAG, OFFSET_ATTR,
    OTHER_LAYERS_PRINT_SEQUENCE_ATTR, OTHER_LAYERS_PRINT_SEQUENCE_NUMS_ATTR, OUTSIDE_ATTR,
    PART_TAG, PATTERN_BBOX_FILE_ATTR, PICK_FILE_ATTR, PLATE_IDX_ATTR, PLATE_TAG, PLATER_NAME_ATTR,
    PLATERID_ATTR, PRINT_SEQUENCE_ATTR, PRINTER_MODEL_ID_ATTR, SHAPE_TAG, SLICE_PREDICTION_ATTR,
    SLICE_WARNING_TAG, SLICE_WEIGHT_ATTR, SPIRAL_VASE_MODE, SUBTYPE_ATTR, SUPPORT_USED_ATTR,
    TEXT_INFO_TAG, TEXT_TAG, THUMBNAIL_FILE_ATTR, TOP_FILE_ATTR, TRANSFORM_ATTR, VALUE_ATTR,
    VOLUME_TAG, WARNING_MSG_TAG,
};
use super::text::{read as read_text_configuration, read_emboss_shape, read_old};
use crate::emboss::{EmbossShape, TextConfiguration};
use crate::model::{Model, ModelVolumeType};
use crate::utils::xml_unescape;

/// `CurrentConfig`: the object and part `model_settings.config`'s handlers are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CurrentConfig {
    pub(super) object_id: i32,
    pub(super) volume_id: i32,
}

impl Default for CurrentConfig {
    fn default() -> Self {
        CurrentConfig {
            object_id: -1,
            volume_id: -1,
        }
    }
}

/// `CurrentInstance`: the plate instance being read (`load_model_from_file` sets it to -1, -1,
/// 0 before the load).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CurrentInstance {
    pub(super) object_id: i32,
    pub(super) instance_id: i32,
    pub(super) identify_id: i32,
}

impl Default for CurrentInstance {
    fn default() -> Self {
        CurrentInstance {
            object_id: -1,
            instance_id: -1,
            identify_id: 0,
        }
    }
}

/// `Metadata`: a key and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Metadata {
    pub(super) key: String,
    pub(super) value: String,
}

/// `ObjectMetadata::VolumeMetadata`: a part's range of triangles or sub-object, its type,
/// metadata, mesh statistics and text and shape configurations.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct VolumeMetadata {
    //BBS: refine the part logic
    /// Read only by `_generate_volumes`, which the C++ has commented out.
    #[allow(dead_code)]
    pub(super) first_triangle_id: u32,
    /// As `first_triangle_id`.
    #[allow(dead_code)]
    pub(super) last_triangle_id: u32,
    pub(super) subobject_id: i32,
    pub(super) metadata: Vec<Metadata>,
    pub(super) mesh_stats: RepairedMeshErrors,
    pub(super) part_type: ModelVolumeType,
    pub(super) text_configuration: Option<TextConfiguration>,
    pub(super) shape_configuration: Option<EmbossShape>,
}

impl VolumeMetadata {
    /// `VolumeMetadata(first_triangle_id, last_triangle_id, type)`.
    fn with_triangles(first_triangle_id: u32, last_triangle_id: u32, ty: ModelVolumeType) -> Self {
        VolumeMetadata {
            first_triangle_id,
            last_triangle_id,
            subobject_id: -1,
            metadata: Vec::new(),
            mesh_stats: RepairedMeshErrors::default(),
            part_type: ty,
            text_configuration: None,
            shape_configuration: None,
        }
    }

    /// `VolumeMetadata(sub_id, type)`.
    pub(super) fn with_subobject(sub_id: i32, ty: ModelVolumeType) -> Self {
        VolumeMetadata {
            first_triangle_id: 0,
            last_triangle_id: 0,
            subobject_id: sub_id,
            metadata: Vec::new(),
            mesh_stats: RepairedMeshErrors::default(),
            part_type: ty,
            text_configuration: None,
            shape_configuration: None,
        }
    }
}

/// `ObjectMetadata`: an object's metadata and its parts'.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ObjectMetadata {
    pub(super) metadata: Vec<Metadata>,
    pub(super) volumes: Vec<VolumeMetadata>,
}

/// `PlateData`, with the members a later line reads; the others are parsed and dropped (the
/// module's header names them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlateData {
    pub(super) plate_index: i32,
    pub(super) obj_inst_map: BTreeMap<i32, (i32, i32)>,
    pub(super) thumbnail_file: String,
    pub(super) no_light_thumbnail_file: String,
    pub(super) top_file: String,
    pub(super) pick_file: String,
}

impl Default for PlateData {
    /// `PlateData()`.
    fn default() -> Self {
        PlateData {
            plate_index: -1,
            obj_inst_map: BTreeMap::new(),
            thumbnail_file: String::new(),
            no_light_thumbnail_file: String::new(),
            top_file: String::new(),
            pick_file: String::new(),
        }
    }
}

/// The attributes of a start tag and their count (`XML_GetSpecifiedAttributeCount`).
type Atts<'a, 'b> = (&'a [&'b [u8]], u32);

/// What a handler answers: `Ok(false)` stops the parser, `Err` is an exception it throws (its
/// `what()`).
type Handled = Result<bool, String>;

/// The handlers of `model_settings.config`'s parse (`_handle_start_config_xml_element` and the
/// others, with the importer as `userData`).
pub(super) struct ConfigXml<'a> {
    pub(super) importer: &'a mut Importer,
    pub(super) model: &'a mut Model,
}

impl Handler for ConfigXml<'_> {
    fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
        self.importer
            .handle_start_config_xml_element(ctl, self.model, name, atts);
    }
    fn end_element(&mut self, ctl: &mut Ctl, name: &[u8]) {
        self.importer.handle_end_config_xml_element(ctl, name);
    }
    fn character_data(&mut self, _: &mut Ctl, s: &[u8]) {
        self.importer.handle_xml_characters(s);
    }
}

/// `atoi(value.c_str())`.
fn atoi(s: &str) -> i32 {
    orca_fmt::parse::atoi(s.as_bytes())
}

/// `atof(value.c_str())`.
fn atof(s: &str) -> f64 {
    orca_fmt::parse::atof(s.as_bytes())
}

/// `std::istringstream(value) >> std::boolalpha >> b`: what `b` holds after, given what it held.
fn boolalpha(s: &str, before: bool) -> bool {
    orca_fmt::istream::get_bool_alpha(s.as_bytes()).or(before)
}

/// `get_vector_from_string(str)`: `while (stream >> value) results.push_back(value);`.
fn get_vector_from_string(s: &str) -> Vec<i32> {
    let b = s.as_bytes();
    let mut results = Vec::new();
    let mut pos = 0usize;
    loop {
        let x = orca_fmt::istream::get_i32(&b[pos..]);
        match x.value {
            Some(value) if !x.fail => results.push(value),
            _ => break,
        }
        pos += x.read;
    }
    results
}

/// `ConfigOptionEnum<T>::from_string(value, v)` with the enum of the option `opt_key`: the
/// value, or `None` when the enum has no such name.
fn enum_from_string(opt_key: &str, value: &str) -> Option<i32> {
    orca_config::get(opt_key)?
        .enum_keys_map?
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, v)| *v)
}

impl Importer {
    /// `_handle_start_config_xml_element(name, attributes)`.
    pub(super) fn handle_start_config_xml_element(
        &mut self,
        ctl: &mut Ctl,
        model: &mut Model,
        name: &[u8],
        attributes: &[&[u8]],
    ) {
        if self.m_thrown.is_some() {
            return;
        }
        let num_attributes = ctl.specified_attribute_count() as u32;
        let a = (attributes, num_attributes);
        let res = if name == CONFIG_TAG.as_bytes() {
            Ok(self.handle_start_config(a))
        } else if name == OBJECT_TAG.as_bytes() {
            Ok(self.handle_start_config_object(a))
        } else if name == VOLUME_TAG.as_bytes() || name == PART_TAG.as_bytes() {
            Ok(self.handle_start_config_volume(a))
        } else if name == MESH_STAT_TAG.as_bytes() {
            Ok(self.handle_start_config_volume_mesh(a))
        } else if name == METADATA_TAG.as_bytes() {
            self.handle_start_config_metadata(a)
        } else if name == SHAPE_TAG.as_bytes() {
            Ok(self.handle_start_shape_configuration(a))
        } else if name == TEXT_TAG.as_bytes() {
            Ok(self.handle_start_text_configuration(a))
        } else if name == PLATE_TAG.as_bytes() {
            Ok(self.handle_start_config_plater(a))
        } else if name == INSTANCE_TAG.as_bytes() {
            Ok(self.handle_start_config_plater_instance(a))
        } else if name == FILAMENT_TAG.as_bytes() {
            Ok(self.handle_start_config_filament(a))
        } else if name == SLICE_WARNING_TAG.as_bytes() {
            Ok(self.handle_start_config_warning(a))
        } else if name == ASSEMBLE_TAG.as_bytes() {
            Ok(self.handle_start_assemble(a))
        } else if name == ASSEMBLE_ITEM_TAG.as_bytes() {
            Ok(self.handle_start_assemble_item(model, a))
        } else if name == TEXT_INFO_TAG.as_bytes() {
            Ok(self.handle_start_text_info_item(a))
        } else {
            Ok(true)
        };
        self.answer(ctl, res);
    }

    /// `_handle_end_config_xml_element(name)`.
    pub(super) fn handle_end_config_xml_element(&mut self, ctl: &mut Ctl, name: &[u8]) {
        if self.m_thrown.is_some() {
            return;
        }
        let res = if name == CONFIG_TAG.as_bytes() {
            self.handle_end_config()
        } else if name == OBJECT_TAG.as_bytes() {
            self.handle_end_config_object()
        } else if name == VOLUME_TAG.as_bytes() || name == PART_TAG.as_bytes() {
            self.handle_end_config_volume()
        } else if name == MESH_STAT_TAG.as_bytes() {
            self.handle_end_config_volume_mesh()
        } else if name == METADATA_TAG.as_bytes() {
            self.handle_end_config_metadata()
        } else if name == PLATE_TAG.as_bytes() {
            self.handle_end_config_plater()
        } else if name == FILAMENT_TAG.as_bytes() {
            self.handle_end_config_filament()
        } else if name == INSTANCE_TAG.as_bytes() {
            self.handle_end_config_plater_instance()
        } else if name == ASSEMBLE_TAG.as_bytes() {
            self.handle_end_assemble()
        } else if name == ASSEMBLE_ITEM_TAG.as_bytes() {
            self.handle_end_assemble_item()
        } else {
            true
        };
        self.answer(ctl, Ok(res));
    }

    /// `if (!res) _stop_xml_parser();`, or the exception a handler threw kept for the caller to
    /// let escape, the parser stopped so that no handler runs after it.
    fn answer(&mut self, ctl: &mut Ctl, res: Handled) {
        match res {
            Ok(true) => {}
            Ok(false) => self.stop_xml_parser(ctl, ""),
            Err(what) => {
                self.m_thrown = Some(what);
                ctl.stop_parser(false);
            }
        }
    }

    /// `_handle_start_config(...)`: do nothing.
    fn handle_start_config(&mut self, _: Atts<'_, '_>) -> bool {
        true
    }

    /// `_handle_end_config()`: do nothing.
    fn handle_end_config(&mut self) -> bool {
        true
    }

    /// `_handle_start_config_object(attributes, num_attributes)`.
    fn handle_start_config_object(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        if self.m_parsing_slice_info {
            return true;
        }
        let object_id = get_attribute_value_int(atts, n, ID_ATTR);
        if self.m_objects_metadata.contains_key(&object_id) {
            self.add_error(&format!(
                "Duplicated object id: {object_id} in model_settings.config"
            ));
            return false;
        }

        // Added because of github #3435, currently not used by PrusaSlicer
        // int instances_count_id = bbs_get_attribute_value_int(attributes, num_attributes, INSTANCESCOUNT_ATTR);

        self.m_objects_metadata
            .insert(object_id, ObjectMetadata::default());
        self.m_curr_config.object_id = object_id;
        true
    }

    /// `_handle_end_config_object()`.
    fn handle_end_config_object(&mut self) -> bool {
        self.m_curr_config.object_id = -1;
        true
    }

    /// `_handle_start_config_volume(attributes, num_attributes)`: a `<volume>` or `<part>`.
    fn handle_start_config_volume(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        let object_id = self.m_curr_config.object_id;
        let Some(object) = self.m_objects_metadata.get_mut(&object_id) else {
            self.add_error(&format!("can not find object for part, id {object_id}"));
            return false;
        };

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let volume_id = object.volumes.len() as i32;
        self.m_curr_config.volume_id = volume_id;

        #[allow(clippy::cast_sign_loss)]
        let first_triangle_id = get_attribute_value_int(atts, n, FIRST_TRIANGLE_ID_ATTR) as u32;
        #[allow(clippy::cast_sign_loss)]
        let last_triangle_id = get_attribute_value_int(atts, n, LAST_TRIANGLE_ID_ATTR) as u32;

        //BBS: refine the part type logic
        let subtype_str = get_attribute_value_string(atts, n, SUBTYPE_ATTR);
        let ty = ModelVolumeType::from_string(&subtype_str);

        let subbject_id = get_attribute_value_int(atts, n, ID_ATTR);

        if last_triangle_id > 0 {
            object.volumes.push(VolumeMetadata::with_triangles(
                first_triangle_id,
                last_triangle_id,
                ty,
            ));
        } else {
            object
                .volumes
                .push(VolumeMetadata::with_subobject(subbject_id, ty));
        }
        true
    }

    /// The current part's index for `<mesh_stat>` and `<text_info>`, or `None` where they refuse
    /// it: `(volume_id == -1) || ((volumes.size() - 1) < volume_id)` in `size_t`, and an empty
    /// list, which the C++ test passes and then indexes (the module's header).
    fn current_volume(&self, volumes_len: usize) -> Option<usize> {
        let volume_id = self.m_curr_config.volume_id;
        if volume_id == -1 {
            return None;
        }
        #[allow(clippy::cast_sign_loss)]
        let id = volume_id as usize;
        if volumes_len.wrapping_sub(1) < id || volumes_len == 0 {
            return None;
        }
        Some(id)
    }

    /// `_handle_start_config_volume_mesh(attributes, num_attributes)`: a part's mesh statistics.
    fn handle_start_config_volume_mesh(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        let object_id = self.m_curr_config.object_id;
        let Some(len) = self
            .m_objects_metadata
            .get(&object_id)
            .map(|o| o.volumes.len())
        else {
            self.add_error(&format!(
                "can not find object for mesh_stats, id {object_id}"
            ));
            return false;
        };
        let Some(volume_id) = self.current_volume(len) else {
            self.add_error("can not find part for mesh_stats");
            return false;
        };

        let edges_fixed = get_attribute_value_int(atts, n, MESH_STAT_EDGES_FIXED);
        let degenerate_facets = get_attribute_value_int(atts, n, MESH_STAT_DEGENERATED_FACETS);
        let facets_removed = get_attribute_value_int(atts, n, MESH_STAT_FACETS_REMOVED);
        let facets_reversed = get_attribute_value_int(atts, n, MESH_STAT_FACETS_RESERVED);
        let backwards_edges = get_attribute_value_int(atts, n, MESH_STAT_BACKWARDS_EDGES);

        if let Some(object) = self.m_objects_metadata.get_mut(&object_id) {
            object.volumes[volume_id].mesh_stats = RepairedMeshErrors {
                edges_fixed,
                degenerate_facets,
                facets_removed,
                facets_reversed,
                backwards_edges,
            };
        }
        true
    }

    /// `_handle_end_config_volume()`.
    fn handle_end_config_volume(&mut self) -> bool {
        self.m_curr_config.volume_id = -1;
        true
    }

    /// `_handle_end_config_volume_mesh()`: do nothing.
    fn handle_end_config_volume_mesh(&mut self) -> bool {
        true
    }

    /// `_handle_start_config_metadata(attributes, num_attributes)`: an object's or part's
    /// metadata outside a plate, a plate's inside one. Throws `std::stoi`'s exception for a
    /// plate's `other_layers_print_sequence_nums`.
    #[allow(clippy::too_many_lines)]
    fn handle_start_config_metadata(&mut self, (atts, n): Atts<'_, '_>) -> Handled {
        //std::string type = bbs_get_attribute_value_string(attributes, num_attributes, TYPE_ATTR);
        let key = get_attribute_value_string(atts, n, KEY_ATTR);
        let value = get_attribute_value_string(atts, n, VALUE_ATTR);
        if key.is_empty() {
            return Ok(true);
        }

        // get_vector_array_from_string, a lambda the C++ defines here, is never called.

        if self.m_curr_plater.is_none() && !self.m_parsing_slice_info {
            let object_id = self.m_curr_config.object_id;
            let volume_id = self.m_curr_config.volume_id;
            let Some(object) = self.m_objects_metadata.get_mut(&object_id) else {
                self.add_error(&format!("Cannot find object for metadata, id {object_id}"));
                return Ok(false);
            };
            if volume_id == -1 {
                object.metadata.push(Metadata { key, value });
            } else {
                #[allow(clippy::cast_sign_loss)]
                let id = volume_id as usize;
                if id < object.volumes.len() {
                    object.volumes[id].metadata.push(Metadata { key, value });
                }
            }
            return Ok(true);
        }

        //plater: m_curr_plater is set here, m_parsing_slice_info never being set by this load
        let Some(cur) = self.m_curr_plater else {
            return Ok(true);
        };
        if key == PLATERID_ATTR {
            self.m_plates[cur].plate_index = atoi(&value);
        } else if key == PLATER_NAME_ATTR {
            let _plate_name = xml_unescape(&value); // not kept
        } else if key == LOCK_ATTR {
            let _locked = boolalpha(&value, false); // not kept
        } else if key == BED_TYPE_ATTR {
            let bed_type_value = if value == "SuperTack Plate" {
                "Supertack Plate"
            } else {
                value.as_str()
            };
            // BedType::btPC unless from_string finds the name; config's curr_bed_type, not kept
            let _bed_type = enum_from_string("curr_bed_type", bed_type_value);
        } else if key == PRINT_SEQUENCE_ATTR {
            // PrintSequence::ByLayer unless found; config's print_sequence, not kept
            let _print_sequence = enum_from_string("print_sequence", &value);
        } else if key == FIRST_LAYER_PRINT_SEQUENCE_ATTR || key == OTHER_LAYERS_PRINT_SEQUENCE_ATTR
        {
            // config's first_layer_print_sequence or other_layers_print_sequence, not kept
            let _print_sequence = get_vector_from_string(&value);
        } else if key == OTHER_LAYERS_PRINT_SEQUENCE_NUMS_ATTR {
            let _other_layers_print_sequence_nums = stoi(value.as_bytes())?; // not kept
        } else if key == SPIRAL_VASE_MODE {
            let _spiral_mode = boolalpha(&value, false); // not kept
        } else if key == FILAMENT_MAP_MODE_ATTR {
            // handle old versions, only load manual params
            if value != "Auto" {
                // FilamentMapMode::fmmAutoForFlush unless found; not kept
                let _map_mode = enum_from_string("filament_map_mode", &value);
            }
        } else if key == FILAMENT_MAP_ATTR {
            let mut filament_map = get_vector_from_string(&value);
            for f in &mut filament_map {
                if *f < 1 {
                    *f = 1;
                }
            }
            let _filament_maps = filament_map; // filament_maps and config's filament_map, not kept
        } else if key == GCODE_FILE_ATTR {
            // gcode_file: not kept
        } else if key == THUMBNAIL_FILE_ATTR {
            self.m_plates[cur].thumbnail_file = value;
        } else if key == NO_LIGHT_THUMBNAIL_FILE_ATTR {
            self.m_plates[cur].no_light_thumbnail_file = value;
        } else if key == TOP_FILE_ATTR {
            self.m_plates[cur].top_file = value;
        } else if key == PICK_FILE_ATTR {
            self.m_plates[cur].pick_file = value;
        }
        //else if (key == PATTERN_FILE_ATTR)
        //{
        //    m_curr_plater->pattern_file = value;
        //}
        else if key == PATTERN_BBOX_FILE_ATTR {
            // pattern_bbox_file: not kept
        } else if key == INSTANCEID_ATTR {
            self.m_curr_instance.instance_id = atoi(&value);
        } else if key == IDENTIFYID_ATTR {
            self.m_curr_instance.identify_id = atoi(&value);
        } else if key == OBJECT_ID_ATTR {
            self.m_curr_instance.object_id = atoi(&value);
        } else if key == PLATE_IDX_ATTR {
            let plate_index = atoi(&value);
            if let Some(&it) = self.m_plater_data.get(&plate_index) {
                self.m_curr_plater = Some(it);
            }
        } else if key == SLICE_PREDICTION_ATTR || key == SLICE_WEIGHT_ATTR {
            // gcode_prediction, gcode_weight: not kept
        } else if key == OUTSIDE_ATTR
            || key == SUPPORT_USED_ATTR
            || key == LABEL_OBJECT_ENABLED_ATTR
            || key == ENABLE_FILAMENT_DYNAMIC_MAP_ATTR
            || key == HAS_FILAMENT_SWITCHER_ATTR
        {
            // toolpath_outside, is_support_used, is_label_object_enabled and config's
            // enable_filament_dynamic_map and has_filament_switcher: not kept
            let _flag = boolalpha(&value, false);
        } else if key == PRINTER_MODEL_ID_ATTR || key == NOZZLE_DIAMETERS_ATTR {
            // printer_model_id, nozzle_diameters: not kept
        }

        Ok(true)
    }

    /// `_handle_end_config_metadata()`: do nothing.
    fn handle_end_config_metadata(&mut self) -> bool {
        true
    }

    /// `_handle_start_config_filament(attributes, num_attributes)`: a plate's filament, parsed
    /// and not kept (`slice_filaments_info`).
    fn handle_start_config_filament(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        if self.m_curr_plater.is_some() {
            let id = get_attribute_value_string(atts, n, FILAMENT_ID_TAG);
            let _type = get_attribute_value_string(atts, n, FILAMENT_TYPE_TAG);
            let _color = get_attribute_value_string(atts, n, FILAMENT_COLOR_TAG);
            let used_m = get_attribute_value_string(atts, n, FILAMENT_USED_M_TAG);
            let used_g = get_attribute_value_string(atts, n, FILAMENT_USED_G_TAG);
            let _filament_id = get_attribute_value_string(atts, n, FILAMENT_TRAY_INFO_ID_TAG);
            let used_for_object = get_attribute_value_string(atts, n, FILAMENT_USED_FOR_OBJECT);
            let used_for_support = get_attribute_value_string(atts, n, FILAMENT_USED_FOR_SUPPORT);
            let group_id = get_attribute_value_string(atts, n, FILAMENT_NOZZLE_GROUP_ID_TAG);
            let nozzle_diameter = get_attribute_value_string(atts, n, FILAMENT_NOZZLE_DIAMETER_TAG);
            let _volume_type = get_attribute_value_string(atts, n, FILAMENT_NOZZLE_VOLUME_TYPE_TAG);
            // FilamentInfo filament_info, not kept
            let _id = atoi(&id).wrapping_sub(1);
            let _used_m = atof(&used_m);
            let _used_g = atof(&used_g);
            let _used_for_object = boolalpha(&used_for_object, false);
            let _used_for_support = boolalpha(&used_for_support, false);
            let _group_id = parse_int_list(&group_id);
            let _nozzle_diameter = atof(&nozzle_diameter);
        }
        true
    }

    /// `_handle_end_config_filament()`: do nothing.
    fn handle_end_config_filament(&mut self) -> bool {
        true
    }

    /// `_handle_start_config_warning(attributes, num_attributes)`: a plate's warning, parsed and
    /// not kept.
    fn handle_start_config_warning(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        if self.m_curr_plater.is_some() {
            let _msg = get_attribute_value_string(atts, n, WARNING_MSG_TAG);
            let lvl_str = get_attribute_value_string(atts, n, "level");
            let _level = atoi(&lvl_str);
        }
        true
    }

    /// `_handle_start_config_plater(...)`: a new current plate; one open before is lost.
    fn handle_start_config_plater(&mut self, _: Atts<'_, '_>) -> bool {
        if !self.m_parsing_slice_info {
            self.m_plates.push(PlateData::default());
            self.m_curr_plater = Some(self.m_plates.len() - 1);
        }
        true
    }

    /// `_handle_end_config_plater()`: the plate kept by its id, unless one of that id is.
    ///
    /// The assembly (piece 3, `bbs_3mf.cpp:2276`) tests the ids with `it->first >
    /// m_plater_data.size()`, an `int` against a `size_t`, so an id of 0 passes and the C++
    /// writes `plate_data_list[-1]`, before the list's storage. The port never writes out of
    /// bounds: the plate list is not kept (the command line releases it unread), so an id of 0
    /// passes as in the C++, nothing is written for it, and its instances' loaded ids are set
    /// like any plate's. No record can hold the C++'s answer, a write before a heap block whose
    /// outcome is undefined (`parity/HAZARDS.md`, the 3MF table).
    fn handle_end_config_plater(&mut self) -> bool {
        let Some(cur) = self.m_curr_plater else {
            self.add_error("_handle_end_config_plater: don't find plate created before");
            return false;
        };
        self.m_plater_data
            .entry(self.m_plates[cur].plate_index)
            .or_insert(cur);
        self.m_curr_plater = None;
        true
    }

    /// `_handle_start_config_plater_instance(...)`.
    fn handle_start_config_plater_instance(&mut self, _: Atts<'_, '_>) -> bool {
        if self.m_curr_plater.is_none() {
            self.add_error("_handle_start_config_plater_instance: don't find plate created before");
            return false;
        }

        //do nothing
        true
    }

    /// `_handle_end_config_plater_instance()`: the instance into the plate's map, unless it lacks
    /// its object or instance id or the object has one already.
    fn handle_end_config_plater_instance(&mut self) -> bool {
        let Some(cur) = self.m_curr_plater else {
            self.add_error("_handle_end_config_plater_instance: don't find plate created before");
            return false;
        };
        let i = self.m_curr_instance;
        if i.object_id == -1 || i.instance_id == -1 {
            //add_error("invalid object id/instance id");
            //skip this instance
            self.m_curr_instance = CurrentInstance::default();
            return true;
        }

        self.m_plates[cur]
            .obj_inst_map
            .entry(i.object_id)
            .or_insert((i.instance_id, i.identify_id));
        self.m_curr_instance = CurrentInstance::default();
        true
    }

    /// `_handle_start_assemble(...)`.
    fn handle_start_assemble(&mut self, _: Atts<'_, '_>) -> bool {
        true
    }

    /// `_handle_end_assemble()`: do nothing.
    fn handle_end_assemble(&mut self) -> bool {
        true
    }

    /// `_handle_start_assemble_item(attributes, num_attributes)`: an instance's place in the
    /// assembly view, its object found through the index paths, then the built objects.
    fn handle_start_assemble_item(&mut self, model: &mut Model, (atts, n): Atts<'_, '_>) -> bool {
        if !self.m_load_model {
            return true;
        }

        let object_id = get_attribute_value_int(atts, n, OBJECT_ID_ATTR);
        let instance_id = get_attribute_value_int(atts, n, INSTANCEID_ATTR);

        let Some(path) = self.m_index_paths.get(&object_id) else {
            self.add_error(&format!(
                "can not find object for assemble item, id= {object_id}"
            ));
            return false;
        };
        let temp_id = (path.clone(), object_id);
        let Some(&object_index) = self.m_objects.get(&temp_id) else {
            self.add_error(&format!(
                "can not find object for assemble item, id= {object_id}"
            ));
            return false;
        };

        let transform = get_transform_from_3mf_specs_string(&get_attribute_value_string(
            atts,
            n,
            TRANSFORM_ATTR,
        ));
        let ofs2ass =
            get_offset_from_3mf_specs_string(&get_attribute_value_string(atts, n, OFFSET_ATTR));
        // `object_id < objects.size()` and `instance_id < instances.size()` in `size_t`: a
        // negative id is past any size.
        if let Ok(oi) = usize::try_from(object_index)
            && oi < model.objects.len()
            && let Ok(ii) = usize::try_from(instance_id)
            && ii < model.objects[oi].instances.len()
        {
            let instance = &mut model.objects[oi].instances[ii];
            instance.set_assemble_from_transform(&transform);
            instance.set_offset_to_assembly(ofs2ass);
        }
        true
    }

    /// `_handle_end_assemble_item()`.
    fn handle_end_assemble_item(&mut self) -> bool {
        true
    }

    /// `_handle_start_text_info_item(attributes, num_attributes)`: the legacy text information,
    /// which Orca does not read (its body is commented out); refused without an object or part.
    fn handle_start_text_info_item(&mut self, _: Atts<'_, '_>) -> bool {
        let object_id = self.m_curr_config.object_id;
        let Some(len) = self
            .m_objects_metadata
            .get(&object_id)
            .map(|o| o.volumes.len())
        else {
            self.add_error(&format!(
                "can not find object for text_info, id {object_id}"
            ));
            return false;
        };
        let Some(volume_id) = self.current_volume(len) else {
            self.add_error("can not find part for text_info");
            return false;
        };

        let has_text = self.m_objects_metadata[&object_id].volumes[volume_id]
            .text_configuration
            .is_some();
        if has_text {
            self.add_error("Both text_info and text_configuration found, ignore legacy text_info");
            return true;
        }

        // TODO: Orca: support legacy text info (the C++ keeps the reading commented out)
        true
    }

    /// `_handle_start_text_configuration(attributes, num_attributes)`: the last part's text, and
    /// for a file without shapes its shape from the same attributes.
    fn handle_start_text_configuration(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        let object_id = self.m_curr_config.object_id;
        let Some(object) = self.m_objects_metadata.get_mut(&object_id) else {
            self.add_error("Can not assign volume mesh to a valid object");
            return false;
        };
        let Some(volume) = object.volumes.last_mut() else {
            self.add_error("Can not assign mesh to a valid volume");
            return false;
        };
        volume.text_configuration = read_text_configuration(atts, n);
        if volume.text_configuration.is_none() {
            return false;
        }

        // Is 3mf version with shapes?
        if volume.shape_configuration.is_some() {
            return true;
        }

        // Back compatibility for 3mf version without shapes
        volume.shape_configuration = Some(read_old(atts, n));
        true
    }

    /// `_handle_start_shape_configuration(attributes, num_attributes)`: the last part's shape,
    /// with its svg file's data when the archive's entry was read before.
    fn handle_start_shape_configuration(&mut self, (atts, n): Atts<'_, '_>) -> bool {
        let object_id = self.m_curr_config.object_id;
        let Some(object) = self.m_objects_metadata.get_mut(&object_id) else {
            self.add_error("Can not assign volume mesh to a valid object");
            return false;
        };
        let Some(volume) = object.volumes.last_mut() else {
            self.add_error("Can not assign mesh to a valid volume");
            return false;
        };
        volume.shape_configuration = read_emboss_shape(atts, n);
        let Some(shape) = volume.shape_configuration.as_mut() else {
            return false;
        };

        // Fill svg file content into shape_configuration
        let Some(svg) = shape.svg_file.as_mut() else {
            return true; // do not contain svg file
        };

        let path = &svg.path_in_3mf;
        if path.is_empty() {
            return true; // do not contain svg file
        }

        let Some(file) = self.m_path_to_emboss_shape_files.get(path.as_bytes()) else {
            return true; // svg file is not loaded yet
        };

        svg.file_data = Some(file.clone());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values read from bbs_3mf.cpp:4359-4367 (`stream >> int` until it fails) and MSVC's
    // std::stoi texts.
    #[test]
    fn plate_numbers() {
        assert_eq!(get_vector_from_string("1 2  3"), vec![1, 2, 3]);
        assert_eq!(get_vector_from_string(" 4,5"), vec![4]);
        assert_eq!(get_vector_from_string("x 1"), Vec::<i32>::new());
        assert_eq!(get_vector_from_string("7 2147483648 8"), vec![7]);
        assert_eq!(get_vector_from_string(""), Vec::<i32>::new());
        assert!(boolalpha("true", false));
        assert!(!boolalpha("1", true));
        assert!(boolalpha("", true));
        assert_eq!(enum_from_string("print_sequence", "by object"), Some(1));
        assert_eq!(enum_from_string("print_sequence", "nope"), None);
    }

    fn atts<'a>(pairs: &[(&'a str, &'a str)]) -> Vec<&'a [u8]> {
        pairs
            .iter()
            .flat_map(|(k, v)| [k.as_bytes(), v.as_bytes()])
            .collect()
    }

    // Expected values read from bbs_3mf.cpp:4263-4349 and 4649-4702: a part by its triangles or
    // its id, mesh statistics on the current part, a plate kept by its id once, an instance kept
    // once per object, -1 ids skipped.
    #[test]
    fn objects_parts_and_plates() {
        let mut im = Importer::new();
        im.m_curr_config = CurrentConfig::default();
        let o = atts(&[("id", "3")]);
        assert!(im.handle_start_config_object((&o, 2)));
        assert!(!im.handle_start_config_object((&o, 2)));
        assert_eq!(
            im.m_errors.last().map(String::as_str),
            Some("Duplicated object id: 3 in model_settings.config")
        );
        let p = atts(&[("id", "5"), ("subtype", "modifier_part")]);
        assert!(im.handle_start_config_volume((&p, 4)));
        let s = atts(&[("edges_fixed", "2"), ("backwards_edges", "9")]);
        assert!(im.handle_start_config_volume_mesh((&s, 4)));
        assert!(im.handle_end_config_volume());
        let q = atts(&[("firstid", "-1"), ("lastid", "4"), ("subtype", "x")]);
        assert!(im.handle_start_config_volume((&q, 6)));
        let v = &im.m_objects_metadata[&3].volumes;
        assert_eq!(v[0].subobject_id, 5);
        assert_eq!(v[0].part_type, ModelVolumeType::ParameterModifier);
        assert_eq!(v[0].mesh_stats.edges_fixed, 2);
        assert_eq!(v[0].mesh_stats.backwards_edges, 9);
        assert_eq!(v[1].first_triangle_id, u32::MAX);
        assert_eq!(v[1].last_triangle_id, 4);
        assert_eq!(v[1].subobject_id, -1);
        assert_eq!(v[1].part_type, ModelVolumeType::ModelPart);

        // an object opened inside a part: the C++ indexes the new object's empty list
        let o4 = atts(&[("id", "4")]);
        assert!(im.handle_start_config_object((&o4, 2)));
        assert!(!im.handle_start_config_volume_mesh((&s, 4)));
        assert_eq!(
            im.m_errors.last().map(String::as_str),
            Some("can not find part for mesh_stats")
        );

        assert!(im.handle_start_config_plater((&[], 0)));
        let md = |im: &mut Importer, k: &str, v: &str| {
            let a = atts(&[("key", k), ("value", v)]);
            im.handle_start_config_metadata((&a, 4))
        };
        assert_eq!(md(&mut im, "plater_id", "2"), Ok(true));
        assert_eq!(md(&mut im, "thumbnail_file", "t.png"), Ok(true));
        assert!(im.handle_start_config_plater_instance((&[], 0)));
        assert_eq!(md(&mut im, "object_id", "1"), Ok(true));
        assert_eq!(md(&mut im, "instance_id", "0"), Ok(true));
        assert_eq!(md(&mut im, "identify_id", "77"), Ok(true));
        assert!(im.handle_end_config_plater_instance());
        assert_eq!(md(&mut im, "object_id", "1"), Ok(true));
        assert_eq!(md(&mut im, "instance_id", "1"), Ok(true));
        assert!(im.handle_end_config_plater_instance());
        assert_eq!(md(&mut im, "object_id", "2"), Ok(true));
        assert!(im.handle_end_config_plater_instance());
        assert_eq!(
            md(&mut im, "other_layers_print_sequence_nums", "lots"),
            Err("invalid stoi argument".to_owned())
        );
        assert!(im.handle_end_config_plater());
        assert!(im.handle_start_config_plater((&[], 0)));
        assert_eq!(md(&mut im, "plater_id", "2"), Ok(true));
        assert!(im.handle_end_config_plater());
        assert!(!im.handle_end_config_plater());
        assert_eq!(im.m_plater_data.len(), 1);
        let plate = &im.m_plates[im.m_plater_data[&2]];
        assert_eq!(plate.thumbnail_file, "t.png");
        assert_eq!(plate.obj_inst_map.len(), 1);
        assert_eq!(plate.obj_inst_map[&1], (0, 77));
    }
}
