// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the assembly of _BBS_3MF_Importer::_load_model_from_file (lines
// 1998-2375), _generate_current_object_list (4867-4892) and _generate_volumes_new (4894-5129) of
// src/libslic3r/Format/bbs_3mf.cpp of OrcaSlicer v2.4.2, translated on 2026-11-11 (not
// release_PlateData_list: no plate list is kept). Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE). And ADMesh code:
// FaceProperty::from_string of deps_src/admesh/stl.h (189-216) of OrcaSlicer v2.4.2, translated
// on 2026-11-11. Copyright (C) 1995, 1996 Anthony D. Martin and (C) 2013, 2014 the ADMesh
// contributors, with changes by the PrusaSlicer and OrcaSlicer authors, under the GNU General
// Public License, version 2 or later (GPL-2.0-or-later; licenses/GPL-2.0.txt), taken here under
// version 3 (licenses/GPL-3.0.txt) as section 13 of the AGPL-3.0 allows.

//! The assembly into the model, after the archive is closed: a file another program wrote has
//! each object of several build items split into one object per item; the colour groups give
//! the objects' extruders; each object built by a build item gets its settings, layer heights,
//! height ranges and brim ears (by its 1-based index), its parts (`_generate_volumes_new`: one for
//! each mesh its components reach, with its painting, type, text and shape, metadata and
//! settings) and its cut information; a lone part's offset moves to the instances; the parts get
//! their source; the extruders are checked; the plates' instances get their loaded ids.
//!
//! The parts share meshes as the C++'s do: `m_shared_meshes` holds, by the `mesh_shared`
//! metadata's id or the sub-object's own, the first part made from a mesh, and a later part of
//! that id is made with [`crate::model::ModelObject::add_volume_with_shared_mesh`] (the C++ holds
//! a pointer to the part; here its object's and its own index, which nothing moves while the
//! assembly runs).
//!
//! `_generate_current_object_list` walks the components breadth first and never ends when they
//! refer back to an object they come from (a component of its own object, directly or not): here
//! such an object is refused, "found a cycle in the components of object <id>", and the load fails
//! (`parity/HAZARDS.md`, the 3MF table).
//!
//! The plate list (`plate_data_list`) is not kept: the command line's load releases it unread
//! (`cli/run.cpp`, `load_model`, through `release_PlateData_list`, which frees each plate; with
//! no list there is nothing to free). Each plate's index is checked as the C++ checks it, its
//! thumbnail names logged and its instances given their loaded ids. An index of 0 passes the
//! check, where the C++ writes `plate_data_list[-1]` (undefined); here nothing is written for it
//! and its instances get their ids like any plate's (`parity/HAZARDS.md`, the 3MF table). The
//! command line's load asks for no plate (`plate_id` 0), so no object is skipped or deleted for
//! one, and the configuration it passes is empty, so no `filament_settings_id` bounds the
//! extruders (the bound is `INT_MAX`). The progress callback is null.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use orca_config::OptionValue;
use orca_eigen::{Transform3d, Vec3d};
use orca_mesh::admesh::{FaceProperty, FaceType, IndexedTriangleSet};
use orca_mesh::log::{Severity, log};
use orca_mesh::triangle_mesh::TriangleMesh;

use super::config::{ObjectMetadata, VolumeMetadata};
use super::names::{
    MATRIX_KEY, MESH_SHARED_KEY, NAME_KEY, PART_TYPE_KEY, SOURCE_FILE_KEY, SOURCE_IN_INCHES,
    SOURCE_IN_METERS, SOURCE_OBJECT_ID_KEY, SOURCE_OFFSET_X_KEY, SOURCE_OFFSET_Y_KEY,
    SOURCE_OFFSET_Z_KEY, SOURCE_VOLUME_ID_KEY, VOLUME_TYPE_KEY,
};
use super::{Component, Id, Importer, LOAD_MODEL_FROM_FILE, ReadError};
use crate::model::{CutConnectorType, CutInfo, Model, ModelVolumeType};
use crate::transformation::{Transformation, transform3d_from_string};

/// The function `__FUNCTION__` names in `_generate_volumes_new`'s logs.
const GENERATE_VOLUMES_NEW: &str = "Slic3r::_BBS_3MF_Importer::_generate_volumes_new";

/// `std::numeric_limits<int>::max()`: the extruders' bound when the configuration has no
/// `filament_settings_id`.
const MAX_FILAMENT_ID: i32 = i32::MAX;

/// A part's address as `operator<<` prints a pointer (`%p`: 16 upper-case hexadecimal digits).
fn volume_address(model: &Model, (object, volume): (usize, usize)) -> String {
    let address = std::ptr::from_ref(&model.objects[object].volumes[volume]) as usize;
    format!("{address:016X}")
}

/// `FaceProperty::from_string(str)` (admesh `stl.h:189-216`): a type of 1 to 3 (`atoi`) and the
/// area after the first space (`atof`, 0 when there is none); `eNormal` and 0 for anything else.
fn face_property_from_string(s: &str) -> FaceProperty {
    let bytes = s.as_bytes();
    if !bytes.is_empty() {
        let type_ = match orca_fmt::parse::atoi(bytes) {
            1 => Some(FaceType::SmallOverhang),
            2 => Some(FaceType::SmallHole),
            3 => Some(FaceType::ExteriorAppearance),
            // type <= eNormal || type >= eMaxNumFaceTypes
            _ => None,
        };
        if let Some(type_) = type_ {
            let area = match bytes.iter().position(|&c| c == b' ') {
                None => 0.0,
                Some(type_end_pos) => {
                    let area_str = &bytes[type_end_pos + 1..];
                    if area_str.is_empty() {
                        0.0
                    } else {
                        orca_fmt::parse::atof(area_str)
                    }
                }
            };
            return FaceProperty { type_, area };
        }
    }
    FaceProperty {
        type_: FaceType::Normal,
        area: 0.0,
    }
}

impl Importer {
    /// Lines 1998-2375 of `_load_model_from_file`, after the archive's closing: the model
    /// assembled from what the root model, the sub-models and the config entries gave.
    ///
    /// # Errors
    ///
    /// [`ReadError::Message`] for `set_deserialize`'s exception (an object's or a part's
    /// metadata that its option does not read), which escapes the importer.
    pub(super) fn assemble(
        &mut self,
        model: &mut Model,
        filename: &str,
    ) -> Result<bool, ReadError> {
        // m_objects_metadata is read here only, and no line after the assembly reads it.
        let objects_metadata = std::mem::take(&mut self.m_objects_metadata);
        let objects: Vec<(Id, i32)> = self
            .m_objects
            .iter()
            .map(|(id, &index)| (id.clone(), index))
            .collect();

        if !self.m_is_bbl_3mf {
            // if the 3mf was not produced by OrcaSlicer and there is more than one instance,
            // split the object in as many objects as instances
            log(
                Severity::Info,
                &format!(
                    "{LOAD_MODEL_FROM_FILE}:2001, found 3mf from other vendor, split as instance"
                ),
            );
            for (first, second) in &objects {
                if *second >= model.objects.len() as i32 {
                    self.add_error(&format!("3rd 3mf, invalid object, id: {}", first.1));
                    return Ok(false);
                }
                let model_object = *second as usize;
                if model.objects[model_object].instances.len() > 1 {
                    if !self.m_current_objects.contains_key(first) {
                        self.add_error(&format!("3rd 3mf, can not find object, id {}", first.1));
                        return Ok(false);
                    }
                    let Some(object_id_list) = self.generate_current_object_list(first) else {
                        self.add_error(&format!(
                            "found a cycle in the components of object {}",
                            first.1
                        ));
                        return Ok(false);
                    };

                    // select as volumes
                    let volumes: Vec<VolumeMetadata> = object_id_list
                        .iter()
                        .map(|c| {
                            VolumeMetadata::with_subobject(
                                c.object_id.1,
                                ModelVolumeType::ModelPart,
                            )
                        })
                        .collect();

                    // for each instance after the 1st, create a new model object containing only
                    // that instance and copy into it the geometry
                    while model.objects[model_object].instances.len() > 1 {
                        let other = model.objects[model_object].clone();
                        let new_model_object = model.add_object_copy(&other);
                        model.objects[new_model_object].clear_instances();
                        let last = other.instances[other.instances.len() - 1].clone();
                        model.objects[new_model_object].add_instance_copy(&last);
                        model.objects[model_object].delete_last_instance();
                        if !self.generate_volumes_new(
                            model,
                            new_model_object,
                            &object_id_list,
                            &volumes,
                        )? {
                            return Ok(false);
                        }
                    }
                }
            }
        }

        log(
            Severity::Info,
            &format!(
                "{LOAD_MODEL_FROM_FILE}:2043, process group colors, size {}\n",
                self.m_group_id_to_color.len()
            ),
        );
        let mut color_group_id_to_extruder_id_map: BTreeMap<i32, i32> = BTreeMap::new();
        let mut color_to_extruder_id_map: BTreeMap<String, i32> = BTreeMap::new();
        let mut extruder_id = 0;
        for (group, color) in &self.m_group_id_to_color {
            if let Some(&id) = color_to_extruder_id_map.get(color) {
                color_group_id_to_extruder_id_map.insert(*group, id);
            } else {
                extruder_id += 1;
                color_to_extruder_id_map.insert(color.clone(), extruder_id);
                color_group_id_to_extruder_id_map.insert(*group, extruder_id);
            }
        }

        log(
            Severity::Info,
            &format!(
                "{LOAD_MODEL_FROM_FILE}:2058, begin to assemble objects, size {}\n",
                self.m_objects.len()
            ),
        );
        //only load objects in plate_id: the command line's load asks for none (plate_id 0).
        for (first, second) in &objects {
            if *second >= model.objects.len() as i32 {
                self.add_error(&format!("invalid object, id: {}", first.1));
                return Ok(false);
            }
            let model_object = *second as usize;

            // m_layer_heights_profiles are indexed by a 1 based model object index.
            if let Some(profile) = self.m_layer_heights_profiles.get_mut(&(second + 1)) {
                model.objects[model_object].layer_height_profile = std::mem::take(profile);
            }

            // m_layer_config_ranges are indexed by a 1 based model object index.
            if let Some(ranges) = self.m_layer_config_ranges.get_mut(&(second + 1)) {
                model.objects[model_object].layer_config_ranges = std::mem::take(ranges);
            }

            if let Some(points) = self.m_brim_ear_points.get_mut(&(second + 1)) {
                model.objects[model_object].brim_points = std::mem::take(points);
            }

            let Some(object_id_list) = self.generate_current_object_list(first) else {
                self.add_error(&format!(
                    "found a cycle in the components of object {}",
                    first.1
                ));
                return Ok(false);
            };

            let volumes: Vec<VolumeMetadata>;
            let volumes_ptr: &[VolumeMetadata] = if let Some(obj_metadata) =
                objects_metadata.get(&first.1)
            {
                // config data has been found, this model was saved using slic3r pe
                apply_object_metadata(model, model_object, obj_metadata)?;

                // select object's detected volumes
                &obj_metadata.volumes
            } else {
                // config data not found, this model was not saved using slic3r pe

                // add the entire geometry as the single volume to generate
                volumes = object_id_list
                    .iter()
                    .map(|c| {
                        VolumeMetadata::with_subobject(c.object_id.1, ModelVolumeType::ModelPart)
                    })
                    .collect();

                if let Some(current_object) = self.m_current_objects.get(first) {
                    let o = &mut model.objects[model_object];
                    // get name
                    if current_object.name.is_empty() {
                        o.name = format!("Object_{}", second + 1);
                    } else {
                        o.name.clone_from(&current_object.name);
                    }

                    // get color
                    if let Some(&id) = color_group_id_to_extruder_id_map.get(&current_object.pid) {
                        o.config.set_key_value("extruder", OptionValue::Int(id));
                    }
                }

                // select as volumes
                &volumes
            };

            if !self.generate_volumes_new(model, model_object, &object_id_list, volumes_ptr)? {
                return Ok(false);
            }

            // Apply cut information for object if any was loaded
            // m_cut_object_ids are indexed by a 1 based model object index.
            if let Some(cut_object_info) = self.m_cut_object_infos.get(&(second + 1)).cloned() {
                model.objects[model_object].cut_id = cut_object_info.id;
                let vol_cnt = model.objects[model_object].volumes.len() as i32;
                for connector in &cut_object_info.connectors {
                    if connector.volume_id < 0 || connector.volume_id >= vol_cnt {
                        self.add_error("Invalid connector is found");
                        continue;
                    }
                    model.objects[model_object].volumes[connector.volume_id as usize].cut_info =
                        CutInfo::connector(
                            CutConnectorType(connector.ty),
                            connector.r_tolerance,
                            connector.h_tolerance,
                            true,
                        );
                }
            }
        }

        // If instances contain a single volume, the volume offset should be 0,0,0
        // This equals to say that instance world position and volume world position should match
        // Correct all instances/volumes for which this does not hold
        for o in &mut model.objects {
            if o.volumes.len() == 1 {
                // Every object here has an instance: each was made by a build item, or by the
                // split with one.
                let Some(first_instance) = o.instances.first() else {
                    continue;
                };
                let first_inst_trafo = *first_instance.get_transformation();
                let v = &mut o.volumes[0];
                let world_vol_offset = first_inst_trafo.mul(v.get_transformation()).get_offset();
                let world_inst_offset = first_inst_trafo.get_offset();

                if !world_vol_offset.is_approx(&world_inst_offset, 1e-12) {
                    let vol_trafo = *v.get_transformation();
                    for i in &mut o.instances {
                        let inst_trafo = *i.get_transformation();
                        i.set_offset(&inst_trafo.mul(&vol_trafo).get_offset());
                    }
                    v.set_offset(&Vec3d::zero());
                }
            }
        }

        for (object_idx, o) in model.objects.iter_mut().enumerate() {
            for (volume_idx, v) in o.volumes.iter_mut().enumerate() {
                if v.source.input_file.is_empty() && v.volume_type() == ModelVolumeType::ModelPart {
                    v.source.input_file = filename.to_owned();
                    if v.source.volume_idx == -1 {
                        v.source.volume_idx = volume_idx as i32;
                    }
                    if v.source.object_idx == -1 {
                        v.source.object_idx = object_idx as i32;
                    }
                }
            }
        }

        // The configuration has no filament_settings_id (the module's header).
        let max_filament_id = MAX_FILAMENT_ID;
        for mo in &mut model.objects {
            let mut extruder_id = 0;
            if let Some(id) = mo.config.opt_int("extruder") {
                extruder_id = id;
            }

            if extruder_id == 0 || extruder_id > max_filament_id {
                mo.config.set_key_value("extruder", OptionValue::Int(1));
            }

            if mo.volumes.len() == 1 {
                mo.volumes[0].config.erase("extruder");
            } else {
                for mv in &mut mo.volumes {
                    let Some(vol_extruder) = mv.config.opt_int("extruder") else {
                        continue;
                    };

                    if vol_extruder == 0 {
                        mv.config.erase("extruder");
                    } else if vol_extruder > max_filament_id {
                        mv.config.set_key_value("extruder", OptionValue::Int(1));
                    }
                }
            }
        }

        //BBS progress point
        log(
            Severity::Info,
            &format!(
                "{LOAD_MODEL_FROM_FILE}:2258import 3mf IMPORT_STAGE_LOADING_PLATES, m_plater_data size {}, m_backup_path {}\n",
                self.m_plater_data.len(),
                self.m_backup_path
            ),
        );

        if !self.load_plates(model) {
            return Ok(false);
        }

        // plate_id is 0: no object is removed for a plate.

        //BBS progress point
        log(
            Severity::Info,
            &format!("{LOAD_MODEL_FROM_FILE}:2367import 3mf IMPORT_STAGE_FINISH\n"),
        );

        Ok(true)
    }

    /// Lines 2265-2349: each plate's index checked, its thumbnail names logged and its
    /// instances given their loaded ids (the arrange order). The plate list itself is not kept
    /// (the module's header).
    fn load_plates(&mut self, model: &mut Model) -> bool {
        let plates: Vec<(i32, usize)> = self
            .m_plater_data
            .iter()
            .map(|(&index, &plate)| (index, plate))
            .collect();
        let size = plates.len() as u64;
        for (index, plate) in plates {
            // `it->first > m_plater_data.size()`: the int compared as a size_t, so a negative
            // index is past any size.
            if index as u64 > size {
                self.add_error("invalid plate index");
                return false;
            }
            // An index of 0 passes, where the C++ writes plate_data_list[-1]: nothing is written.
            let p = &self.m_plates[plate];
            let in_backup = |file: &str| {
                if self.m_load_restore || file.is_empty() {
                    file.to_owned()
                } else {
                    format!("{}/{file}", self.m_backup_path)
                }
            };
            let thumbnail_file = in_backup(&p.thumbnail_file);
            let no_light_thumbnail_file = in_backup(&p.no_light_thumbnail_file);
            let top_file = in_backup(&p.top_file);
            let pick_file = in_backup(&p.pick_file);

            log(
                Severity::Info,
                &format!(
                    "{LOAD_MODEL_FROM_FILE}:2309, plate {index}, thumbnail_file={thumbnail_file}, no_light_thumbnail_file={no_light_thumbnail_file}"
                ),
            );
            log(
                Severity::Info,
                &format!(
                    "{LOAD_MODEL_FROM_FILE}:2310, top_thumbnail_file={top_file}, pick_thumbnail_file={pick_file}"
                ),
            );

            //update the arrange order
            for (&obj_id, &(inst_index, loaded_id)) in &p.obj_inst_map {
                let Some(path) = self.m_index_paths.get(&obj_id) else {
                    log(
                        Severity::Warning,
                        &format!(
                            "{LOAD_MODEL_FROM_FILE}:2319, can not find object from plate's obj_map, id={obj_id}, skip this object"
                        ),
                    );
                    continue;
                };
                let temp_id: Id = (path.clone(), obj_id);
                let Some(&obj_index) = self.m_objects.get(&temp_id) else {
                    log(
                        Severity::Warning,
                        &format!(
                            "{LOAD_MODEL_FROM_FILE}:2327, can not find object from plate's obj_map, ID <{path}, {obj_id}>, skip this object"
                        ),
                    );
                    continue;
                };

                // Both compared as size_t.
                if obj_index as u64 >= model.objects.len() as u64 {
                    log(
                        Severity::Warning,
                        &format!("{LOAD_MODEL_FROM_FILE}:2335invalid object id {obj_index}\n"),
                    );
                    continue;
                }
                let obj = &mut model.objects[obj_index as usize];
                if inst_index as u64 >= obj.instances.len() as u64 {
                    log(
                        Severity::Warning,
                        &format!("{LOAD_MODEL_FROM_FILE}:2341invalid instance id {inst_index}\n"),
                    );
                    continue;
                }
                // An int stored in a size_t.
                obj.instances[inst_index as usize].loaded_id = loaded_id as usize;
            }
        }
        true
    }

    /// `_generate_current_object_list(sub_objects, object_id, current_objects)`: the objects
    /// with a mesh the object's components reach, breadth first, each with the product of the
    /// transforms on its way. `None` when the components reach a cycle, where the C++ never
    /// ends (the module's header).
    fn generate_current_object_list(&self, object_id: &Id) -> Option<Vec<Component>> {
        if self.components_reach_a_cycle(object_id) {
            return None;
        }
        let mut sub_objects = Vec::new();
        let mut id_list: VecDeque<(Component, Transform3d)> = VecDeque::new();
        id_list.push_back((
            Component {
                object_id: object_id.clone(),
                transform: Transform3d::identity(),
            },
            Transform3d::identity(),
        ));

        while let Some(current_item) = id_list.pop_front() {
            let current_id = &current_item.0;
            if let Some((key, current_object)) =
                self.m_current_objects.get_key_value(&current_id.object_id)
            {
                //found one
                if !current_object.components.is_empty() {
                    for comp in &current_object.components {
                        id_list.push_back((comp.clone(), current_item.1.mul(&comp.transform)));
                    }
                } else if !current_object.geometry.empty() {
                    sub_objects.push(Component {
                        object_id: key.clone(),
                        transform: current_item.1,
                    });
                }
            }
        }
        Some(sub_objects)
    }

    /// Whether the components an object reaches (through the objects found that have
    /// components, as `_generate_current_object_list` follows them) lead back to an object on
    /// their own way: a depth-first walk with the objects on the way and those finished.
    fn components_reach_a_cycle(&self, root: &Id) -> bool {
        let Some((root, _)) = self.m_current_objects.get_key_value(root) else {
            return false;
        };
        let components = |id: &Id| -> &[Component] {
            self.m_current_objects
                .get(id)
                .map_or(&[][..], |o| &o.components[..])
        };
        let mut on_the_way: BTreeSet<&Id> = BTreeSet::new();
        let mut finished: BTreeSet<&Id> = BTreeSet::new();
        let mut stack: Vec<(&Id, usize)> = vec![(root, 0)];
        on_the_way.insert(root);
        while let Some((id, next)) = stack.last_mut() {
            let comps = components(id);
            if *next < comps.len() {
                let child = &comps[*next].object_id;
                *next += 1;
                if on_the_way.contains(child) {
                    return true;
                }
                if finished.contains(child) {
                    continue;
                }
                on_the_way.insert(child);
                stack.push((child, 0));
            } else {
                let id = *id;
                on_the_way.remove(id);
                finished.insert(id);
                stack.pop();
            }
        }
        false
    }

    /// `_generate_volumes_new(object, sub_objects, volumes, config_substitutions)`: a part for
    /// each sub-object, made from its mesh or sharing one made before, placed by its component's
    /// transform, painted, and given its metadata.
    ///
    /// # Errors
    ///
    /// [`ReadError::Message`] for `set_deserialize`'s exception.
    fn generate_volumes_new(
        &mut self,
        model: &mut Model,
        object: usize,
        sub_objects: &[Component],
        volumes: &[VolumeMetadata],
    ) -> Result<bool, ReadError> {
        if !model.objects[object].volumes.is_empty() {
            self.add_error("object already built with parts");
            return Ok(false);
        }

        //unsigned int geo_tri_count = (unsigned int)geometry.triangles.size();
        let mut renamed_volumes_count: u32 = 0;

        for (index, sub_comp) in sub_objects.iter().enumerate() {
            //find the volume metadata firstly
            let object_id = &sub_comp.object_id;
            let Some(sub_object) = self.m_current_objects.get(object_id) else {
                self.add_error(&format!("sub_objects can not be found, id={}", object_id.1));
                return Ok(false);
            };

            let default_volume_data =
                VolumeMetadata::with_subobject(sub_object.id, ModelVolumeType::ModelPart);
            let mut volume_data: Option<&VolumeMetadata> = None;
            if index < volumes.len() && volumes[index].subobject_id == sub_object.id {
                volume_data = Some(&volumes[index]);
            } else {
                for volume_iter in volumes {
                    if volume_iter.subobject_id == sub_object.id {
                        volume_data = Some(volume_iter);
                        break;
                    }
                }
            }

            let mut volume_matrix_to_object = Transform3d::identity();
            let mut has_transform = false;
            let mut shared_mesh_id = object_id.1;
            if let Some(vd) = volume_data {
                let mut found_count = 0;
                // extract the volume transformation from the volume's metadata, if present
                for metadata in &vd.metadata {
                    if metadata.key == MATRIX_KEY {
                        volume_matrix_to_object = transform3d_from_string(&metadata.value);
                        has_transform =
                            !volume_matrix_to_object.is_approx(&Transform3d::identity(), 1e-10);
                        found_count += 1;
                    } else if metadata.key == MESH_SHARED_KEY {
                        //add the shared mesh logic
                        shared_mesh_id = orca_fmt::parse::atoi(metadata.value.as_bytes());
                        found_count += 1;
                        log(
                            Severity::Info,
                            &format!(
                                "{GENERATE_VOLUMES_NEW}: line 4944, shared_mesh_id {shared_mesh_id}"
                            ),
                        );
                    }

                    if found_count >= 2 {
                        break;
                    }
                }
            } else {
                //create a volume_data
                volume_data = Some(&default_volume_data);
            }
            let volume_data = volume_data.unwrap_or(&default_volume_data);

            let mut shared_volume: Option<(usize, usize)> = None;
            log(
                Severity::Info,
                &format!(
                    "{GENERATE_VOLUMES_NEW}: line 4958, subobject_id {}, shared_mesh_id {shared_mesh_id}",
                    sub_object.id
                ),
            );
            if shared_mesh_id != -1 {
                if let Some(&found) = self.m_shared_meshes.get(&shared_mesh_id) {
                    shared_volume = Some(found);
                    log(
                        Severity::Info,
                        &format!(
                            "{GENERATE_VOLUMES_NEW}: line 4963, found shared mesh, id {shared_mesh_id}, mesh {}",
                            volume_address(model, found)
                        ),
                    );
                }
            } else {
                //for some cases, object point to this shared mesh already loaded, treat that one as the root
                if let Some(&found) = self.m_shared_meshes.get(&sub_object.id) {
                    shared_volume = Some(found);
                    log(
                        Severity::Info,
                        &format!(
                            "{GENERATE_VOLUMES_NEW}: line 4971, already loaded copy-share mesh before, id {}, mesh {}",
                            sub_object.id,
                            volume_address(model, found)
                        ),
                    );
                }
            }

            let triangles_count = sub_object.geometry.triangles.len();
            if triangles_count == 0 {
                let error = format!("found no trianges in the object {}", sub_object.id);
                self.m_errors.push(error);
                return Ok(false);
            }
            let volume = if let Some(shared) = shared_volume {
                //create volume to use shared mesh
                let mesh = model.objects[shared.0].volumes[shared.1].mesh_ptr();
                let v = model.objects[object]
                    .add_volume_with_shared_mesh(&mesh, ModelVolumeType::ModelPart);
                log(
                    Severity::Info,
                    &format!(
                        "{GENERATE_VOLUMES_NEW}: line 5035, create volume using shared_mesh {}",
                        volume_address(model, shared)
                    ),
                );
                v
            } else {
                // splits volume out of imported geometry
                let mut its = IndexedTriangleSet::default();
                its.indices.clone_from(&sub_object.geometry.triangles);
                let vertices_count = sub_object.geometry.vertices.len() as i32;
                for face in &its.indices {
                    for &tri_id in &face.0 {
                        if tri_id < 0 || tri_id >= vertices_count {
                            let error = format!("invalid vertex id in object {}", sub_object.id);
                            self.m_errors.push(error);
                            return Ok(false);
                        }
                    }
                }

                its.vertices.clone_from(&sub_object.geometry.vertices);

                // BBS
                for prop_str in &sub_object.geometry.face_properties {
                    its.properties.push(face_property_from_string(prop_str));
                }

                let mut triangle_mesh =
                    TriangleMesh::from_its_with_errors(its, volume_data.mesh_stats);

                // BBS: no need to multiply the instance matrix into the volume
                if triangle_mesh.volume() < 0.0 {
                    triangle_mesh.flip_triangles();
                }

                let v = model.objects[object].add_volume(
                    triangle_mesh,
                    ModelVolumeType::ModelPart,
                    true,
                );

                if shared_mesh_id != -1 {
                    //for some cases the shared mesh is in other plate and not loaded in cli slicing
                    //we need to use the first one in the same plate instead
                    self.m_shared_meshes.insert(shared_mesh_id, (object, v));
                } else {
                    self.m_shared_meshes.insert(sub_object.id, (object, v));
                }
                v
            };

            let o = &mut model.objects[object];
            let vol = &mut o.volumes[volume];
            // stores the volume matrix taken from the metadata, if present
            if has_transform {
                vol.source.transform = Transformation::new(&volume_matrix_to_object);
            }

            vol.calculate_convex_hull();

            //set transform from 3mf
            let comp_transformatino = Transformation::new(&sub_comp.transform);
            let t = comp_transformatino.mul(vol.get_transformation());
            vol.set_transformation(t);
            if shared_volume.is_some() {
                let shift = vol.mesh().get_init_shift();
                if !shift.is_approx(&Vec3d::zero(), 1e-12) {
                    vol.translate(&shift);
                }
            }

            // recreate custom supports, seam and mmu segmentation from previously loaded attribute
            {
                let g = &sub_object.geometry;
                vol.supported_facets.reserve(triangles_count);
                vol.seam_facets.reserve(triangles_count);
                vol.mmu_segmentation_facets.reserve(triangles_count);
                vol.fuzzy_skin_facets.reserve(triangles_count);
                for i in 0..triangles_count {
                    // Each painting list holds an entry for every triangle the object's mesh
                    // was given since its last reset, so at least as many as the triangles.
                    let id = i as i32;
                    if !g.custom_supports[i].is_empty() {
                        vol.supported_facets
                            .set_triangle_from_string(id, g.custom_supports[i].as_bytes());
                    }
                    if !g.custom_seam[i].is_empty() {
                        vol.seam_facets
                            .set_triangle_from_string(id, g.custom_seam[i].as_bytes());
                    }
                    if !g.mmu_segmentation[i].is_empty() {
                        vol.mmu_segmentation_facets
                            .set_triangle_from_string(id, g.mmu_segmentation[i].as_bytes());
                    }
                    if !g.fuzzy_skin[i].is_empty() {
                        vol.fuzzy_skin_facets
                            .set_triangle_from_string(id, g.fuzzy_skin[i].as_bytes());
                    }
                }
                vol.supported_facets.shrink_to_fit();
                vol.seam_facets.shrink_to_fit();
                vol.mmu_segmentation_facets.shrink_to_fit();
                // touch(): a timestamp, not kept.
                vol.fuzzy_skin_facets.shrink_to_fit();
            }

            vol.set_type(volume_data.part_type);

            // A copy: volume_data is const, so std::move of its optional copies it.
            if let Some(es) = &volume_data.shape_configuration {
                vol.emboss_shape = Some(es.clone());
            }
            if let Some(tc) = &volume_data.text_configuration {
                vol.text_configuration = Some(tc.clone());
            }

            // apply the remaining volume's metadata
            for metadata in &volume_data.metadata {
                let (key, value) = (metadata.key.as_str(), metadata.value.as_str());
                if key == NAME_KEY {
                    vol.name = value.to_owned();
                } else if key == VOLUME_TYPE_KEY || key == PART_TYPE_KEY {
                    //for old format
                    vol.set_type(ModelVolumeType::from_string(value));
                } else if key == SOURCE_FILE_KEY {
                    vol.source.input_file = value.to_owned();
                } else if key == SOURCE_OBJECT_ID_KEY {
                    vol.source.object_idx = orca_fmt::parse::atoi(value.as_bytes());
                } else if key == SOURCE_VOLUME_ID_KEY {
                    vol.source.volume_idx = orca_fmt::parse::atoi(value.as_bytes());
                } else if key == SOURCE_OFFSET_X_KEY {
                    vol.source.mesh_offset[0] = orca_fmt::parse::atof(value.as_bytes());
                } else if key == SOURCE_OFFSET_Y_KEY {
                    vol.source.mesh_offset[1] = orca_fmt::parse::atof(value.as_bytes());
                } else if key == SOURCE_OFFSET_Z_KEY {
                    vol.source.mesh_offset[2] = orca_fmt::parse::atof(value.as_bytes());
                } else if key == SOURCE_IN_INCHES {
                    vol.source.is_converted_from_inches = value == "1";
                } else if key == SOURCE_IN_METERS {
                    vol.source.is_converted_from_meters = value == "1";
                } else if key == MATRIX_KEY || key == MESH_SHARED_KEY {
                    continue;
                } else {
                    vol.config
                        .set_deserialize(key, value)
                        .map_err(|e| ReadError::Message(e.what()))?;
                }
            }

            // this may happen for 3mf saved by 3rd part softwares
            if vol.name.is_empty() {
                vol.name.clone_from(&o.name);
                if renamed_volumes_count > 0 {
                    vol.name += &format!("_{}", renamed_volumes_count + 1);
                }
                renamed_volumes_count += 1;
            }
        }

        Ok(true)
    }
}

/// Lines 2124-2132: an object's metadata from `model_settings.config`, its name, module and
/// settings.
///
/// # Errors
///
/// [`ReadError::Message`] for `set_deserialize`'s exception.
fn apply_object_metadata(
    model: &mut Model,
    model_object: usize,
    obj_metadata: &ObjectMetadata,
) -> Result<(), ReadError> {
    let o = &mut model.objects[model_object];
    // apply object's name and config data
    for metadata in &obj_metadata.metadata {
        if metadata.key == "name" {
            o.name.clone_from(&metadata.value);
        //BBS: add module name
        } else if metadata.key == "module" {
            o.module_name.clone_from(&metadata.value);
        } else {
            o.config
                .set_deserialize(&metadata.key, &metadata.value)
                .map_err(|e| ReadError::Message(e.what()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bbs_3mf::CurrentObject;
    use crate::bbs_3mf::config::PlateData;

    // Expected values read from admesh stl.h:189-216: a type of 1 to 3 with the area after the
    // first space, anything else eNormal and 0.
    #[test]
    fn face_properties() {
        let p = face_property_from_string("2 0.5");
        assert_eq!((p.type_, p.area), (FaceType::SmallHole, 0.5));
        let p = face_property_from_string("3");
        assert_eq!((p.type_, p.area), (FaceType::ExteriorAppearance, 0.0));
        let p = face_property_from_string("1 ");
        assert_eq!((p.type_, p.area), (FaceType::SmallOverhang, 0.0));
        for s in ["", "0 3", "4 1", "-1 2", "x"] {
            let p = face_property_from_string(s);
            assert_eq!((p.type_, p.area), (FaceType::Normal, 0.0), "{s:?}");
        }
    }

    fn object_with(components: &[i32], mesh: bool) -> CurrentObject {
        let mut o = CurrentObject::new();
        o.components = components
            .iter()
            .map(|&id| Component {
                object_id: (String::new(), id),
                transform: Transform3d::identity(),
            })
            .collect();
        if mesh {
            o.geometry
                .vertices
                .push(orca_eigen::Vec3f::new(0.0, 0.0, 0.0));
            o.geometry.triangles.push(orca_eigen::Vec3i32::new(0, 0, 0));
        }
        o
    }

    // Expected values read from bbs_3mf.cpp:4867-4892: the meshes reached breadth first, an
    // object without components or a mesh left out; a cycle (refused here, `parity/HAZARDS.md`).
    #[test]
    fn components_breadth_first_and_cycles() {
        let mut im = Importer::new();
        im.m_current_objects
            .insert((String::new(), 1), object_with(&[2, 3, 9], false));
        im.m_current_objects
            .insert((String::new(), 2), object_with(&[4], false));
        im.m_current_objects
            .insert((String::new(), 3), object_with(&[], true));
        im.m_current_objects
            .insert((String::new(), 4), object_with(&[], true));
        let list = im
            .generate_current_object_list(&(String::new(), 1))
            .expect("no cycle");
        let ids: Vec<i32> = list.iter().map(|c| c.object_id.1).collect();
        assert_eq!(ids, vec![3, 4]);

        im.m_current_objects
            .insert((String::new(), 4), object_with(&[2], false));
        assert!(
            im.generate_current_object_list(&(String::new(), 1))
                .is_none()
        );
        im.m_current_objects
            .insert((String::new(), 5), object_with(&[5], false));
        assert!(
            im.generate_current_object_list(&(String::new(), 5))
                .is_none()
        );
        assert!(
            im.generate_current_object_list(&(String::new(), 3))
                .is_some()
        );
    }

    /// An importer whose object 1 (model object 0, with `instances` instances) reaches a cycle
    /// through object 2.
    fn cyclic(bbl: bool, instances: usize) -> (Importer, Model) {
        let mut im = Importer::new();
        im.m_is_bbl_3mf = bbl;
        im.m_current_objects
            .insert((String::new(), 1), object_with(&[2], false));
        im.m_current_objects
            .insert((String::new(), 2), object_with(&[1], false));
        im.m_objects.insert((String::new(), 1), 0);
        let mut model = Model::default();
        let o = model.add_empty_object();
        for _ in 0..instances {
            model.objects[o].add_instance();
        }
        (im, model)
    }

    // A cycle never ends in the C++ (bbs_3mf.cpp:4867-4892); refused here with one error and a
    // false answer, at the split of another program's file (2001-2033) and at the assembly
    // (2058-2207) alike (`parity/HAZARDS.md`, the 3MF table).
    #[test]
    fn a_cycle_fails_the_assembly() {
        for (bbl, instances) in [(true, 1), (false, 1), (false, 2)] {
            let (mut im, mut model) = cyclic(bbl, instances);
            assert!(
                matches!(im.assemble(&mut model, "a.3mf"), Ok(false)),
                "bbl {bbl}, {instances} instances"
            );
            assert_eq!(
                im.m_errors,
                vec!["found a cycle in the components of object 1".to_owned()],
                "bbl {bbl}, {instances} instances"
            );
            assert!(model.objects[0].volumes.is_empty());
        }
    }

    // Expected values read from bbs_3mf.cpp:2266-2348: an index of 0 passes the check
    // (`it->first > m_plater_data.size()`); the C++ then writes plate_data_list[-1], which
    // nothing here writes, and its instances take their loaded ids. An index past the count is
    // "invalid plate index".
    #[test]
    fn plate_indexes() {
        let path = "/3D/3dmodel.model".to_owned();
        let mut im = Importer::new();
        im.m_index_paths.insert(1, path.clone());
        im.m_objects.insert((path, 1), 0);
        let mut plate = PlateData {
            plate_index: 0,
            ..PlateData::default()
        };
        plate.obj_inst_map.insert(1, (1, 7));
        im.m_plates.push(plate.clone());
        im.m_plater_data.insert(0, 0);
        let mut model = Model::default();
        let o = model.add_empty_object();
        model.objects[o].add_instance();
        model.objects[o].add_instance();

        assert!(im.load_plates(&mut model));
        assert!(im.m_errors.is_empty());
        assert_eq!(im.m_plates, vec![plate.clone()]);
        assert_eq!(im.m_plater_data.len(), 1);
        let ids: Vec<usize> = model.objects[0]
            .instances
            .iter()
            .map(|i| i.loaded_id)
            .collect();
        assert_eq!(ids, vec![0, 7]);

        im.m_plater_data.clear();
        im.m_plater_data.insert(2, 0);
        assert!(!im.load_plates(&mut model));
        assert_eq!(im.m_errors, vec!["invalid plate index".to_owned()]);
    }
}
