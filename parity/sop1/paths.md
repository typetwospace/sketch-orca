# paths.sop1

The record kinds of the paths probe (`parity/oracle/probe/paths.cpp`, phase O3): Clipper with Z,
Circle, ArcFitter, the 2D and 3D helpers of orca-geom, and ShortestPath's chaining.
orca-o3-probe-paths recorded them on 2026-11-13 and wrote each kind's `### KIND` section on this
page, in the values of `parity/SOP1.md` ("The container", "Values" and "Probe files"). Every kind
starts with `PS`, and no other page defines it (`scripts/lib/parity/probe.ts`, `probeKindProblems`,
which `test/parity-probe.test.ts` runs). `orca-geom` replays the kinds of the pieces geom-1 and
geom-2, `orca-paths` those of paths-1; each section names its piece.

The program's sources: `parity/oracle/probe/paths.cpp` runs the four of
`parity/oracle/probe/paths/`: `parity/oracle/probe/paths/clipperz.cpp` (PSZC),
`parity/oracle/probe/paths/geom1.cpp` (geom-1's other kinds), `parity/oracle/probe/paths/geom2.cpp`
(geom-2's) and `parity/oracle/probe/paths/chain.cpp` (paths-1's), with the writers and shapes of
`parity/oracle/probe/paths/slic.hpp` and the draws and sink of
`parity/oracle/probe/paths/common.hpp`. The order of the file is PSZC, the geom-1 kinds, the
geom-2 kinds, then PSLN (first, see its section) and the other paths-1 kinds, each kind's records
together, its hand-made cases first.

## Values

As in `parity/SOP1.md`'s Clipper and Geometry records, each value is written in its own width: a
point is `i64` x and y; a Point3 (and a `Vec3crd`, such as a box corner) `i64` x, y and z; a
`Vec2d` two `f64`, a `Vec3d` three, a `Vec3f` three `f32`; a `f64` or `f32` is its bit pattern. A
list of points, of Point3s, of polygons or of polylines is a `u32` count and its items (a polygon
or polyline being its point list); a list of ExPolygons is a `u32` count, each the contour's
points and the holes as a list of polygons. "A point if" is `u8` the call's result and the point
it left.

- A circle: its center (a point) and `f64` radius.
- An ArcSegment: `u8` is_arc, center, `f64` radius, start_point, end_point, `u8` direction
  (`ArcDirection`: 0 unknown, 1 CCW, 2 CW), `f64` length, angle_radians, polar_start_theta and
  polar_end_theta.
- A fitting result (`std::vector<PathFittingData>`): a `u32` count, each `u64`
  start_point_index, `u64` end_point_index, `u8` path_type (`EMovePathType`: 0 noop, 1 linear,
  2 arc CW, 3 arc CCW) and its ArcSegment.
- A Polyline or a Polyline3 "with its fit": its points, then its fitting result.
- A chain: a `u32` count, each `u64` the item's index and `u8` reversed. An order: `u64` count and
  `u64` indices.

Every record holds its inputs, then the engine's results, so a replay needs no generator. The
inputs are shapes this program makes (`parity/oracle/probe/paths/slic.hpp`: arcs clean and noisy,
straight runs, zigzags, stars, grids, infill lines, loops, clusters) in domain A's scaled
coordinates, drawn from splitmix64 (`parity/oracle/probe/probe.hpp`) with a seed for each kind;
none comes from the corpus. Hand-made cases go to both sets; of the random ones the committed set
takes the first few of at most a size each section gives.

## Edges

Inputs where the C++ reads outside its arrays or is otherwise undefined are not recorded, and a
port stops there with NotPorted rather than guess:

- `Circle::try_create_circle` of fewer than two points (one reads `points[-1]`, none counts from
  `size_t(-1)`); `Circle::is_over_deviation` and `get_deviation_sum_squared` of no points;
  `ArcSegment::try_create_arc` of fewer than two points; `ArcSegment::are_points_within_slice` of
  fewer than three (it reads `points[index - 1]` at index 0).
- `Polygon::convex_points` and `concave_points` of fewer than two points (`*(end - 2)`).
- `Polyline::split_at_index` of an empty polyline; the `first_point()` of an empty polyline or
  path anywhere in ShortestPath (`chain_polylines`, `chain_extrusion_paths`,
  `chain_extrusion_entities`: an entity must have end points; only
  `chain_and_reorder_extrusion_entities` drops those that have none, which PSXE records).
- `ThickPolyline::start_at_index` outside [0, size).
- `Point3::projection_onto(MultiPoint3)` throws a RuntimeError, which PSP3 records.
- `Geometry::ray_circle_intersections` calls `ray_circle_intersections_r2_lv2_c2`, which does not
  exist, so it cannot be instantiated; only `ray_circle_intersections_r2_lv2_c` (PSRC) can.
- Geometry.cpp's `dist` and `project_point_to_segment` (lines 287-306) are templates no code
  instantiates: there is nothing to record, and a port need not carry them.
- `chain_segments_greedy_constrained_reversals2` has no caller; its body,
  `chain_segments_greedy_constrained_reversals2_`, is reached only as `chain_polylines`'s
  `chain_segments_greedy2` (PSPL), with reversals never refused.
- `reorder_extrusion_paths` is declared in `ShortestPath.hpp` with a non-const chain and defined
  with a const one, so only `chain_and_reorder_extrusion_paths` can call it (PSXP).

### PSZC

geom-1 (`orca-geom`): ClipperLib_Z, the Clipper of the engine's `clipper.lib` built with
`CLIPPERLIB_USE_XYZ`, as `PerimeterGenerator`'s `clip_extrusion` and `LineSplit` use it. A Z path
is a `u32` count and its points as `i64` x, y and z; a list of them a `u32` count and the paths.
Inputs: `u8` the clip type (`ClipType`), `u8` subject open, `u8` the subject's and `u8` the clip's
fill type (`PolyFillType`), `u8` strictly simple (`ioStrictlySimple`), `u8` a Z fill callback set,
`u8` into a PolyTree (else into Paths); the subject paths and the clip paths (always closed).
Results: `u8` what `AddPaths` returned for the subject and for the clip; `u8` threw, `str` the
exception's text (empty when none: Clipper throws on open paths into Paths); `u8` what `Execute`
returned; the callback's calls in order, a `u32` count, each e1bot, e1top, e2bot, e2top and the
point it was handed (the callback sets that point's z to -(1 + the call's index)); the result paths
(`PolyTreeToPaths` of the tree when into a PolyTree); then the tree's nodes in `GetNext` order, a
`u32` count, each `u32` depth (0 for the tree's children), `u8` `IsHole()`, `u8` `IsOpen()` and the
contour (no nodes when into Paths). 12 hand-made cases (an open polyline through a square,
intersection and difference; overlapping squares, union with and without the callback,
intersection; a bow-tie, strictly simple and not; open paths into Paths; squares sharing an edge;
triangles with a Z per vertex, intersection and xor; coordinates past loRange), then 800 random:
one to three subject paths (open walks or closed stars) and up to three clips, in ranges 1e3 to
2e9, now and then snapped to a grid; Z all zero, the path's index, the point's index or drawn (the
committed set those and the first 12 of at most 2 KB).

### PSRZ

geom-1 (`orca-geom`): `Geometry::rotation_xyz_diff` and `rotation_diff_z`. Inputs: `Vec3d` from
and to (Euler angles). Results: `f64` w, x, y and z of the quaternion `rotation_xyz_diff(from,
to)`, then `f64` `rotation_diff_z(from, to)`. 6 hand-made cases (none, a quarter turn, a half turn
across, 1e-9, a tilted object, pi to -pi), then 300 random: rotations about Z alone (as an
instance's), tilted ones differing in Z, and a tenth with every angle drawn, for which the C++'s
debug check would fail (the committed set those and 16).

### PSIC

geom-1 (`orca-geom`): `Geometry::is_point_inside_polygon_corner`. Inputs: points a, b, c and q.
Result: `u8`. 11 hand-made cases (convex and concave corners, q on BA, on BC, at b, a straight
corner from either side, a == c, coordinates near 1e9), then 2000 random in the ranges of
`parity/oracle/probe/paths/slic.hpp` (`range`), now and then q = b, q on the ray BA, or c opposite
a (the committed set those and 64).

### PSCV

geom-1 (`orca-geom`): `Polygon::convex_points` and `concave_points`. Inputs: the points, `f64`
the angle threshold. Results: the list `convex_points(threshold)`, the list
`concave_points(threshold)`. 30 hand-made cases (a square, a notched square both ways round,
collinear points, a repeated point, each at the thresholds 0, 1e-12, pi/4, pi/2, 3pi/4 and pi;
`PrintObject.cpp` passes pi), then 600 random: stars of 3 to 24 points either way round and
notched rectangles, at those thresholds or a drawn one (the committed set those and the first 16
of at most 1 KB).

### PSRC

geom-1 (`orca-geom`): `Geometry::ray_circle_intersections_r2_lv2_c<double>`. Inputs: `f64` r2, a,
b, lv2 and c. Results: `i32` the count (0, 1 or 2) and the two `Vec2d` of out, (0, 0) where not
written. 4 hand-made cases (a tangent, a miss, a hit, a zero radius), then 500 random as
`FillBase.cpp` calls it: a circle about the origin and the line through a point along a vector
(the committed set those and 32).

### PSCI

geom-1 (`orca-geom`): Circle. Inputs: the points (two or more), `f64` max radius, `f64`
tolerance, `Vec3f` pos and center and `u8` ccw. Results: `u8` and the circle of
`try_create_circle(p[0], p[n / 2], p[n - 1], max radius)` (from a default circle); `u8` and the
circle of `try_create_circle(points, max radius, tolerance)` (from a default circle, which a
failed fit may still have changed); the circle c used next (the list's circle if it fitted, else
the three points', else `Circle(p[0], max(1, |p[n - 1] - p[0]|))`); `f64` `c.get_polar_radians`
of each point; `c.get_closest_point` of each point; `u8` `c.is_over_deviation(points,
tolerance)`; `u8` and `f64` sum of `c.get_deviation_sum_squared`; for each segment, no count,
`get_closest_perpendicular_point(p[i], p[i + 1], c.center)` as a point if (out starts at (0, 0));
`calc_tangential_vector(pos, center, ccw)`. 6 hand-made cases (three points on a circle,
collinear points, two points, an 8-point arc, a noisy one, a radius past the maximum), then
800 random arcs of 2 to 16 points, radii 1e3 to 3e9, either way round, clean or noisy, and arc
fitter paths (`fit_path`), at most 40 points (the committed set those and the first 24 of at most
2 KB).

### PSAS

geom-1 (`orca-geom`): `ArcSegment::try_create_arc` of a point list and what follows from the arc.
Inputs: the points, `f64` approximate length, max radius, tolerance and path tolerance percent,
the query points (a list). Results: `u8` and the arc (from a default arc); `u8`
`are_points_within_slice(arc, points)` (0 when no arc); then, on that arc, `u8` `reverse()` of a
copy and the copy; for each query point, no count, `u8` `is_point_inside`, `u8` `clip_start` of a
copy and the copy, `u8` `clip_end` of a copy and the copy, `u8` `split_at` and its two arcs. 5
hand-made cases (a quarter circle with its center, middle, a point off it and a vertex as queries,
the same reversed, a length three times too long, a half circle across 0, two points), then 1200
random: arcs of 3 to 20 points, clean or slightly noisy, and arc fitter paths, the length now and
then off by up to half, the defaults (`DEFAULT_SCALED_MAX_RADIUS`, `DEFAULT_SCALED_RESOLUTION`,
`DEFAULT_ARC_LENGTH_PERCENT_TOLERANCE`) or drawn ones, up to 4 queries (the center, a point of the
list, points on or off the circle) (the committed set those and the first 16 of at most 3 KB).

### PSAC

geom-1 (`orca-geom`): an ArcSegment from its constructor. Inputs: center, `f64` radius, start,
end, `u8` direction, the slice points, the query points, `Vec3f` s, e and c, `u8` ccw, a ray's
origin (a point) and direction (a `Vec2d`), a segment's two points. Results: the arc of
`ArcSegment(center, radius, start, end, direction)`; `u8` `are_points_within_slice(arc, slice)`
(0 when the arc is not valid); the queries as in PSAS (reverse, then for each query point
is_point_inside, clip_start, clip_end and split_at); `f32` `calc_arc_radian(s, e, c, ccw)`,
`calc_arc_radius(s, c)` and `calc_arc_length(s, e, c, ccw)`; `u8` `ray_intersects_segment(origin,
direction, Line(a, b))`. 8 hand-made cases (quarter arcs both ways, arcs across angle 0 both ways,
a zero radius, start equal to end, an unknown direction, a full circle for calc_arc_radian), then
800 random: radii 1e3 to 1e9, directions mostly CCW or CW, 3 to 10 slice points along the arc,
clean or noisy, now and then in reverse order, up to 4 queries, the ray from the center through
the start or drawn (the committed set those and the first 16 of at most 3 KB).

### PSAF

geom-1 (`orca-geom`): `ArcFitter`. Inputs: the points, `f64` tolerance. Results: the fitting
result of `do_arc_fitting(points, tolerance)`; then of `do_arc_fitting_and_simplify` on a copy of
the points, the points it leaves and its fitting result. 6 hand-made cases (no, one and two points,
a straight run, a half circle of 12 points, the same at tolerance 0), then 1200 random arc fitter
paths (straight runs, tangent arcs clean or noisy either way round, zigzags; at most 60 points),
at `DEFAULT_SCALED_RESOLUTION`, a drawn tolerance, or now and then 0 (the committed set those and
the first 16 of at most 3 KB).

### PSPF

geom-1 (`orca-geom`): Polyline's fitting results (`Polyline.cpp` 152-156 and 408-517). Inputs: the
points (three or more), `f64` tol, the second points (two or more), `f64` tol2, a point q, `u64`
k, `f64` dist and `f64` frac. pl is `Polyline(points)` after `simplify_by_fitting_arc(tol)`, pl2
`Polyline(second points)` after `simplify_by_fitting_arc(tol2)`. Results, each a Polyline with its
fit: pl; a copy after `reverse()`; after `clip_end(dist)`; after `clip_start(dist)`; `u8`
`split_at_index(k)` and its two parts; q after `split_at(q)` and its two parts; `u8`
`split_at_length(pl.length() * frac)` and its two parts; pl2; pl after `append(pl2)` (the const
overload); pl after `append(std::move(copy of pl2))`, then that copy; pl after `append(q)`; pl
after `append_before(q)`; `Polyline(second points)` (no fit) after `append(pl)`; pl after
`append(second points)`. 3 hand-made cases (a half circle with a tail, split at a vertex, with a
second arc; the same with a short second line and a far point; three collinear points), then 800
random arc fitter paths, the second's first point now and then the first's last, q a vertex or a
point near, k up to the point count, dist up to 1.1 times the length (the committed set those and
the first 4 of at most 6 KB).

### PSTP

geom-1 (`orca-geom`): ThickPolyline (`Polyline.cpp` 637-659). Inputs: the points, the widths (a
`u32` count and `f64`s, two per segment), `u8` the two endpoints flags, `i32` index. Results:
`thicklines()`, a `u32` count, each a, b, `f64` a_width and b_width; then after
`start_at_index(index)` the points and the widths (a `u32` count and `f64`s). 4 hand-made cases (a
single segment, a closed triangle with matching and with differing end widths, an open square),
then 300 random stars of 2 to 16 points, half of them closed, mostly with the closing width equal
to the first (the committed set those and the first 16 of at most 1 KB).

### PSP3

geom-2 (`orca-geom`): Point3, Line3, BoundingBox3 and MultiPoint3 (`Point.hpp`, `Point.cpp`,
`Line.hpp`, `BoundingBox.hpp`, `MultiPoint.cpp`). Inputs: Point3s p, a, b and c, `f64` angle,
factor, t and eps, a list of points pts2, a list of Point3s pts3, a `Vec3d` v, `i64` k. Results:
`p.rotated(angle)`, `p.rotated(angle, c)`, `p.rotate_90_degree_ccw()`; `p.operator*(factor)` (the
member, which rounds), `Slic3r::operator*(p, factor)` (the free one, which truncates), p after
`*= factor`; `f64` `p.ccw(a, b)`, `p.ccw(Line3(a, b))`, `p.ccw_angle(a, b)`;
`p.projection_onto(Line3(a, b))`; `i32` `p.nearest_point_index(pts2)`; `u8` and the Point3 of
`p.nearest_point(pts2, &out)` (out starts at (0, 0, 0)); `f64` `p.distance_to(a)`; `p.to_point()`;
`u8` `p.both_comp(a, ">")`, `both_comp(a, "<")`, `any_comp(a, ">")`, `any_comp(a, "<")`,
`any_comp(c.x, ">")` and `any_comp(c.x, "<")`; for l = `Line3(a, b)`: `f64` `length()`,
`vector()`, `midpoint()`, `to_line()`'s a and b, `f64` `Line3::distance_to_squared(p, a, b)`;
`lerp(a, b, t)`; `Point3::new_scale(v)`, `new_scale` of v as a `Vec3f`, `new_scale(v.x, v.y,
v.z)`; `to_points(pts3)`; `to_points3(pts2)`; `BoundingBox3(pts3)`'s min, max and `u8` defined;
for pl = `Polyline3(pts3)`: `f64` `length()`, `bounding_box()`'s min, max and `u8` defined, then
for f in (a, pts3[k mod size], or a when pts3 is empty) `i32` `find_point(f.to_point())`,
`find_point(f.to_point(), eps)`, `find_point(f)` and `find_point(f, eps)`; the points after
`translate(c)`, after `rotate(angle)`, after `rotate(angle, c)`; `u8` and the points of
`remove_duplicate_points()`; `lines()` (a `u32` count, each a and b); `i32` and the Point3 of
`foot_pt(pts3, p)`; `u8` threw and `str` the text of `p.projection_onto(pl)` (the MultiPoint3
overload, which throws "Point3::projection_onto(MultiPoint3) not implemented yet"). 2 hand-made
cases, then 600 random in ranges up to 3e8 with z within 2e6, now and then b = a or p = a, pts2 of
up to 8 points and pts3 of up to 10 with a repeated point (the committed set those and the first
16 of at most 2 KB).

### PSD3

geom-2 (`orca-geom`): the 3D Douglas-Peucker. Inputs: the Point3s, `f64` tolerance. Results:
`MultiPoint3::_douglas_peucker(points, tolerance)`; the points of `Polyline3(points)` after
`simplify(tolerance)`. 4 hand-made cases (none, one point, a peak in z, a closed path at tolerance
0), then 800 random arc fitter paths lifted to 3D (flat or with z drawn), up to 60 points, now and
then with a repeated point, at tolerance 0 or 1e1 to 3e6 (the committed set those and the first 16
of at most 2 KB).

### PSL3

geom-2 (`orca-geom`): Polyline3 (`Polyline.cpp` 661-1042). Inputs: the Point3s (three or more),
the second Point3s (two or more), `f64` tol and tol2, a point q2, a Point3 q3, `i64` k, `f64` dist
and `f64` frac. The same steps run twice: on raw = `Polyline3(points)` and raw2 =
`Polyline3(second)`, with no fit, then on fit and fit2, copies after `simplify_by_fitting_arc(tol)`
and `(tol2)` (which drops z: the fitter works on the points' x and y). Each run's results, each a
Polyline3 with its fit: the base; a copy after `reverse()`; after `clip_end(dist)`; `u8`
`split_at_index(k)` and its two parts; q2 after `split_at(q2)` (the 2D overload) and its two
parts; q3 after `split_at(q3)` (the 3D one) and its two parts; `u8`
`split_at_length(length() * frac)` and its two parts; the second; the base after `append(second)`;
after `append(q3)`; after `append_before(q3)`; the points of `to_polyline()`. Then raw after
`append(fit2)` (a fake linear move put before the fit). 1 hand-made case (a half circle with a
tail, split at a vertex), then 600 random lifted arc fitter paths (the committed set those and the
first 3 of at most 8 KB).

### PSC3

geom-2 (`orca-geom`): the Points3 forms of Circle's and ArcSegment's fits. Inputs: the Point3s
(two or more), `f64` max radius, tolerance, approximate length and path tolerance percent.
Results: `u8` and the circle of `Circle::try_create_circle(points3, ...)` (from a default
circle); `u8` and the arc of `ArcSegment::try_create_arc(points3, ...)` (from a default arc); `u8`
`are_points_within_slice(arc, points3)` (0 when no arc); then for each direction, CCW then CW, of
a = `ArcSegment(circle.center, radius or 0 when no circle, first point, last point, direction)`:
`u8` `a.is_valid()`, `u8` `are_points_within_slice(a, points3)` and `u8` the same of the points
as 2D points (0 when a is not valid or there are fewer than three points). 2 hand-made cases (a
quarter circle in 3D, two points), then 800 random arcs and lifted arc fitter paths (the
committed set those and the first 24 of at most 2 KB).

### PSO3

geom-2 (`orca-geom`): `offset` of a Polyline3 (`ClipperUtils.cpp` 415-426). Inputs: the Point3s,
`f32` delta, `u8` join type, `f64` miter limit, `u8` end type (`EndType`). Result: the polygon list
`offset(Polyline3(points), delta, join, miter, end)`. 3 hand-made cases (a segment with square
ends, an L with miter joins and round ends, round joins with butt ends), then 400 random lifted
paths of 2 to 20 points, delta 1e3 to 1e6, each join and end type (the committed set those and the
first 16 of at most 3 KB).

### PSLN

paths-1 (`orca-paths`): `chain_lines`. Inputs: the lines (a `u32` count, each a and b), `f64`
epsilon, `f64` the first epsilon. chain_lines keeps the square of the epsilon of the process's
first call in a function-local static and compares every later call against it, while its lookup
uses each call's own epsilon: the program calls it first in PSLN's first record, with 10000, so
"the first epsilon" is 10000 in every record. Result: the polylines. 5 hand-made cases (three
lines joining within the epsilon, one turned round; none; one degenerate line; a closed triangle;
two lines 50000 apart with an epsilon of 100000), then 400 random: a few polylines cut into
lines, each line's start moved by up to 15000 (or not at all), shuffled, some turned round, at the
first epsilon or now and then another (the committed set those and the first 16 of at most 2 KB).
No code in domain A calls it.

### PSCP

paths-1 (`orca-paths`): `chain_points`, which runs `chain_segments_greedy` (the greedy chaining
with no reversal constraint, its KD tree, priority queue and equivalent chains). Inputs: the
points, `u8` a start point given and the start point ((0, 0) when not). Result: the order. 8
hand-made cases (none, one, two with and without a start, a 3x3 grid with and without a start at
its center, four equal points, six collinear points from their end), then 1200 random sets of 2 to
40 points, a tenth of 60 to 250: scattered, on grids (ties), in clusters far apart, collinear, or
with duplicates, half with a start (the committed set those and the first 32 of at most 2 KB).

### PSCX

paths-1 (`orca-paths`): `chain_expolygons` (each ExPolygon's box center, then `chain_points`).
Inputs: the ExPolygons. Result: the order. 3 hand-made cases (none, one, three equal squares in a
row and a square with a hole), then 600 random sets of 1 to 30 stars, centered as PSCP's points,
some with a hole (the committed set those and the first 16 of at most 2 KB).

### PSXP

paths-1 (`orca-paths`): `chain_extrusion_paths` and `chain_and_reorder_extrusion_paths`. Inputs:
`u8` a start point given and the start point, the paths' points (a `u32` count, each a list of
Point3s, one point or more). Results: the chain of `chain_extrusion_paths(paths, start)`; then the
paths' points after `chain_and_reorder_extrusion_paths(paths, start)` (a `u32` count, each a list
of Point3s). 6 hand-made cases (none, one segment with a start past its end, the same with no
start, three single points, six parallel lines alternating direction with and without a start),
then 900 random sets of 2 to 30 polylines (a tenth of 40 to 120) of the kinds of `segments` in
`parity/oracle/probe/paths/chain.cpp` (infill lines, segments, walks, closed loops, clustered
dashes), now and then a single point, flat or with z drawn (the committed set those and the first
12 of at most 2 KB).

### PSXE

paths-1 (`orca-paths`): `chain_extrusion_entities`, which runs
`chain_segments_greedy_constrained_reversals` (reversals refused for entities that cannot
reverse, the reversed retry when no start is given, and the closest-point fallback,
`chain_segments_closest_point`), and `chain_and_reorder_extrusion_entities`. Inputs: `u8` a start
point given and the start point; the entities, a `u32` count, each `u8` type, `u32` path count
and each path's Point3s. The types: 0 an ExtrusionPath; 1 an ExtrusionPath after `set_reverse()`
(cannot reverse); 2 an ExtrusionPathOriented; 3 an ExtrusionLoop of one closed path; 4 an
ExtrusionLoop of its paths; 5 an ExtrusionMultiPath of its paths; 6 an ExtrusionMultiPath of one
path that cannot reverse; 7 an ExtrusionEntityCollection of its paths; 8 the same with `no_sort`;
9 the same after `set_reverse()`; and, without end points: 10 an ExtrusionPath with no points, 11
an empty collection, 12 a multipath of its paths and an empty one, 13 an ExtrusionLoop with no
paths, 14 a collection of its paths and an empty path. Results: `u8` every entity has end points;
when so, the chain of `chain_extrusion_entities(entities, start)`; then, on a fresh set,
`chain_and_reorder_extrusion_entities(entities, start)`: a `u32` count, each `u32` the entity's
index in the input, its `first_point()` and `last_point()` after the reorder. 7 hand-made cases
(every type with and without a start, five that cannot reverse in a row that wants reversals with
and without a start, the empty types among others, none, one that cannot reverse with a start at
its end), then 900 random sets of 1 to 24 entities from `segments`, closed ones as loops or
collections, a third mostly unable to reverse, a fifth with some empty ones (the committed set
those and the first 14 of at most 3 KB).

### PSPL

paths-1 (`orca-paths`): `chain_polylines`: `chain_segments_greedy2`, then, with no start point
and more than one polyline, `improve_ordering_by_two_exchanges_with_segment_flipping` (its sort of
equal connection lengths shows in the ties). Inputs: `u8` a start point given and the start point,
the polylines (one point or more each). Result: the polylines as it returns them. 5 hand-made cases
(none, one with a start at its end, eight parallel lines with and without a start, two closed
squares and a single point), then 900 random sets of 2 to 40 polylines (a tenth of 40 to 150) of
`segments`' kinds, a third with a start (the committed set those and the first 14 of at most 2 KB).

### PSUO

paths-1 (`orca-paths`): `union_pt_chained_outside_in` (`ClipperUtils.cpp` 992-1017): `union_pt`,
then `traverse_pt_outside_in`, which orders each level with `chain_clipper_polynodes` and turns
holes counter-clockwise. Input: the polygons. Result: the polygons. 3 hand-made cases (none, five
nested squares alternately oriented as concentric fill's loops, the same with a second island of
three), then 500 random: one to four islands of one to five nested squares or 24-gons, inner ones
now and then reversed, sometimes with a star over them (the committed set those and the first 10
of at most 3 KB).
