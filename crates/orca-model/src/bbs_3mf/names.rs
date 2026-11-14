// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: the name constants of src/libslic3r/Format/bbs_3mf.cpp (lines 143-454),
// of OrcaSlicer v2.4.2, translated on 2026-11-10. Copyright the OrcaSlicer authors, the
// BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa Research), under the GNU
// Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! The 3MF file's names: its entries, tags, attributes, metadata names and version numbers, one
//! constant for each of `bbs_3mf.cpp`'s, in its order and with its comments (those the C++ keeps
//! commented out stay comments). The reader and the writer share them; the writer is not
//! translated, so some are used by nothing yet.

// VERSION NUMBERS
// 0 : .3mf, files saved by older slic3r or other applications. No version definition in them.
// 1 : Introduction of 3mf versioning. No other change in data saved into 3mf files.
// 2 : Volumes' matrices and source data added to Metadata/Slic3r_PE_model.config file, meshes transformed back to their coordinate system on loading.
// WARNING !! -> the version number has been rolled back to 1
//               the next change should use 3
pub const VERSION_BBS_3MF: u32 = 1;
// Allow loading version 2 file as well.
pub const VERSION_BBS_3MF_COMPATIBLE: u32 = 2;
pub const BBS_3MF_VERSION1: &str = "bamboo_slicer:Version3mf"; // definition of the metadata name saved into .model file
pub const BBS_3MF_VERSION: &str = "BambuStudio:3mfVersion"; //compatible with prusa currently
// Painting gizmos data version numbers
// 0 : initial version of fdm, seam, mm
pub const FDM_SUPPORTS_PAINTING_VERSION: u32 = 0;
pub const SEAM_PAINTING_VERSION: u32 = 0;
pub const MM_PAINTING_VERSION: u32 = 0;

pub const BBS_FDM_SUPPORTS_PAINTING_VERSION: &str = "BambuStudio:FdmSupportsPaintingVersion";
pub const BBS_SEAM_PAINTING_VERSION: &str = "BambuStudio:SeamPaintingVersion";
pub const BBS_MM_PAINTING_VERSION: &str = "BambuStudio:MmPaintingVersion";
pub const BBL_MODEL_ID_TAG: &str = "model_id";
pub const BBL_MODEL_NAME_TAG: &str = "Title";
pub const BBL_ORIGIN_TAG: &str = "Origin";
pub const BBL_DESIGNER_TAG: &str = "Designer";
pub const BBL_DESIGNER_USER_ID_TAG: &str = "DesignerUserId";
pub const BBL_DESIGNER_COVER_FILE_TAG: &str = "DesignerCover";
pub const BBL_DESCRIPTION_TAG: &str = "Description";
pub const BBL_COPYRIGHT_TAG: &str = "CopyRight";
pub const BBL_COPYRIGHT_NORMATIVE_TAG: &str = "Copyright";
pub const BBL_LICENSE_TAG: &str = "License";
pub const BBL_REGION_TAG: &str = "Region";
pub const BBL_MODIFICATION_TAG: &str = "ModificationDate";
pub const BBL_CREATION_DATE_TAG: &str = "CreationDate";
// Orca: BBL current version
pub const BBL_APPLICATION_TAG: &str = "Application";
// OrcaSlicer version tag
pub const ORCASLICER_TAG: &str = "OrcaSlicer";
pub const BBL_MAKERLAB_TAG: &str = "MakerLab";
pub const BBL_MAKERLAB_VERSION_TAG: &str = "MakerLabVersion";

pub const BBL_PROFILE_TITLE_TAG: &str = "ProfileTitle";
pub const BBL_PROFILE_COVER_TAG: &str = "ProfileCover";
pub const BBL_PROFILE_DESCRIPTION_TAG: &str = "ProfileDescription";
pub const BBL_PROFILE_USER_ID_TAG: &str = "ProfileUserId";
pub const BBL_PROFILE_USER_NAME_TAG: &str = "ProfileUserName";

pub const MODEL_FOLDER: &str = "3D/";
pub const MODEL_EXTENSION: &str = ".model";
pub const MODEL_FILE: &str = "3D/3dmodel.model"; // << this is the only format of the string which works with CURA
pub const MODEL_RELS_FILE: &str = "3D/_rels/3dmodel.model.rels";
//BBS: add metadata_folder
pub const METADATA_DIR: &str = "Metadata/";
pub const ACCESOR_DIR: &str = "accesories/";
pub const GCODE_EXTENSION: &str = ".gcode";
pub const THUMBNAIL_EXTENSION: &str = ".png";
pub const CALIBRATION_INFO_EXTENSION: &str = ".json";
pub const CONTENT_TYPES_FILE: &str = "[Content_Types].xml";
pub const RELATIONSHIPS_FILE: &str = "_rels/.rels";
pub const THUMBNAIL_FILE: &str = "Metadata/plate_1.png";
pub const THUMBNAIL_FOR_PRINTER_FILE: &str = "Metadata/bbl_thumbnail.png";
pub const PRINTER_THUMBNAIL_SMALL_FILE: &str = "/Auxiliaries/.thumbnails/thumbnail_small.png";
pub const PRINTER_THUMBNAIL_MIDDLE_FILE: &str = "/Auxiliaries/.thumbnails/thumbnail_middle.png";
pub const _3MF_COVER_FILE: &str = "/Auxiliaries/.thumbnails/thumbnail_3mf.png";
//const std::string PRINT_CONFIG_FILE = "Metadata/Slic3r_PE.config";
//const std::string MODEL_CONFIG_FILE = "Metadata/Slic3r_PE_model.config";
pub const BBS_PRINT_CONFIG_FILE: &str = "Metadata/print_profile.config";
pub const BBS_PROJECT_CONFIG_FILE: &str = "Metadata/project_settings.config";
pub const BBS_MODEL_CONFIG_FILE: &str = "Metadata/model_settings.config";
pub const BBS_MODEL_CONFIG_RELS_FILE: &str = "Metadata/_rels/model_settings.config.rels";
pub const SLICE_INFO_CONFIG_FILE: &str = "Metadata/slice_info.config";
pub const FILAMENT_SEQUENCE_FILE: &str = "Metadata/filament_sequence.json";
pub const BBS_LAYER_HEIGHTS_PROFILE_FILE: &str = "Metadata/layer_heights_profile.txt";
pub const LAYER_CONFIG_RANGES_FILE: &str = "Metadata/layer_config_ranges.xml";
pub const BRIM_EAR_POINTS_FILE: &str = "Metadata/brim_ear_points.txt";
/*const std::string SLA_SUPPORT_POINTS_FILE = "Metadata/Slic3r_PE_sla_support_points.txt";
const std::string SLA_DRAIN_HOLES_FILE = "Metadata/Slic3r_PE_sla_drain_holes.txt";*/
pub const CUSTOM_GCODE_PER_PRINT_Z_FILE: &str = "Metadata/custom_gcode_per_layer.xml";
pub const AUXILIARY_DIR: &str = "Auxiliaries/";
pub const PROJECT_EMBEDDED_PRINT_PRESETS_FILE: &str = "Metadata/print_setting_";
pub const PROJECT_EMBEDDED_SLICE_PRESETS_FILE: &str = "Metadata/process_settings_";
pub const PROJECT_EMBEDDED_FILAMENT_PRESETS_FILE: &str = "Metadata/filament_settings_";
pub const PROJECT_EMBEDDED_PRINTER_PRESETS_FILE: &str = "Metadata/machine_settings_";
pub const CUT_INFORMATION_FILE: &str = "Metadata/cut_information.xml";

pub const AUXILIARY_STR_LEN: u32 = 12;
pub const METADATA_STR_LEN: u32 = 9;

pub const MODEL_TAG: &str = "model";
pub const RESOURCES_TAG: &str = "resources";
pub const COLOR_GROUP_TAG: &str = "m:colorgroup";
pub const COLOR_TAG: &str = "m:color";
pub const OBJECT_TAG: &str = "object";
pub const MESH_TAG: &str = "mesh";
pub const MESH_STAT_TAG: &str = "mesh_stat";
pub const VERTICES_TAG: &str = "vertices";
pub const VERTEX_TAG: &str = "vertex";
pub const TRIANGLES_TAG: &str = "triangles";
pub const TRIANGLE_TAG: &str = "triangle";
pub const COMPONENTS_TAG: &str = "components";
pub const COMPONENT_TAG: &str = "component";
pub const BUILD_TAG: &str = "build";
pub const ITEM_TAG: &str = "item";
pub const METADATA_TAG: &str = "metadata";
pub const FILAMENT_TAG: &str = "filament";
pub const SLICE_WARNING_TAG: &str = "warning";
pub const WARNING_MSG_TAG: &str = "msg";
pub const FILAMENT_ID_TAG: &str = "id";
pub const FILAMENT_TYPE_TAG: &str = "type";
pub const FILAMENT_COLOR_TAG: &str = "color";
pub const FILAMENT_USED_M_TAG: &str = "used_m";
pub const FILAMENT_USED_G_TAG: &str = "used_g";
pub const FILAMENT_USED_FOR_SUPPORT: &str = "used_for_support";
pub const FILAMENT_USED_FOR_OBJECT: &str = "used_for_object";
pub const FILAMENT_NOZZLE_GROUP_ID_TAG: &str = "group_id";
pub const FILAMENT_NOZZLE_DIAMETER_TAG: &str = "nozzle_diameter";
pub const FILAMENT_NOZZLE_VOLUME_TYPE_TAG: &str = "volume_type";
pub const NOZZLE_TAG: &str = "nozzle";
pub const FILAMENT_TRAY_INFO_ID_TAG: &str = "tray_info_idx";
pub const LAYER_FILAMENT_LISTS_TAG: &str = "layer_filament_lists";
pub const LAYER_FILAMENT_LIST_TAG: &str = "layer_filament_list";

pub const CONFIG_TAG: &str = "config";
pub const VOLUME_TAG: &str = "volume";
pub const PART_TAG: &str = "part";
pub const PLATE_TAG: &str = "plate";
pub const INSTANCE_TAG: &str = "model_instance";
//BBS
pub const ASSEMBLE_TAG: &str = "assemble";
pub const ASSEMBLE_ITEM_TAG: &str = "assemble_item";
pub const SLICE_HEADER_TAG: &str = "header";
pub const SLICE_HEADER_ITEM_TAG: &str = "header_item";

// Deprecated: text_info
pub const TEXT_INFO_TAG: &str = "text_info";
pub const TEXT_ATTR: &str = "text";
pub const FONT_NAME_ATTR: &str = "font_name";
pub const FONT_INDEX_ATTR: &str = "font_index";
pub const FONT_SIZE_ATTR: &str = "font_size";
pub const THICKNESS_ATTR: &str = "thickness";
pub const EMBEDED_DEPTH_ATTR: &str = "embeded_depth";
pub const ROTATE_ANGLE_ATTR: &str = "rotate_angle";
pub const TEXT_GAP_ATTR: &str = "text_gap";
pub const BOLD_ATTR: &str = "bold";
pub const ITALIC_ATTR: &str = "italic";
pub const SURFACE_TEXT_ATTR: &str = "surface_text";
pub const KEEP_HORIZONTAL_ATTR: &str = "keep_horizontal";
pub const HIT_MESH_ATTR: &str = "hit_mesh";
pub const HIT_POSITION_ATTR: &str = "hit_position";
pub const HIT_NORMAL_ATTR: &str = "hit_normal";

// BBS: encrypt
pub const RELATIONSHIP_TAG: &str = "Relationship";
pub const PID_ATTR: &str = "pid";
pub const PUUID_ATTR: &str = "p:UUID";
pub const PUUID_LOWER_ATTR: &str = "p:uuid";
pub const PPATH_ATTR: &str = "p:path";
pub const OBJECT_UUID_SUFFIX: &str = "-61cb-4c03-9d28-80fed5dfa1dc";
pub const OBJECT_UUID_SUFFIX2: &str = "-71cb-4c03-9d28-80fed5dfa1dc";
pub const SUB_OBJECT_UUID_SUFFIX: &str = "-81cb-4c03-9d28-80fed5dfa1dc";
pub const COMPONENT_UUID_SUFFIX: &str = "-b206-40ff-9872-83e8017abed1";
pub const BUILD_UUID: &str = "2c7c17d8-22b5-4d84-8835-1976022ea369";
pub const BUILD_UUID_SUFFIX: &str = "-b1ec-4553-aec9-835e5b724bb4";
pub const TARGET_ATTR: &str = "Target";
pub const RELS_TYPE_ATTR: &str = "Type";

pub const UNIT_ATTR: &str = "unit";
pub const NAME_ATTR: &str = "name";
pub const COLOR_ATTR: &str = "color";
pub const TYPE_ATTR: &str = "type";
pub const ID_ATTR: &str = "id";
pub const X_ATTR: &str = "x";
pub const Y_ATTR: &str = "y";
pub const Z_ATTR: &str = "z";
pub const V1_ATTR: &str = "v1";
pub const V2_ATTR: &str = "v2";
pub const V3_ATTR: &str = "v3";
pub const OBJECTID_ATTR: &str = "objectid";
pub const TRANSFORM_ATTR: &str = "transform";
// BBS
pub const OFFSET_ATTR: &str = "offset";
pub const PRINTABLE_ATTR: &str = "printable";
pub const AUTO_DROP_ATTR: &str = "auto_drop";
pub const INSTANCESCOUNT_ATTR: &str = "instances_count";
pub const CUSTOM_SUPPORTS_ATTR: &str = "paint_supports";
pub const CUSTOM_FUZZY_SKIN_ATTR: &str = "paint_fuzzy_skin";
pub const CUSTOM_SEAM_ATTR: &str = "paint_seam";
pub const MMU_SEGMENTATION_ATTR: &str = "paint_color";
// BBS
pub const FACE_PROPERTY_ATTR: &str = "face_property";

pub const KEY_ATTR: &str = "key";
pub const VALUE_ATTR: &str = "value";
pub const FIRST_TRIANGLE_ID_ATTR: &str = "firstid";
pub const LAST_TRIANGLE_ID_ATTR: &str = "lastid";
pub const SUBTYPE_ATTR: &str = "subtype";
pub const LOCK_ATTR: &str = "locked";
pub const BED_TYPE_ATTR: &str = "bed_type";
pub const PRINT_SEQUENCE_ATTR: &str = "print_sequence";
pub const FIRST_LAYER_PRINT_SEQUENCE_ATTR: &str = "first_layer_print_sequence";
pub const OTHER_LAYERS_PRINT_SEQUENCE_ATTR: &str = "other_layers_print_sequence";
pub const OTHER_LAYERS_PRINT_SEQUENCE_NUMS_ATTR: &str = "other_layers_print_sequence_nums";
pub const SPIRAL_VASE_MODE: &str = "spiral_mode";
pub const FILAMENT_MAP_MODE_ATTR: &str = "filament_map_mode";
pub const FILAMENT_MAP_ATTR: &str = "filament_maps";
pub const LIMIT_FILAMENT_MAP_ATTR: &str = "limit_filament_maps";
pub const GCODE_FILE_ATTR: &str = "gcode_file";
pub const THUMBNAIL_FILE_ATTR: &str = "thumbnail_file";
pub const NO_LIGHT_THUMBNAIL_FILE_ATTR: &str = "thumbnail_no_light_file";
pub const TOP_FILE_ATTR: &str = "top_file";
pub const PICK_FILE_ATTR: &str = "pick_file";
pub const PATTERN_FILE_ATTR: &str = "pattern_file";
pub const PATTERN_BBOX_FILE_ATTR: &str = "pattern_bbox_file";
pub const OBJECT_ID_ATTR: &str = "object_id";
pub const INSTANCEID_ATTR: &str = "instance_id";
pub const IDENTIFYID_ATTR: &str = "identify_id";
pub const PLATERID_ATTR: &str = "plater_id";
pub const PLATER_NAME_ATTR: &str = "plater_name";
pub const PLATE_IDX_ATTR: &str = "index";
pub const PRINTER_MODEL_ID_ATTR: &str = "printer_model_id";
pub const EXTRUDER_TYPE_ATTR: &str = "extruder_type";
pub const NOZZLE_VOLUME_TYPE_ATTR: &str = "nozzle_volume_type";
pub const NOZZLE_TYPE_ATTR: &str = "nozzle_types";
pub const NOZZLE_DIAMETERS_ATTR: &str = "nozzle_diameters";
pub const SLICE_PREDICTION_ATTR: &str = "prediction";
pub const SLICE_WEIGHT_ATTR: &str = "weight";
pub const FIRST_LAYER_TIME_ATTR: &str = "first_layer_time";
pub const TIMELAPSE_TYPE_ATTR: &str = "timelapse_type";
pub const OUTSIDE_ATTR: &str = "outside";
pub const SUPPORT_USED_ATTR: &str = "support_used";
pub const LABEL_OBJECT_ENABLED_ATTR: &str = "label_object_enabled";
pub const ENABLE_FILAMENT_DYNAMIC_MAP_ATTR: &str = "enable_filament_dynamic_map";
pub const HAS_FILAMENT_SWITCHER_ATTR: &str = "has_filament_switcher";
pub const SKIPPED_ATTR: &str = "skipped";

pub const OBJECT_TYPE: &str = "object";
pub const VOLUME_TYPE: &str = "volume";
pub const PART_TYPE: &str = "part";

pub const NAME_KEY: &str = "name";
pub const VOLUME_TYPE_KEY: &str = "volume_type";
pub const PART_TYPE_KEY: &str = "part_type";
pub const MATRIX_KEY: &str = "matrix";
pub const SOURCE_FILE_KEY: &str = "source_file";
pub const SOURCE_OBJECT_ID_KEY: &str = "source_object_id";
pub const SOURCE_VOLUME_ID_KEY: &str = "source_volume_id";
pub const SOURCE_OFFSET_X_KEY: &str = "source_offset_x";
pub const SOURCE_OFFSET_Y_KEY: &str = "source_offset_y";
pub const SOURCE_OFFSET_Z_KEY: &str = "source_offset_z";
pub const SOURCE_IN_INCHES: &str = "source_in_inches";
pub const SOURCE_IN_METERS: &str = "source_in_meters";

pub const MESH_SHARED_KEY: &str = "mesh_shared";

pub const MESH_STAT_EDGES_FIXED: &str = "edges_fixed";
pub const MESH_STAT_DEGENERATED_FACETS: &str = "degenerate_facets";
pub const MESH_STAT_FACETS_REMOVED: &str = "facets_removed";
pub const MESH_STAT_FACETS_RESERVED: &str = "facets_reversed";
pub const MESH_STAT_BACKWARDS_EDGES: &str = "backwards_edges";

// Store / load of TextConfiguration
pub const TEXT_TAG: &str = "slic3rpe:text";
pub const TEXT_DATA_ATTR: &str = "text";
// TextConfiguration::EmbossStyle
pub const STYLE_NAME_ATTR: &str = "style_name";
pub const FONT_DESCRIPTOR_ATTR: &str = "font_descriptor";
pub const FONT_DESCRIPTOR_TYPE_ATTR: &str = "font_descriptor_type";

// TextConfiguration::FontProperty
pub const CHAR_GAP_ATTR: &str = "char_gap";
pub const LINE_GAP_ATTR: &str = "line_gap";
pub const LINE_HEIGHT_ATTR: &str = "line_height";
pub const BOLDNESS_ATTR: &str = "boldness";
pub const SKEW_ATTR: &str = "skew";
pub const PER_GLYPH_ATTR: &str = "per_glyph";
pub const HORIZONTAL_ALIGN_ATTR: &str = "horizontal";
pub const VERTICAL_ALIGN_ATTR: &str = "vertical";
pub const COLLECTION_NUMBER_ATTR: &str = "collection";

pub const FONT_FAMILY_ATTR: &str = "family";
pub const FONT_FACE_NAME_ATTR: &str = "face_name";
pub const FONT_STYLE_ATTR: &str = "style";
pub const FONT_WEIGHT_ATTR: &str = "weight";

// Store / load of EmbossShape
pub const SHAPE_TAG: &str = "slic3rpe:shape";
pub const SHAPE_SCALE_ATTR: &str = "scale";
pub const UNHEALED_ATTR: &str = "unhealed";
pub const SVG_FILE_PATH_ATTR: &str = "filepath";
pub const SVG_FILE_PATH_IN_3MF_ATTR: &str = "filepath3mf";

// EmbossProjection
pub const DEPTH_ATTR: &str = "depth";
pub const USE_SURFACE_ATTR: &str = "use_surface";
// static constexpr const char *FIX_TRANSFORMATION_ATTR = "transform";

pub const BBS_VALID_OBJECT_TYPES_COUNT: u32 = 2;
pub const BBS_VALID_OBJECT_TYPES: [&str; 2] = ["model", "other"];

pub const BBS_INVALID_OBJECT_TYPES: [&str; 3] = ["solidsupport", "support", "surface"];
