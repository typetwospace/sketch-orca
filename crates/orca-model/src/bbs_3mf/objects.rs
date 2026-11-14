// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: _BBS_3MF_Importer's struct ObjectImporter (lines 933-1072) with its
// handlers and _extract_object_from_archive (5316-5780), and the sub-model branch of
// _load_model_from_file (1757-1824), of src/libslic3r/Format/bbs_3mf.cpp of OrcaSlicer v2.4.2,
// translated on 2026-11-11. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab)
// and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! The sub-model files of the 3MF production extension: the model files the root model's own
//! relationships (`3D/_rels/3dmodel.model.rels`) name, each read by an `ObjectImporter` that opens
//! the archive again and parses the file with handlers of its own, before the root model is
//! parsed. Their objects (meshes, components and colour groups) are merged into the importer's,
//! keyed by the relationship's target as written (with its leading `/`), which is the path the
//! root model's components and build items name.
//!
//! The C++ reads the files in a `tbb::parallel_for` over the importers; here
//! [`orca_tbb::parallel_for`] runs it the same way, each importer behind a lock of its own, and
//! the importer's errors behind the importer's lock (`add_error` takes it in the C++, `:731`), so
//! in a parallel arena they come in the order the workers reach it. The objects are merged in the
//! importers' order, the relationships file's, an id already held keeping the first. Under the
//! oracle's arena (one thread, `parity/oracle/probe/model.cpp`) the files are read one after
//! another on the calling thread, so the errors and the log records the loop writes come in the
//! importers' order, in the log of the thread that reads the file (`parity/HAZARDS.md`, the 3MF
//! table).
//!
//! The build is `/EHsc`, so the `catch` blocks of `_extract_object_from_archive` around miniz's
//! extraction (a C function) are left out: a parse error thrown in the callback reaches
//! `_extract_from_archive`'s handler, which adds `e.what()` with no " for <path>" after it
//! (`parity/HAZARDS.md`, the 3MF table). Here the callback's `Err` carries the text there.
//!
//! What the command line's load does not take, named and not translated: the restore branch of an
//! object's end (a BBL file's object UUID ending in `OBJECT_UUID_SUFFIX` renumbers the object and
//! its components under `Restore`, 5376-5414), and the lookup of a missing entry in the backup
//! archives (`_extract_from_archive`'s `restore`). `object_xml_parser` has no initializer in the
//! C++ and the importers are leaked when a file fails (`return false` skips the merge that deletes
//! them); here each is an owned value, dropped. `object_unit_factor` has no initializer either: a
//! vertex before the file's `<model>` element (a root element of another name) is scaled by
//! whatever the heap held; here the factor starts at 1, millimetres (`parity/HAZARDS.md`).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use orca_eigen::{Vec3f, Vec3i32};
use orca_mesh::log::{Severity, log};
use orca_tbb::partitioner::Partitioner;
use orca_tbb::range::BlockedRange;
use orca_xml::{Ctl, Handler, Parser, Status, error_string};
use orca_zip::extension::{close_zip_reader, open_zip_reader};
use orca_zip::reader::{FileStat, ZipArchive};

use super::attrs::{
    get_attribute_value_float, get_attribute_value_int, get_attribute_value_string,
    get_transform_from_3mf_specs_string, get_unit_factor, is_valid_object_type,
};
use super::names::{
    BBS_3MF_VERSION, BBS_3MF_VERSION1, COLOR_ATTR, COLOR_GROUP_TAG, COLOR_TAG, COMPONENT_TAG,
    COMPONENTS_TAG, CUSTOM_FUZZY_SKIN_ATTR, CUSTOM_SEAM_ATTR, CUSTOM_SUPPORTS_ATTR,
    FACE_PROPERTY_ATTR, ID_ATTR, MESH_TAG, METADATA_TAG, MMU_SEGMENTATION_ATTR, MODEL_TAG,
    NAME_ATTR, OBJECT_TAG, OBJECTID_ATTR, PID_ATTR, PUUID_ATTR, PUUID_LOWER_ATTR, RESOURCES_TAG,
    TRANSFORM_ATTR, TRIANGLE_TAG, TRIANGLES_TAG, TYPE_ATTR, UNIT_ATTR, V1_ATTR, V2_ATTR, V3_ATTR,
    VERTEX_TAG, VERTICES_TAG, X_ATTR, Y_ATTR, Z_ATTR,
};
use super::{
    Component, CurrentObject, Id, Importer, LOAD_MODEL_FROM_FILE, c_str_or_null, locate_entry,
    snprintf_1024,
};

/// The function `__FUNCTION__` names in `extract_object_model`'s log.
const EXTRACT_OBJECT_MODEL: &str =
    "Slic3r::_BBS_3MF_Importer::ObjectImporter::extract_object_model";

/// A lock taken as the C++'s `boost::unique_lock` takes it; a worker that panicked leaves the
/// value as it was.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What an `ObjectImporter` reaches of its `top_importer` while the files are read: its errors,
/// behind its lock (`_BBS_3MF_Base::mutex`).
struct TopImporter {
    errors: Mutex<Vec<String>>,
}

impl TopImporter {
    /// `top_importer->add_error(error)`.
    fn add_error(&self, error: &str) {
        lock(&self.errors).push(error.to_owned());
    }
}

/// `ObjectImporter`: one sub-model file's objects and colours, read into its own maps.
struct ObjectImporter {
    object_list: BTreeMap<Id, CurrentObject>,
    current_object: Option<CurrentObject>,
    object_path: String,
    zip_path: String,
    obj_parse_error: bool,
    obj_parse_error_message: String,

    //local parsed datas
    obj_curr_metadata_name: String,
    obj_curr_characters: Vec<u8>,
    object_unit_factor: f32,
    object_current_color_group: i32,
    object_group_id_to_color: BTreeMap<i32, String>,
    /// Read only by the restore branch of `_handle_object_end_object`, which the command line's
    /// load does not take (not translated).
    #[allow(dead_code)]
    is_bbl_3mf: bool,
}

/// The handlers of a sub-model file's parse (`_handle_object_start_model_xml_element` and the
/// others, with the object importer as `userData`).
struct ObjectXml<'a> {
    importer: &'a mut ObjectImporter,
    top: &'a TopImporter,
}

impl Handler for ObjectXml<'_> {
    fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]) {
        self.importer
            .handle_object_start_model_xml_element(ctl, name, atts);
    }
    fn end_element(&mut self, ctl: &mut Ctl, name: &[u8]) {
        self.importer
            .handle_object_end_model_xml_element(ctl, self.top, name);
    }
    fn character_data(&mut self, _: &mut Ctl, s: &[u8]) {
        self.importer.handle_object_xml_characters(s);
    }
}

impl ObjectImporter {
    /// `ObjectImporter(importer, file_path, obj_path)`.
    fn new(file_path: &str, obj_path: &str) -> ObjectImporter {
        ObjectImporter {
            object_list: BTreeMap::new(),
            current_object: None,
            object_path: obj_path.to_owned(),
            zip_path: file_path.to_owned(),
            obj_parse_error: false,
            obj_parse_error_message: String::new(),
            obj_curr_metadata_name: String::new(),
            obj_curr_characters: Vec::new(),
            // No initializer in the C++ (the module's header).
            object_unit_factor: 1.0,
            object_current_color_group: -1,
            object_group_id_to_color: BTreeMap::new(),
            is_bbl_3mf: false,
        }
    }

    /// `_stop_object_xml_parser(msg)`.
    fn stop_object_xml_parser(&mut self, ctl: &mut Ctl, msg: &str) {
        self.obj_parse_error = true;
        self.obj_parse_error_message = msg.to_owned();
        ctl.stop_parser(false);
    }

    /// `object_parse_error_message()`.
    fn object_parse_error_message(&self, parser: &Parser) -> String {
        if self.obj_parse_error {
            // The error was signalled by the user code, not the expat parser.
            if self.obj_parse_error_message.is_empty() {
                "Invalid 3MF format".to_owned()
            } else {
                self.obj_parse_error_message.clone()
            }
        } else {
            // The error was signalled by the expat parser.
            c_str_or_null(error_string(parser.error_code().code())).to_owned()
        }
    }

    /// `extract_object_model()`: the archive opened again and the file parsed. The entry's
    /// lookup and the handler around the extraction are `top_importer->_extract_from_archive`'s.
    fn extract_object_model(&mut self, top: &TopImporter) -> bool {
        let mut archive = ZipArchive::default();

        if !open_zip_reader(&mut archive, Path::new(&self.zip_path)) {
            top.add_error(&format!("Unable to open the zipfile {}", self.zip_path));
            return false;
        }

        let extracted = match locate_entry(&mut archive, &self.object_path) {
            None => {
                top.add_error(&snprintf_1024(format!(
                    "File {} not found from archive",
                    self.object_path
                )));
                false
            }
            Some(stat) => match self.extract_object_from_archive(top, &mut archive, &stat) {
                Ok(result) => result,
                Err(what) => {
                    // _extract_from_archive's handler
                    top.add_error(&what);
                    false
                }
            },
        };
        if !extracted {
            let error_msg = format!(
                "Archive does not contain a valid model for {}",
                self.object_path
            );
            top.add_error(&error_msg);

            close_zip_reader(&mut archive);
            return false;
        }

        close_zip_reader(&mut archive);

        if self.obj_parse_error {
            //already add_error inside
            //top_importer->add_error(object_parse_error_message());
            log(
                Severity::Error,
                &format!(
                    "{EXTRACT_OBJECT_MODEL}:1018, Found error while extrace object {}\n",
                    self.object_path
                ),
            );
            return false;
        }
        true
    }

    /// `_extract_object_from_archive(archive, stat)`: the file parsed chunk by chunk in the
    /// extraction callback. A parse error is the callback's `FileIOError`, which no handler here
    /// catches (`/EHsc`): it comes back as `Err` for `_extract_from_archive`.
    fn extract_object_from_archive(
        &mut self,
        top: &TopImporter,
        archive: &mut ZipArchive,
        stat: &FileStat,
    ) -> Result<bool, String> {
        if stat.m_uncomp_size == 0 {
            top.add_error(&format!("Found invalid size for {}", self.object_path));
            return Ok(false);
        }

        // XML_ParserCreate fails only when it cannot allocate ("Unable to create parser for
        // <path>"); the entity handlers are expat's defaults (null).
        let mut parser = Parser::new();
        let filename = String::from_utf8_lossy(&stat.m_filename).into_owned();
        let uncomp_size = stat.m_uncomp_size;
        let res = {
            let mut callback = |file_ofs: u64, buf: &[u8]| -> Result<usize, String> {
                let n = buf.len();
                let status = {
                    let mut h = ObjectXml {
                        importer: &mut *self,
                        top,
                    };
                    parser.parse(&mut h, buf, file_ofs + n as u64 == uncomp_size)
                };
                if status == Status::Error || self.obj_parse_error {
                    let message = self.object_parse_error_message(&parser);
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
            top.add_error(&format!(
                "Error while extracting model data from zip archive for {}",
                self.object_path
            ));
            return Ok(false);
        }

        Ok(true)
    }

    /// `_handle_object_start_model_xml_element(name, attributes)`.
    fn handle_object_start_model_xml_element(
        &mut self,
        ctl: &mut Ctl,
        name: &[u8],
        attributes: &[&[u8]],
    ) {
        let num_attributes = ctl.specified_attribute_count() as u32;
        let a = (attributes, num_attributes);
        let res = if name == MODEL_TAG.as_bytes() {
            self.handle_object_start_model(a)
        } else if name == RESOURCES_TAG.as_bytes() {
            // do nothing
            true
        } else if name == OBJECT_TAG.as_bytes() {
            self.handle_object_start_object(a)
        } else if name == COLOR_GROUP_TAG.as_bytes() {
            self.handle_object_start_color_group(a)
        } else if name == COLOR_TAG.as_bytes() {
            self.handle_object_start_color(a)
        } else if name == MESH_TAG.as_bytes() {
            self.handle_object_start_mesh()
        } else if name == VERTICES_TAG.as_bytes() {
            self.handle_object_start_vertices()
        } else if name == VERTEX_TAG.as_bytes() {
            self.handle_object_start_vertex(a)
        } else if name == TRIANGLES_TAG.as_bytes() {
            self.handle_object_start_triangles()
        } else if name == TRIANGLE_TAG.as_bytes() {
            self.handle_object_start_triangle(a)
        } else if name == COMPONENTS_TAG.as_bytes() {
            self.handle_object_start_components()
        } else if name == COMPONENT_TAG.as_bytes() {
            self.handle_object_start_component(a)
        } else if name == METADATA_TAG.as_bytes() {
            self.handle_object_start_metadata(a)
        } else {
            true
        };
        if !res {
            self.stop_object_xml_parser(ctl, "");
        }
    }

    /// `_handle_object_end_model_xml_element(name)`: only the object's and the metadata's ends
    /// do something; the rest answer true.
    fn handle_object_end_model_xml_element(
        &mut self,
        ctl: &mut Ctl,
        top: &TopImporter,
        name: &[u8],
    ) {
        let res = if name == OBJECT_TAG.as_bytes() {
            self.handle_object_end_object(top)
        } else if name == METADATA_TAG.as_bytes() {
            self.handle_object_end_metadata()
        } else {
            // MODEL, RESOURCES, COLOR_GROUP, COLOR, MESH, VERTICES, VERTEX, TRIANGLES, TRIANGLE,
            // COMPONENTS and COMPONENT: do nothing
            true
        };
        if !res {
            self.stop_object_xml_parser(ctl, "");
        }
    }

    /// `_handle_object_xml_characters(s, len)`.
    fn handle_object_xml_characters(&mut self, s: &[u8]) {
        self.obj_curr_characters.extend_from_slice(s);
    }

    /// `_handle_object_start_model(attributes, num_attributes)`: the file's own unit.
    fn handle_object_start_model(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        self.object_unit_factor = get_unit_factor(&get_attribute_value_string(atts, n, UNIT_ATTR));

        true
    }

    /// `_handle_object_start_object(attributes, num_attributes)`.
    fn handle_object_start_object(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        // reset current object data
        self.current_object = None;

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
            self.current_object = Some(o);
        }

        true
    }

    /// `_handle_object_end_object()`: the object kept by the file's path and its id. A BBL
    /// object whose UUID ends in `OBJECT_UUID_SUFFIX` is renumbered here only under `Restore`
    /// (not translated).
    fn handle_object_end_object(&mut self, top: &TopImporter) -> bool {
        let Some(current_object) = self.current_object.take().filter(|o| o.id != -1) else {
            top.add_error(&format!("Found invalid object for {}", self.object_path));
            return false;
        };
        let id: Id = (self.object_path.clone(), current_object.id);
        if let std::collections::btree_map::Entry::Vacant(e) = self.object_list.entry(id) {
            e.insert(current_object);
        } else {
            top.add_error(&format!(
                "Found object with duplicate id for {}",
                self.object_path
            ));
            return false;
        }

        true
    }

    /// `_handle_object_start_color_group(attributes, num_attributes)`.
    fn handle_object_start_color_group(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        self.object_current_color_group = get_attribute_value_int(atts, n, ID_ATTR);
        true
    }

    /// `_handle_object_start_color(attributes, num_attributes)`.
    fn handle_object_start_color(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        let color = get_attribute_value_string(atts, n, COLOR_ATTR);
        self.object_group_id_to_color
            .insert(self.object_current_color_group, color);
        true
    }

    /// `_handle_object_start_mesh(...)`: reset current geometry.
    fn handle_object_start_mesh(&mut self) -> bool {
        if let Some(o) = self.current_object.as_mut() {
            o.geometry.reset();
        }
        true
    }

    /// `_handle_object_start_vertices(...)`: reset current vertices.
    fn handle_object_start_vertices(&mut self) -> bool {
        if let Some(o) = self.current_object.as_mut() {
            o.geometry.vertices.clear();
        }
        true
    }

    /// `_handle_object_start_vertex(attributes, num_attributes)`: the vertex in millimetres
    /// (`float`), a missing coordinate 0.
    fn handle_object_start_vertex(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        // appends the vertex coordinates
        // missing values are set equal to ZERO
        let f = self.object_unit_factor;
        if let Some(o) = self.current_object.as_mut() {
            o.geometry.vertices.push(Vec3f::new(
                f * get_attribute_value_float(atts, n, X_ATTR),
                f * get_attribute_value_float(atts, n, Y_ATTR),
                f * get_attribute_value_float(atts, n, Z_ATTR),
            ));
        }
        true
    }

    /// `_handle_object_start_triangles(...)`: reset current triangles.
    fn handle_object_start_triangles(&mut self) -> bool {
        if let Some(o) = self.current_object.as_mut() {
            o.geometry.triangles.clear();
        }
        true
    }

    /// `_handle_object_start_triangle(attributes, num_attributes)`: the triangle's vertex
    /// indices (a missing one 0) and its painting strings.
    fn handle_object_start_triangle(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        // we are ignoring the following attributes:
        // p1
        // p2
        // p3
        // pid
        // see specifications

        // appends the triangle's vertices indices
        // missing values are set equal to ZERO
        if let Some(o) = self.current_object.as_mut() {
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

    /// `_handle_object_start_components(...)`: reset current components.
    fn handle_object_start_components(&mut self) -> bool {
        if let Some(o) = self.current_object.as_mut() {
            o.components.clear();
        }
        true
    }

    /// `_handle_object_start_component(attributes, num_attributes)`: the component's object, in
    /// this file, by its id, and its transform.
    fn handle_object_start_component(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        let object_id = get_attribute_value_int(atts, n, OBJECTID_ATTR);
        let transform = get_transform_from_3mf_specs_string(&get_attribute_value_string(
            atts,
            n,
            TRANSFORM_ATTR,
        ));

        if let Some(o) = self.current_object.as_mut() {
            let id: Id = (self.object_path.clone(), object_id);
            o.components.push(Component {
                object_id: id,
                transform,
            });
        }

        true
    }

    /// `_handle_object_start_metadata(attributes, num_attributes)`.
    fn handle_object_start_metadata(&mut self, (atts, n): (&[&[u8]], u32)) -> bool {
        self.obj_curr_metadata_name.clear();

        let name = get_attribute_value_string(atts, n, NAME_ATTR);
        if !name.is_empty() {
            self.obj_curr_metadata_name = name;
        }

        true
    }

    /// `_handle_object_end_metadata()`.
    fn handle_object_end_metadata(&mut self) -> bool {
        if self.obj_curr_metadata_name == BBS_3MF_VERSION
            || self.obj_curr_metadata_name == BBS_3MF_VERSION1
        {
            self.is_bbl_3mf = true;
        }
        true
    }
}

impl Importer {
    /// Lines 1781-1816 of `_load_model_from_file`: an `ObjectImporter` for each sub-model path,
    /// the files read in a `tbb::parallel_for`, and their objects and colours merged in the
    /// importers' order. False (with the log line 1803 writes) when any file failed.
    pub(super) fn load_sub_models(&mut self, filename: &str) -> bool {
        let object_importers: Vec<Mutex<ObjectImporter>> = self
            .m_sub_model_paths
            .iter()
            .map(|path| Mutex::new(ObjectImporter::new(filename, path)))
            .collect();

        let object_load_result = Mutex::new(true);
        let top = TopImporter {
            errors: Mutex::new(std::mem::take(&mut self.m_errors)),
        };
        orca_tbb::parallel_for::parallel_for(
            BlockedRange::new(0usize, object_importers.len(), 1),
            Partitioner::Auto,
            |importer_range: BlockedRange<usize>| {
                // CNumericLocalesSetter: the parsers here read numbers in the C locale.
                let range = importer_range.begin()..importer_range.end();
                for importer in &object_importers[range] {
                    let result = lock(importer).extract_object_model(&top);
                    {
                        let mut l = lock(&object_load_result);
                        *l &= result;
                    }
                }
            },
        );
        self.m_errors = top
            .errors
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);

        if !object_load_result
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
        {
            log(
                Severity::Error,
                &format!("{LOAD_MODEL_FROM_FILE}:1803, loading sub-objects error\n"),
            );
            return false;
        }

        //merge these objects into one
        for obj_importer in object_importers {
            let obj_importer = obj_importer
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner);
            for (key, object) in obj_importer.object_list {
                self.m_current_objects.entry(key).or_insert(object);
            }
            for (group, color) in obj_importer.object_group_id_to_color {
                self.m_group_id_to_color.entry(group).or_insert(color);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atts<'a>(pairs: &[(&'a str, &'a str)]) -> Vec<&'a [u8]> {
        pairs
            .iter()
            .flat_map(|(k, v)| [k.as_bytes(), v.as_bytes()])
            .collect()
    }

    // Expected values read from bbs_3mf.cpp:5341-5611: an object keyed by the file's path as the
    // relationship gives it, components in the same file, vertices in the file's unit, a
    // duplicate id refused with the path, the version metadata.
    #[test]
    fn objects_in_a_sub_model_file() {
        let top = TopImporter {
            errors: Mutex::new(Vec::new()),
        };
        let mut im = ObjectImporter::new("a.3mf", "/3D/Objects/o.model");
        let model = atts(&[("unit", "centimeter")]);
        assert!(im.handle_object_start_model((&model, 2)));
        let object = atts(&[("id", "4"), ("name", "part"), ("pid", "2")]);
        assert!(im.handle_object_start_object((&object, 6)));
        assert!(im.handle_object_start_mesh());
        let v = atts(&[("x", "1"), ("y", "2")]);
        assert!(im.handle_object_start_vertex((&v, 4)));
        let c = atts(&[("objectid", "7")]);
        assert!(im.handle_object_start_component((&c, 2)));
        assert!(im.handle_object_end_object(&top));
        let key: Id = ("/3D/Objects/o.model".to_owned(), 4);
        let o = &im.object_list[&key];
        assert_eq!(o.name, "part");
        assert_eq!(o.pid, 2);
        assert_eq!(o.geometry.vertices, vec![Vec3f::new(10.0, 20.0, 0.0)]);
        assert_eq!(
            o.components[0].object_id,
            ("/3D/Objects/o.model".to_owned(), 7)
        );

        assert!(im.handle_object_start_object((&object, 6)));
        assert!(!im.handle_object_end_object(&top));
        assert!(!im.handle_object_end_object(&top));
        assert_eq!(
            *lock(&top.errors),
            vec![
                "Found object with duplicate id for /3D/Objects/o.model".to_owned(),
                "Found invalid object for /3D/Objects/o.model".to_owned(),
            ]
        );

        let m = atts(&[("name", "BambuStudio:3mfVersion")]);
        assert!(im.handle_object_start_metadata((&m, 2)));
        assert!(im.handle_object_end_metadata());
        assert!(im.is_bbl_3mf);
    }
}
