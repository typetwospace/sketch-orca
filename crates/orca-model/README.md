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
| `model` | `Model`, `ModelObject`, `ModelVolume` and `ModelInstance`: bounding boxes, hulls, `ensure_on_bed`, `center_around_origin`, the print volume state and the arranged position written back. An object owns its parts and instances; what needs the object is an object method given the instance's index. | `Model.cpp` |
| `transformation` | `Geometry::Transformation`: the offset, rotation, scale and mirror taken out of a matrix (through Eigen's `computeRotationScaling`) and put back in the C++ order of products; `transform3d_from_string`. | `Geometry.cpp` |
| `config` | `ModelConfig`: an object's or a part's settings, set from text under the `EnableSilent` rule the command line passes. | `PrintConfig.hpp`, `Config.cpp` |
| `facets` | A part's painting as the 3MF reader loads it: the triangles to split, their bit stream and the states used. | `Model.cpp`, `TriangleSelector.cpp` |
| `build_volume`, `circle` | The print volume (rectangle, circle, convex or custom) and whether an instance is inside, colliding, outside or below it; the circle fits a round bed is found with. | `BuildVolume.cpp`, `Geometry/Circle.cpp` |
| `arrange` | An instance's arrange polygon with its filaments, temperatures, brim and height; the bed shrunk for the skirt; the items' inflation; the filaments' temperature classes. | `ModelArrange.cpp`, `Arrange.cpp`, `PrintConfig.cpp`, `Print.cpp`, `MaterialType.cpp` |

Still to come: the OBJ reader (`Format/OBJ.cpp`, `objparser.cpp`) and the 3MF reader
(`Format/bbs_3mf.cpp`, on `orca-zip` and `orca-xml`). `read_from_file` answers `.obj`, `.3mf`,
`.amf`, `.svg` and `.drc` with `ReadError::NotPorted`, which a caller declines.

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
| MLOD | A model file read as the command line reads it (generated boxes, refused files, OrcaSlicer's test STL files and its OBJ meshes as `model.stl`), then its boxes, hulls, instances, `ensure_on_bed`, the print volume states, the arrange polygons, an arranged position applied and `center_around_origin` |
| MARP | `get_bed_shape`, `update_arrange_params`, `get_shrink_bedpts` and the items' inflation |
| MTMP | `Print::get_filament_temp_type` and `get_compatible_filament_type` |

Every record of both sets matched on 2026-11-07. The probes found two bugs the unit tests had
missed: `get_rotation` takes the `Transform3d` overload of `extract_euler_angles` (columns
normalized first), and `FacetsAnnotation::reset()` leaves the used states.

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
