# slice.sop1

The record kinds of the slice probe (`parity/oracle/probe/slice.cpp` and the sources of
`parity/oracle/probe/slice/`, phase O3): the Model and mesh helpers the slicer calls,
TriangleMeshSlicer, SlicingParameters and adaptive slicing, recorded by orca-o3-probe-slice
(2026-11-13) from the oracle's libslic3r. The values are those of `parity/SOP1.md` ("The
container", "Values" and "Probe files"). Every kind starts with `SL`, and no other page defines it
(`scripts/lib/parity/probe.ts`, `probeKindProblems`, which `test/parity-probe.test.ts` runs).
`orca-model` replays the records of the piece front-2 and `orca-slice` those of slice-1 and
slice-2: each section below names its piece, and the crate's probe test rebuilds every record of
its pieces from the Rust and compares it whole.

A record holds its inputs, then what the engine computed from them. Inputs are shapes the probe
builds (seeded with the splitmix64 of `parity/oracle/probe/probe.hpp`, one generator per record
function, its seed given in the kind's section) and the OBJ meshes of OrcaSlicer's tests
(`<exe>/../src/tests/data`), never the corpus. The probe checks that `SCALING_FACTOR` is 1e-6 and
runs under the oracle's threading: `tbb::global_control` of one thread inside a `task_arena` of 8,
so the intersection lines `slice_mesh` collects under its mutexes come in the order of the faces,
and `its_face_neighbors_par` walks the faces in order. Log lines (`BOOST_LOG_TRIVIAL`) are not
recorded.

## slice.sop1's values

`polygon`, `expolygon` and `matrix` are those of `parity/SOP1.md` ("Values"): scaled `i64` points,
a `u64` count before each list, a matrix's 16 `f64` column by column. A `bool` is a `u8`, a `str` a
`u32` byte count and the bytes. Besides them:

- a mesh (`indexed_triangle_set`): a `u64` count of vertices, each `f32` x, y, z; then a `u64`
  count of faces, each `i32` three vertex indices;
- edges: a `u64` count of faces, each `i32` three values (an edge id or a neighbour per face edge,
  -1 where there is none);
- a transformation: 12 `f64`, the offset, the rotation (radians), the scaling factor and the
  mirror, each x, y, z. The `Geometry::Transformation` is made from a default one by
  `set_offset`, `set_rotation`, `set_scaling_factor` and `set_mirror`, in that order;
- floats: a `u64` count of `f32`; a profile: a `u64` count of `f64` (a layer height profile is
  pairs of z and layer height, a layer series pairs of low and high z);
- settings: a `u64` count, each `str` key and `str` value. A record's settings make a
  `PrintConfig` and a `PrintObjectConfig` at their defaults, each setting set from its text by
  `set_deserialize_strict`, in order, on whichever of the two holds its key (both when both do);
- the parameters (`SlicingParameters`), in this order: `bool` valid, `u64` base raft layers, `u64`
  interface raft layers, `f64` base raft layer height, interface raft layer height, contact raft
  layer height, layer height, min layer height, max layer height, max support layer height
  (`max_suport_layer_height`), first print layer height, first object layer height, `bool` first
  object layer bridging, `bool` zero gap interface top, bottom and raft, `f64` gap raft object, gap
  object support, gap support object, raft base top z, raft interface top z, raft contact top z,
  object print z min, object print z max, object print z uncompensated max, object shrinkage
  compensation z;
- ranges (`t_layer_config_ranges`): a `u64` count, each `f64` low z, `f64` high z, `f64` the
  range's `layer_height` (the only key its config holds), in the map's order;
- an object: a `u64` count of volumes, each `u32` the mesh's id in SLMH, `i32` its type
  (`ModelVolumeType`) and its `matrix`; then the one instance's `matrix`, `bool` the instance is
  left-handed, and the object's ranges. The object is a `ModelObject` of a `Model`: each volume
  added with `add_volume(TriangleMesh, type, false)` (the mesh as given, not centred) and given its
  matrix, then one instance with its matrix. The probe builds it from the table's meshes: the
  first volume a part, a second one (one case in three) a part or a modifier moved by up to 5 mm;
  the instance rotated about z (and about x and y one time in four each), scaled (in z on its own
  one time in three), mirrored in x one time in five, moved in x and y, and lifted so that its
  lowest vertex is at z 0;
- a slicing mode: `u32` mode (`MeshSlicingParams::SlicingMode`: 0 Regular, 1 EvenOdd, 2 Positive,
  3 PositiveLargestContour), `u64` `slicing_mode_normal_below_layer`, `u32` `mode_below`, `matrix`
  `trafo`.

## The committed set

The committed set (`parity/probes/slice.sop1`) takes every mesh of SLMH with 300 faces or fewer
(the small meshes), then, in the file's order, each record of another kind that the probe marks,
while the payload bytes of that kind's records there stay within its share (a marked record that
would pass the share is left out, and later ones are still taken while they fit). A record is
marked when every mesh it names is small, and besides: for SLFE when the mesh has 120 faces or
fewer; for SLMS and SLMX when its case (0 to 6) is the mesh's id modulo 7 and its payload is 6000
bytes or fewer; for SLM1 when its case (0 to 2) is the id modulo 3; for SLSB when its case (0 or
1) is the id modulo 2 and its payload is 4000 bytes or fewer. So the cases rotate over the small
meshes, with at most one record of each mesh in each of these kinds. A small mesh whose marked
record passes its kind's payload cap (6000 bytes for SLMS and SLMX, 4000 for SLSB) has no
committed record of that kind: in SLMS the meshes 3, 5, 7, 35, 37, 40 and 45, in SLMX the mesh 3,
in SLSB the meshes 4, 5, 6, 7, 18, 21, 35, 37 and 43; and SLSB's share leaves out its last ones,
the meshes 41, 44 and 45. The records of SLMC, SLTS, SLSH, SLBX, SLPR, SLRG, SLSM, SLAJ and SLOL
are all marked. The shares, in bytes: SLFE 24000, SLMC 64000 (every one of SLMC's 22 changes),
SLTS 20000, SLSH 12000, SLBX 12000, SLPR 40000, SLRG 20000, SLMS 66500, SLM1 17500, SLMX 66500,
SLSB 30000, SLAD 24000, SLAP 18000, SLSM 18000, SLAJ 18000, SLOL 22000. The full set
(`<work>/parity/probes/slice.sop1`) takes every record.

## The pieces and their edges

front-2 replays SLFE, SLMC, SLTS, SLSH and SLBX: `its_face_edge_ids`
(`src/libslic3r/TriangleMesh.cpp`), the copies of `Model` and `ModelObject`, the painted queries
and `raw_mesh`, the comparisons PrintApply makes, `FacetsAnnotation`'s `equals`, `has_facets` and
`get_facets_strict` (`src/libslic3r/Model.cpp`), `TriangleSelector::has_facets`
(`src/libslic3r/TriangleSelector.cpp`), the shrinkage compensation (`src/libslic3r/Geometry.cpp`)
and the bed's excluded area (`src/libslic3r/PrintConfig.cpp`). `check_model_ids_validity` and
`check_model_ids_equal` only assert, and the oracle is built without asserts: they compute nothing
a record could hold.

slice-1 replays SLPR and SLRG: `SlicingParameters::create_from_config`, `equal_layering`, the
nozzle limits and `layer_height_profile_from_ranges` (`src/libslic3r/Slicing.cpp`, its first
part). slice-1's lines of `src/libslic3r/TriangleMeshSlicer.cpp` (the facet slicing:
`slice_facet`, the transformation of the vertices, the intersection lines) are file-local and
have no boundary of their own: the engine reaches them only through `slice_mesh`, `slice_mesh_ex`
and `slice_mesh_slabs`, so slice-2's SLMS, SLM1, SLMX and SLSB hold them, and slice-1's port is
proved through slice-2's replay.

slice-2 replays SLMS, SLM1, SLMX and SLSB (the public functions of
`src/libslic3r/TriangleMeshSlicer.cpp`, which reach its file-local line chaining by triangle
connectivity, exact and then closing gaps, the loops and `make_expolygons`), SLAD
(`src/libslic3r/SlicingAdaptive.cpp`), and SLAP, SLSM, SLAJ and SLOL (`src/libslic3r/Slicing.cpp`,
its second part, with the file-local `adjust_layer_series_to_align_object_height` reached through
`generate_object_layers`). PrintObjectSlice calls `slice_mesh_ex` with the object's trafo times the
volume's matrix, a closing radius of `slice_closing_radius` (0.049 by default; 0 is allowed, and
then with no extra offset `make_expolygons` takes only the union), no extra offset and a resolution
of 0.0025 (0 at a print resolution of 0.001 or less), in the mode of `slicing_mode` (spiral vase:
PositiveLargestContour above `slicing_mode_normal_below_layer`): SLMX holds those settings and
others around them. Before the call `slice_volume` (`src/libslic3r/PrintObjectSlice.cpp`) flips the
triangles when `params2.trafo.rotation().determinant() < 0.`. Eigen's `rotation()` comes from
`computeRotationScaling` (the polar decomposition, its last column times the sign that makes the
determinant positive), so its determinant is always +1 and the C++ never flips there: a mirrored
volume is sliced with its triangles as they are, and its loops run the other way. A port does not
flip either. SLMS, SLM1 and SLMX record mirrored cases both unflipped (as the engine slices them)
and flipped (`its_flip_triangles` before the call), at random.

The edge: painted facets are outside domain A. `get_facets_strict` on an annotation that holds
data deserializes a `TriangleSelector`, which front-2 does not carry: there front-2's port stops
with NotPorted naming `painted-facets`, and SLTS keeps the C++ result for the phase that carries
painting. On an empty annotation the result is an empty mesh, which front-2 gives.
`slice_mesh_slabs` is reached in the engine only for painted facets, but its lines are slice-2's,
so SLSB records it whole.

A second edge, in `slice_mesh` (the polygons) only: on a layer in PositiveLargestContour mode it
keeps the loop of the largest area, and when the layer has loops whose areas are all 0 it
dereferences a null pointer (`make_loops` of `src/libslic3r/TriangleMeshSlicer.cpp`; the probe
crashed there on the prism cut at its ridge). The engine never gets there: PrintObjectSlice slices
through `slice_mesh_ex`, which gives `slice_mesh` Positive in place of PositiveLargestContour and
keeps the largest contour itself (`keep_largest_contour_only`). So SLMS and SLM1 slice a case that
asks for PositiveLargestContour once in Regular first, and where a layer that would take that mode
has loops and none of nonzero area, the case is sliced in Positive instead; the record's mode
fields hold the modes the call used. slice-2's port must not fail on such a layer, and since no
record holds one, what it gives there is its own choice.

Orders: these `std::sort`s compare a key that ties, so the order of equals is the one MSVC's
introsort leaves, and the ports sort them with `orca-stl`'s sort (`orca_stl::sort_by`), never
Rust's own, as `parity/HAZARDS.md` says of the STL's orders. In front-2, `create_edge_map`
(`src/libslic3r/TriangleMesh.cpp`, 558) sorts the edges on their vertex pair only: where more than
two faces share an edge, which faces `its_face_edge_ids` pairs follows the sort. In slice-2, in
`src/libslic3r/TriangleMeshSlicer.cpp`: `chain_lines_by_triangle_connectivity` sorts the lines by
`edge_a_id` and by `a_id` (1077, 1078), and the first unused line of equals is the next one
chained (equals come from edges of three or more faces, faces wound against each other, and faces
around a vertex on the plane); `open_polylines_sorted` sorts the open polylines by length,
longest first (1173), the order both `chain_open_polylines_exact` and
`chain_open_polylines_close_gaps` seed from; `chain_open_polylines_exact` sorts the polyline ends
by their vertex or edge id (1202), and the first unconsumed end of equals is joined. In
`src/libslic3r/SlicingAdaptive.cpp`, `prepare` sorts the faces by `z_span`, the pair of low and
high z (96): faces with equal spans and different normals tie, and their order is the order SLAD
records `m_faces` in and the one `next_layer_height`'s facet index walks. The ties really occur:
`orca:extruder_idler_quads.obj` has 92 edges shared by three or more faces, and the meshes wound
against their neighbours give equal ids. One more order is not a sort:
`chain_open_polylines_close_gaps` finds the nearest polyline end through
`ClosestPointInRadiusLookup` (`src/libslic3r/Point.hpp`), a `std::unordered_multimap` keyed by grid
cell. Its `find` reads four cells in a fixed order, each cell's ends in the order they went in
(MSVC's `unordered_multimap` puts a new element after the last one of its key, so an end erased
and inserted again goes last), and keeps the first of equal distances: a port keeps each cell's
ends in a list in that order. `parity/HAZARDS.md` has the row of `create_edge_map`; the rows of the
other five sorts and of the lookup go to it (orca-o3-probe-slice's returned text).

### SLMH

The mesh table, the input of the records that name a mesh by its id (front-2 and slice-2 read it;
no port computes it): `u32` id (its index), `str` label, `bool` symmetric, the mesh. A mesh is
symmetric when the neighbours `its_face_neighbors` gives, walked as the neighbour overload of
`its_face_edge_ids` walks them, write every slot of that overload and find every edge in its
neighbour: that overload leaves its slots uninitialised and writes them as it walks, so SLFE calls
it and SLSB calls `slice_mesh_slabs` only on symmetric meshes. The meshes, in the order of their
ids: 30 shapes the probe builds (`parity/oracle/probe/slice/meshes.cpp`), a 20 mm cube
(`its_make_cube`), a 30 by 10 by 5 box centred in x and y, a pyramid, a cylinder, a cone, a sphere,
a frustum, a torus, a box inside a box, a hollow box (the inner box flipped), two overlapping cubes,
an open box (two faces removed), a box whose +x wall has vertices of its own (touching, then 0.3 mm
away), a flipped cube, a cube with a degenerate face and an unused duplicate vertex, two stacked
blocks (horizontal faces at the planes of a layer series), a tetrahedron, a jittered sphere (seed
0x51CE0001), a cube at x and y 150, a 0.5 mm cube, two islands 0.08 mm apart, a wall 0.08 mm thin,
a prism, a wedge whose vertices lie on the heights of 0.2 mm layers, then five meshes with faces
wound against their neighbours (indices 1 and 2 of a face swapped): the cube with face 4 flipped
(a triangle of its +x wall, which every plane between its ends cuts), that mesh flipped
whole (face 4 the only one left as the cube has it), the box whose +x wall has vertices of its own
with that wall (faces 4 and 5) flipped, touching and then 0.3 mm away, and the second of those
flipped whole. The flipped cube and the hollow box flip closed shells, which stay consistent; only
a face wound against its neighbour makes two intersection lines meet end to end. So the five give
`make_loops`' second rounds (`try_connect_reversed`) polylines to join in reverse, through the
edge ids in `chain_open_polylines_exact` (the cube) and by distance in
`chain_open_polylines_close_gaps` (the boxes, whose wall shares no edge with the rest), and the
loop each joins is reversed where it runs clockwise (the two meshes flipped whole: at z 10 their
loop is counter-clockwise, where the flipped cube's is clockwise); then 21 OBJ files of
OrcaSlicer's tests as `load_obj` reads them, labelled `orca:<name>.obj`, these files of
`<exe>/../src/tests/data`: `20mm_cube.obj`, `2x20x10.obj`, `V.obj`, `V_standing.obj`, `bridge.obj`,
`cube_with_concave_hole.obj`, `cube_with_concave_hole_enlarged.obj`,
`cube_with_concave_hole_enlarged_standing.obj`, `cube_with_hole.obj`, `ipadstand.obj`,
`overhang.obj`, `pyramid.obj`, `simplification.obj`, `sloping_hole.obj`, `small_dorito.obj`,
`two_hollow_squares.obj`, `A.obj`, `A_upsidedown.obj`, `extruder_idler.obj`,
`extruder_idler_quads.obj`, `frog_legs.obj`. The 46 meshes of 300 faces or fewer (ids 0 to 45)
are all but the last five.

### SLFE

`its_face_edge_ids` in its three forms (front-2): `u32` mesh, the edges of `its_face_edge_ids(its)`;
a `u64` count and a `bool` per face, the face mask (each face kept with a chance of one to four in
four, drawn per mesh), the edges of `its_face_edge_ids(its, mask)`; `bool` symmetric, and when true
the edges `its_face_neighbors` gives, then for `assign_unbound_edges` false and true the edges of
`its_face_edge_ids(its, neighbours, assign_unbound_edges, &num_edges)` and `i32` num_edges (set to
-7 before the call). One record per mesh of SLMH; seed 0x51CE1000.

### SLMC

The Model comparisons PrintApply makes (front-2). Model a has one to three objects of the small
meshes of 40 faces or fewer: one to three volumes each (the first a part, the others a part or a
type drawn from all five), half of them transformed, a quarter of them painted (each of the
support, seam, colour and fuzzy-skin annotations a drawn bit picks given
`set_triangle_from_string(0, "4")`), sometimes with `extruder` or `wall_loops` in their config; one
or two instances; the object's config empty, `extruder` 1 or `layer_height` 0.12; and sometimes
one or two brim points. Model b is the copy `Model b(a)`, then one change through Model's own API,
by its `u8` number: 0 none, 1 an object cloned onto the end (`add_object`), 2 an object deleted, 3
an object replaced by its clone, 4 two objects swapped, 5 a volume added (a part or a modifier), 6
the volumes cleared, 7 a volume's type changed between part and negative volume, 8 a volume moved
1 mm in x, 9 moved 1e-13 mm, 10 support painting set on triangle 1 (`"8"`), 11 to 14 the support,
seam, colour and fuzzy-skin annotations reset, 15 a brim point added, 16 an instance added, 17 the
instances cleared, 18 `sparse_infill_density` 35% set on the object, 19 b made by moving a copy of
a, 20 the objects cleared, 21 the materials cleared. Records 0 to 21 make changes 0 to 21 in turn;
the rest draw one. 132 records; seed 0x51CE2000.

The record: `u8` change, then model a and model b, each a `u64` count of objects, each: `u32` id
token, a `u64` count of volumes, each `u32` id token, `i32` type, `u32` mesh, `matrix`, then for the
support, seam, colour and fuzzy-skin annotations in that order `u32` timestamp token and `bool`
empty, then the volume's config as a `u64` count of `str` key and `str` value
(`opt_serialize`); then `u64` the count of instances, the object's config as the volume's, and a
`u64` count of brim points, each `f32` x, y, z and head front radius. A token numbers the object
IDs and the timestamps from 0 in the order the record first writes them (a and b share the
numbering), so equal IDs and equal timestamps show without the counters' values. Then the results:
`bool` `model_object_list_equal(a, b)`, `bool` `model_object_list_extended(a, b)`; a `u64` count
of the objects a and b both have, each `bool` `model_volume_list_changed` for MODEL_PART, for
{MODEL_PART, NEGATIVE_VOLUME, PARAMETER_MODIFIER}, for SUPPORT_BLOCKER and for SUPPORT_ENFORCER,
then, only when the parts did not change (PrintApply asks no more otherwise),
`model_custom_supports_data_changed`, `model_custom_seam_data_changed`,
`model_mmu_segmentation_data_changed` and `model_fuzzy_skin_data_changed`, then
`model_brim_points_data_changed`; `bool` `model_has_multi_part_objects` of a and of b, `bool`
`model_has_advanced_features` of a and of b; `bool` b's `is_fdm_support_painted`,
`is_seam_painted`, `is_mm_painted` and `is_fuzzy_skin_painted`; a `u64` count of b's objects, each
the same four `bool`s and the mesh of `raw_mesh()`.

### SLTS

`FacetsAnnotation` and `TriangleSelector::has_facets` (front-2). A volume of a small mesh of 40
faces or fewer (records 0 to 3: the cube, the degenerate cube, the tetrahedron, the pyramid) is
added to an object with `add_volume(TriangleMesh, MODEL_PART, false)`, and its support annotation
is given up to four triangles' strings by `set_triangle_from_string`, at rising triangle ids. A
string is a random tree of `TriangleSelector`'s serialization, at most two levels deep, nibble by
nibble from the string's end: a split node is its number of split sides (1 to 3) with its special
side (0 to 2) shifted by 2, then one child more than the sides; a leaf is its state shifted by 2,
or `0b1100` then the state less 3 for states 3 to 15. Records 0 to 3 and one in five of the rest
paint nothing.

The record: `u32` mesh, a `u64` count of strings, each `i32` triangle id and `str` the string;
`bool` `empty()`; 17 `bool`s, `has_facets` for each state from 0 to 16 (`EnforcerBlockerType`, up
to `ExtruderMax`); for states 1, 2 and 3 the mesh of `get_facets_strict` (the edge: front-2 gives
it only when the annotation is empty, and stops with NotPorted `painted-facets` otherwise); `bool`
`equals` with the same strings set on the seam annotation of a second volume; and when there are
strings, `str` the last string with its last character made `8` when it was `4` and `4` otherwise,
and `bool` `equals` with the strings, the last one so changed, set on that volume's colour
annotation after `reset()`. 60 records; seed 0x51CE3000.

### SLSH

`Transformation::get_matrix_with_applied_shrinkage_compensation` (front-2): the transformation,
`f64` the compensation x, y, z, then the `matrix` it gives. Record 0 the identity; the others
random transformations (tilted one time in three), the compensation 1 in one record of four and
otherwise either 100 over 97 to 103 in x and y with its own z, or 0.95 to 1.05 on each axis. 80
records; seed 0x51CE4000.

### SLBX

`get_bed_excluded_area` and `get_bed_shape_with_excluded_area` (front-2), on a `PrintConfig` whose
`printable_area`, `bed_exclude_area` and `extruder_printable_area` are the record's: a `u64` count
of the bed's points, each `f64` x, y; the excluded area's points as the bed's; a `u64` count of
extruder areas, each points as the bed's; then the polygons `get_bed_excluded_area` gives (a `u64`
count of `polygon`), the `polygon` `get_bed_shape_with_excluded_area(config, false)` and the
`polygon` with `true`. Records 0 to 7 by hand on a 256 mm bed: an excluded corner, none, a
clockwise corner, an area off the bed, a strip across it, an area over all of it, a round bed of
24 points with a square in it, and two extruder areas; then 32 drawn: a random bed, sometimes a
random excluded rectangle (sometimes reversed), sometimes two extruder areas. 40 records; seed
0x51CE5000.

### SLPR

`SlicingParameters::create_from_config`, `equal_layering` and the nozzle limits (slice-1): the
settings, `f64` the object's height, a `u64` count of extruders, each `u32`, `f64` the shrinkage
compensation x, y, z; the parameters of those; `str` key and `str` value, one more setting; the
parameters with it added; `bool` `equal_layering` of the two; then a `u64` n (the nozzle count
plus 2) and for each index from 0 to n - 1 `f64` `Slicing::min_layer_height_from_nozzle` and `f64`
`Slicing::max_layer_height_from_nozzle`, on a `DynamicPrintConfig` that holds only the settings'
`nozzle_diameter`, `min_layer_height` and `max_layer_height`. The settings: `layer_height`,
`initial_layer_print_height` (0 among them), one to three nozzles with their limits (0 among
them); one time in three the support settings (filaments, `enable_support`,
`enforce_support_layers`, the top and bottom distances, `raft_contact_distance`, the interface
layers, `independent_support_layer_height`); one time in four `raft_layers` 1 to 5. The extruders:
none, {1}, every nozzle, or {0}. The added setting is one of `layer_height`, `raft_layers`,
`initial_layer_print_height`, `support_top_z_distance`, `max_layer_height`, `min_layer_height`,
`enable_support`. 160 records (record 0 of height 0.1); seed 0x51CE9000.

### SLRG

`layer_height_profile_from_ranges` (slice-1): the parameters (of random settings, extruders {1}
and a random shrinkage, drawn as SLPR draws them), the ranges, then the profile it gives. Up to
five ranges, one in six from z 0, one in six under 2e-7 mm long, one in five past the object's top.
100 records (the first 40 of objects at most 8 mm high); seed 0x51CEA000.

### SLMS

`slice_mesh` over a list of heights (slice-2): `u32` mesh, `bool` flipped (the triangles flipped by
`its_flip_triangles` before the call; only a mirrored case may be, at random, and the engine never
flips, as "The pieces and their edges" says), the slicing mode, the floats of the heights, then a
`u64` count of layers, each a `u64` count of `polygon`.
Seven records per mesh of SLMH: a layer series (the mid-heights of layers of 0.12 to 2 mm, the first
of the same height, 0.2 or 0.3); the mesh's own vertex heights (planes through vertices, edges and
horizontal faces); a random mode and object transformation; EvenOdd at random heights; Positive at
one height, sometimes transformed; PositiveLargestContour with Regular or Positive below layer 1
to 4, sometimes transformed; and the heights below, at and above the mesh's ends (even ids) or none
(odd ids). At most 24 heights; they are those of the mesh transformed as the slicer transforms it.
One height takes `slice_mesh`'s path that transforms the vertices in place; more take the one of
`transform_mesh_vertices_for_slicing`. A PositiveLargestContour case may be sliced Positive, as
"The pieces and their edges" says. Seed 0x51CE6000, shared with SLM1.

### SLM1

`slice_mesh` at one plane (slice-2): `u32` mesh, `bool` flipped, the slicing mode, `f32` the
height, then the `polygon`s it gives (a `u64` count). Three records per mesh: Regular at the
mesh's mid-height, a random mode at one of its vertex heights, a random mode and transformation at
a random height; PositiveLargestContour guarded as in SLMS. Seed 0x51CE6000, after SLMS's
records.

### SLMX

`slice_mesh_ex` (slice-2): `u32` mesh, `bool` flipped, the slicing mode, `f32` closing radius,
`f32` extra offset, `f64` resolution, the floats of the heights, then a `u64` count of layers, each
a `u64` count of `expolygon`. Seven records per mesh, at a closing radius of 0.049 unless said:
PrintObjectSlice's settings (resolution 0.0025) on a random transformation; the vertex heights;
EvenOdd with a resolution of 0.0025, transformed; Positive with a closing radius of 0.2 and an
extra offset of 0.1 at random heights; a random mode, no closing radius and a resolution of 0.05,
with no extra offset on even mesh ids (`make_expolygons` takes only the union, as PrintObjectSlice
at a `slice_closing_radius` of 0) and an extra offset of 0.3 on odd ones; PositiveLargestContour
with Regular below layer 1 to 4, an extra offset of
-0.05 and a resolution of 0.0025; a closing radius of 1 at one height or the vertex heights. At
most 16 heights. Seed 0x51CE7000.

### SLSB

`slice_mesh_slabs` (slice-2), on the symmetric meshes: `u32` mesh, `matrix` the transformation,
`u8` flags (1 the tops asked for, 2 the bottoms, 4 the vertical points), the floats of the heights;
then with flag 1 a `u64` count of layers of tops, each a `u64` count of `polygon`; with flag 2 the
bottoms the same way; with flag 4 a `u64` count of vertical points, each `f32` x, y, z of the first
point and of the second. Two records per symmetric mesh: the identity with all three, and a random
transformation with random flags and a layer series or the vertex heights. Seed 0x51CE8000.

### SLAD

`SlicingAdaptive` (slice-2): the object, the parameters, then the faces `prepare` keeps (`m_faces`,
read through a subclass): a `u64` count, each `f32` low z, high z, `n_cos` and `n_sin`; `f32` the
quality; a `u64` count of calls of `next_layer_height`, each `f32` print z, `u64` the facet index
given, `f32` the height it gives and `u64` the facet index after; then `u64` 6 and six times `f32`
z and `f32` `horizontal_facet_distance(z)`. The calls: the profile's loop from the first object
layer height up the object, each with the index the last one left (at most 48), then four at random
heights from facet 0. Objects of the meshes of 400 faces or fewer, scaled up to 0.6 in the first 20
records and up to 2 in the rest; the quality 0, 0.25, 0.5, 0.75, 1 or random. 48 records; seed
0x51CEB000, shared with SLAP.

### SLAP

`layer_height_profile_adaptive` (slice-2): the object (with random ranges one time in three), the
parameters, `f32` the quality, then the profile it gives. 72 records (scaled up to 0.5 in the first
30); seed 0x51CEB000, after SLAD's records.

### SLSM

`smooth_height_profile` (slice-2): the parameters, the profile, `u32` radius (0, 1, 2, 5 or 10),
`bool` keep min, then the profile it gives. A profile is one of four kinds: from random ranges,
adaptive on an object, by hand (0, h, top, h or 0, h0, h0, h0, h0, h, top, h, the last height
sometimes 1.5 times), or random steps between the limits. 72 records (the first 30 at most 4 mm
high, or of objects scaled up to 0.2); seed 0x51CEC000.

### SLAJ

`adjust_layer_height_profile` (slice-2), on an object whose ranges are the record's: the parameters,
the ranges, the profile (from ranges or adaptive, as SLSM makes them), `f64` z, `f64` the height
delta, `f64` the band width, `u32` the action (`LayerHeightEditActionType`: 0 increase, 1 decrease,
2 reduce, 3 smooth), then the profile after the call. z lies from the first layer height (when it
is fixed) or just past half the band (when not) up to the top, and one time in eight past the top
or below the first layer; the band never reaches z 0 on a profile whose first layer is not fixed
(the C++ index would wrap there). 80 records (the first 32 at most 4 mm high, or of objects scaled
up to 0.2); seed 0x51CED000.

### SLOL

`generate_object_layers` and `check_object_layers_fixed` (slice-2): the parameters, the profile (as
SLSM makes them), `bool` precise Z height, then the layer series it gives and `bool`
`check_object_layers_fixed`. Every profile's z values rise and its heights are above 0, or
`generate_object_layers` would not end. 100 records (the first 40 at most 4 mm high, or of objects
scaled up to 0.2); seed 0x51CEE000.
