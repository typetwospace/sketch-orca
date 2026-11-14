# orca-model

The model and its readers: STL, OBJ and 3MF, Model, BuildVolume and ModelArrange.

The model as the C++ engine loads and places it before slicing, translated from OrcaSlicer
v2.4.2 by the o2-model task (2026-11-07). The command line's `load_model` (`cli/run.cpp`) reads a
file with `Model::read_from_file(path, ..., LoadModel | AddDefaultInstances)`, keeps only each
object's and part's `extruder`, and puts each object on the bed; this crate does each step as the
C++ does it, bit for bit.

## What is in it

| Module | What it does | C++ source |
| --- | --- | --- |
| `read` | `read_from_file`: the reader by the extension (compared without case), the objects given the file's path, an instance added to each object with none. The STL reader (`.stl`, `.oltp` with a 256-byte header) runs `orca-mesh`; an object is named after the text behind the path's last `\`. The errors are the C++ exceptions' texts. | `Model.cpp`, `Format/STL.cpp` |
| `obj` | The OBJ reader: the file read in text mode (`orca-fmt`'s `fopen("rt")`) in 64 KiB pieces, cut into lines at CR or LF, each line parsed as a C string (`strtod`, `strtol`); the material libraries beside it; faces of three or four corners (a quad split in two), a mesh turned inside out when its volume is negative. A line the parser refuses is skipped, keeping the corners of a face it had read; a last line with no line end is never read. Vertex and material colours go to `ObjInfo`, which only the GUI reads. Its failures, a line of a command it does not know and a material library that is not there go to the log as the C++ logs them, in the same words and order. | `Format/OBJ.cpp`, `Format/objparser.cpp` |
| `model` | `Model`, `ModelObject`, `ModelVolume` and `ModelInstance`: bounding boxes, hulls, `ensure_on_bed`, `center_around_origin`, the print volume state and the arranged position written back; the parts' cut information, text and shape configurations and the objects' cut ids, which the CLI's `.gcode.3mf` writer reads. An object owns its parts and instances; what needs the object is an object method given the instance's index. A part's mesh is an `Arc`, shared by the parts the 3MF reader makes from one mesh (`add_volume_with_shared_mesh`); centring one copies it (`parity/HAZARDS.md`). Since orca-o3-front-2 (2026-11-13): each model, object, part, instance and material has its id (`object_id`), kept by a copy and renewed by `assign_new_unique_ids_recursive` and `add_object_copy`; `assign_copy` and `assign_move` of `Model` and `ModelObject` (a clone is `assign_copy`, so its transformation caches are invalid, as the C++ leaves them, and a model's backup path is not copied); the materials (`ModelMaterial`, which only the AMF reader fills); `is_fdm_support_painted`, `is_seam_painted`, `is_mm_painted` and `is_fuzzy_skin_painted` at each level; `ModelObject::raw_mesh`. | `Model.cpp`, `ObjectID.hpp` |
| `object_id` | `ObjectID` and the timestamps of `ObjectWithTimestamp`: the process's two counters, ids from 0 and timestamps from 1. | `ObjectID.hpp`, `ObjectID.cpp` |
| `compare` | What the GUI and the print's apply step ask of two object lists: equal, extended, a part type's list changed, painting or brim ears changed, objects of several parts, advanced features. | `Model.cpp` 3596-3819, `BrimEarsPoint.hpp` |
| `stop` | `NotPorted`, the error a function returns where it reaches a family outside domain A (`painted-facets`: a painted part's split triangles, which `TriangleSelector` would build). | none |
| `transformation` | `Geometry::Transformation`: the offset, rotation, scale and mirror taken out of a matrix (through Eigen's `computeRotationScaling`) and put back in the C++ order of products; `transform3d_from_string`; `get_matrix_with_applied_shrinkage_compensation`. | `Geometry.cpp` |
| `config` | `ModelConfig`: an object's or a part's settings, set from text under the `EnableSilent` rule the command line passes. | `PrintConfig.hpp`, `Config.cpp` |
| `facets` | A part's painting as the 3MF reader loads it: the triangles to split, their bit stream and the states used; the states each triangle's tree leaves on its leaves, which a part's extruders list (`TriangleSelector::deserialize` read without building the splits). The annotation's timestamp (touched by `set_data` and `reset`), `has_facets` and `get_facets_strict`: on an annotation with no painting it returns the part's used vertices and no triangles, as the C++ does; any painting answers `NotPorted` (`painted-facets`). | `Model.cpp`, `TriangleSelector.cpp` |
| `build_volume`, `circle` | The print volume (rectangle, circle, convex or custom) and whether an instance is inside, colliding, outside or below it; the circle fits a round bed is found with. | `BuildVolume.cpp`, `Geometry/Circle.cpp` |
| `arrange` | An instance's arrange polygon with its filaments, temperatures, brim and height; the bed shrunk for the skirt; the items' inflation; the filaments' temperature classes; the bed's excluded area and the bed shape with it cut out (`get_bed_shape_with_excluded_area`). | `ModelArrange.cpp`, `Arrange.cpp`, `PrintConfig.cpp`, `Print.cpp`, `MaterialType.cpp` |
| `bbs_3mf` | The 3MF reader, whole, ported in three pieces. Piece 1 (2026-11-10): the archive opened through `orca-zip`, its relationships and the root `3D/3dmodel.model` parsed through `orca-xml` (objects, meshes, components, build items, transforms, units, metadata), the model's information (design, profile, model, metadata items), objects without instances deleted, the transforms applied. Piece 2 (2026-11-11): the loop over the other entries (`entries`): `model_settings.config` through expat (`config`: the objects' and parts' metadata, mesh statistics, text and shape configurations, the plates and their instances, the assembly items), the cut information and height ranges through `orca-xml`'s `read_xml` and `Ptree`, the layer heights, the brim ears and the svg files of embossed shapes; each entry extracted without allocating from the size it claims. Piece 3 (2026-11-11): the sub-model files the root model's own relationships name (`objects`: one `ObjectImporter` for each, run through `orca-tbb`'s `parallel_for` and merged in order), and the assembly into the model (`assemble`): another program's objects split one per build item, the colour groups' extruders, each object's settings, layer heights, height ranges and brim ears, its parts made from the meshes its components reach (shared meshes, painting, types, text and shape configurations, metadata, settings), its cut information, a lone part's offset moved to the instances, the parts' source, the extruders checked, the plates' indexes checked and their instances' loaded ids. The branches a `LoadModel \| AddDefaultInstances` load never takes (restore, version checks, config, auxiliary files) are named in the module headers. Errors go to the log as the C++ logs them; a refused read is "Loading of a model file failed.", an exception that escapes the reader is `ReadError::Message` with its text and no log. `names` holds the file's names, `attrs` its attribute and transform helpers, `fast_float` the float grammar it parses numbers with, `spirit` Boost Spirit's `qi::int_`, `lexical_cast` Boost's `lexical_cast<int>`, `text` the text and shape configurations' attributes. | `Format/bbs_3mf.cpp`, `fast_float.h`, Boost Spirit, Boost.LexicalCast |
| `emboss` | A part's text configuration and the shape it was embossed from (with its svg file's data), as the 3MF reader reads them: only the data. | `TextConfiguration.hpp`, `EmbossShape.hpp` |
| `semver` | `Semver` as the 3MF reader parses a version: semver.c's parse with its 50-byte slices and `strtol`, and a fourth number folded into the patch. | `Semver.hpp`, `semver/semver.c` |
| `utils` | What the 3MF reader takes from `utils.cpp` and Boost: the temporary folder, the process id, the local time and `put_time`, `encode_path` (the ANSI code page on Windows), `xml_unescape`, Boost's `quoted` and path `stem`, `save_string_file`. | `utils.cpp`, Boost.Filesystem, Boost io |

The 3MF reader reads a file to the end: no 3MF answers `ReadError::NotPorted` but at an entry
only a load with `LoadConfig`, `LoadAuxiliary` or `Restore` reads, which the command line's load
never asks for. What the reader does not keep: the plate list (the command line releases it
unread; a plate's index is still checked), the restore branches and backup archives, the
project's presets and configuration, the progress callback, the backup ids of objects, and
`check_painting_version` (only code the C++ has commented out calls it). `read_from_file`
answers `.amf`, `.svg` and `.drc` with `ReadError::NotPorted`, which a caller declines.

The reader writes the objects' backup folder (`Model::get_backup_path`) under
`utils::temporary_dir()`, as the C++ does; a caller sets it first with
`utils::set_temporary_dir(work_dir)`, as `cli/run.cpp` does, or the folder lands in
`/orcaslicer_model` at the drive's root. The folder is never removed (`~Model`'s `remove_backup`
is the GUI's backup thread, not translated).

Where the C++ is undefined (a part without a hull, a missing option, a painting string read past
its end, the CR box), the hazards are listed with what the translation does in
`parity/HAZARDS.md`, "orca-model".

## Probe gate

`tests/probes.rs` replays the records of `model.sop1` (`parity/SOP1.md`, "Probe files"), which
`bun run engine parity probe --only model` writes from the C++ engine's own build
(`parity/oracle/probe/model.cpp`): the committed subset in `parity/probes/` by default, the full
set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. Each record holds its inputs and what the
engine computed; the test computes the same from the inputs, writes it the same way and compares
the bytes, naming the field of the first difference.

| Record | What it proves |
| --- | --- |
| MXFM | `Transformation` set from an offset, a rotation, a scale and a mirror, made from a matrix, and `transform3d_from_string` |
| MFAC | Painting strings: the bit stream, the used states, each triangle's string back, and `reset()` |
| MCFG | `ModelConfig::set_deserialize`: the options left, or the exception's text |
| MCIR | `circle_ransac` and `smallest_enclosing_circle_welzl` |
| MBVL | `BuildVolume` of a bed outline with extruder areas, then `object_state` and `volume_state_bbox` of a box at many places |
| MLOD | A model file read as the command line reads it (generated boxes, refused files, OrcaSlicer's test STL files and its OBJ meshes as `model.stl` and as they are, the OBJ fixtures of `parity/oracle/probe/fixtures/model/` with their material libraries, OBJ files made by the probe for line ends, NULs, Ctrl-Z, long lines, numbers and refused faces, and in the full set the corpus's model files (STL, OBJ and two material libraries, each read alone)), its parts' painting, then its boxes, hulls, instances, `ensure_on_bed`, the print volume states, the arrange polygons, an arranged position applied and `center_around_origin` |
| M3MF | A 3MF file read as the command line reads it (the 3MF fixtures of `parity/oracle/probe/fixtures/model/`, archives it refuses, variants of the number fixture, and in the full set the corpus's 19 3MF files and OrcaSlicer's own projects): the engine's log of the read as stderr gets it (the importer's errors among it), then the error, or what the read leaves (the model's information, settings, layer heights, height ranges, brim ears, loaded ids, assembly) and the model as MLOD records it, for some files with MLOD's steps; each names the piece of the 3MF reader's port it needs. The records of all three pieces replay |
| MARP | `get_bed_shape`, `update_arrange_params`, `get_shrink_bedpts` and the items' inflation |
| MTMP | `Print::get_filament_temp_type` and `get_compatible_filament_type` |

Every record of both sets matched on 2026-11-07; with the OBJ reader, on 2026-11-09, every record
of both sets but the corpus's 3MF file, which moved to M3MF on 2026-11-10 (every MLOD record of
both sets matched then). With piece 1 of the 3MF reader, on 2026-11-10, every M3MF record of
piece 1 matched (13 committed, 15 in the full set), with every older record. With piece 2, on
2026-11-11, every M3MF record of pieces 1 and 2 matched (13 and 13 committed, 17 and 45 in the
full set), with every MLOD record of both sets. The probes found two bugs
the unit tests had missed: `get_rotation` takes the `Transform3d` overload of `extract_euler_angles` (columns
normalized first), and `FacetsAnnotation::reset()` leaves the used states. Piece 2 found a third:
`ModelVolumeType::from_string` and `to_str` used the enum's names, not the 3MF file's
(`normal_part`, `negative_part`, `modifier_part`, `support_enforcer`, `support_blocker`, with
`MODEL_PART` for any other string). With piece 3, on 2026-11-11, every record of both sets
matched (the M3MF records of piece 3: 13 committed, 57 in the full set). Piece 3 found a fourth:
a part's extruders listed every state its painting's bit stream names (`used_states`), where the
C++ lists the states each triangle's tree leaves on its leaves, so a nibble past a tree (Bambu's
`48`) added a filament. The 17 records orca-3mf-probe-3 added to the full set on 2026-11-11 (the
order of `cut_id`'s attributes, claimed sizes and two entries of one name) matched once
`cut_id`'s attributes were read as MSVC reads them, right to left: the full set's M3MF records
are 19, 60 and 57 by piece, the committed set's 13, 13 and 13.

The same test replays the records of `slice.sop1` of the piece front-2 (`parity/sop1/slice.md`)
through `tests/probes/slice_front.rs` (orca-o3-front-2, 2026-11-13). The ids and timestamps in
them are written as tokens numbered in the order each first appears, not as the process counters'
values; the replay loads ids and timestamps from the tokens.

| Record | What it proves |
| --- | --- |
| SLFE | `its_face_edge_ids` in the three forms the probe records: whole, masked and from face neighbours (orca-mesh's `face_edge_ids`) |
| SLMC | The comparisons of `compare`, the painted queries and `raw_mesh` between model a and model b; and b made again from a through `Model::assign_copy` and the recorded change (every candidate object, part and second object is tried, since the probe's draw is not recorded, and one must give the recorded bytes) |
| SLTS | `FacetsAnnotation`: `has_facets`, `get_facets_strict` and `equals` |
| SLSH | `get_matrix_with_applied_shrinkage_compensation` |
| SLBX | `get_bed_shape_with_excluded_area` |

Every record of both sets matched on 2026-11-13: SLFE 16 and 51 (committed and full), SLMC 24
and 132, SLTS 31 and 60, SLSH 48 and 80, SLBX 25 and 40. Where `get_facets_strict` answers
`NotPorted` (a painted part; 135 fields in the full set), the test counts the stop and does not
compare that field.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the code
it carries keeps its own licence, which `NOTICE` names in the crate's own section.

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`
- `orca-stl`
- `orca-tbb`
- `orca-fmt`
- `orca-json`
- `orca-config`
- `orca-eigen`
- `orca-geom`
- `orca-exact`
- `orca-qhull`
- `orca-mesh`
- `orca-zip`
- `orca-xml`
- `orca-arrange`
- `orca-probe` (development only: the probe gate)
