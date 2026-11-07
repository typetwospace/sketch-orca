# orca-eigen

The Eigen 5.0.1 operations libslic3r's geometry reaches, evaluated as MSVC 14.42 compiles them.

The C++ engine compiles Eigen 5.0.1 (its headers number it 3.5.0; the dependency build's `<work>/deps/dep_Eigen-prefix/src/dep_Eigen`) with `/O2`, `/fp:precise` and no `/arch`, so Eigen vectorises with SSE2: packets of 2 `double`s, 4 `float`s, 2 `int64_t`s or 4 `int32_t`s. Floating-point addition is not associative, so the order in which those packets reduce is part of every result. This crate gives the same bits as that build, for the fixed-size types libslic3r uses (`Point.hpp`, all `DontAlign`):

| Module | What it holds |
| --- | --- |
| `scalar` | `Scalar` (the element types and their packet sizes) and `Real` (`float`, `double`: epsilon, `dummy_precision`, and maths through `orca-crt`, the static UCRT's); `hypot` as Eigen's own; `Cast`, each `static_cast` (float to integer through `orca-crt`) |
| `redux` | the orders: `redux` (sum, `squaredNorm`, `minCoeff`, `maxCoeff`), `dot`, `find_coeff` (the index forms of `minCoeff` and `maxCoeff`), `product_coeff` and `packet_rows` (small matrix products) |
| `vector` | `Vector<T, N>`: `Vec2d`, `Vec3d`, `Vec3f`, `Vec2crd`, `Vec3crd`, `Vec3i32` and the rest, with `norm`, `normalized`, `dot`, `cross`, the coefficient-wise operations, the reductions and `cast` |
| `matrix` | `Matrix<T, R, C>`, column-major: `Matrix3d`, `Matrix4d`, products by matrices, vectors and diagonals, `transpose`, `trace`, `isDiagonal`, the 3x3 `determinant` and `inverse` |
| `rotation` | `Quaternion` (the SSE2 product), `AngleAxis`, their conversions to and from rotation matrices, `stableNorm` of a 3-vector, `eulerAngles` |
| `transform` | `Transform3d` and `Transform3f` (`Transform<T, 3, Affine, DontAlign>`): `translate`, `rotate`, `scale` and their `pre` forms, products, `inverse`, `linear()`, `computeRotationScaling` |
| `svd` | `JacobiSVD` of a 3x3 matrix with full U and V, as `computeRotationScaling` uses it |

## How Eigen orders the arithmetic

- **Reductions** (`sum`, `squaredNorm`, `norm`, `minCoeff`, `maxCoeff`). With packet access and at least one whole packet: the packets combine in a tree, the lanes fold (`a0 + a1`; four lanes `(a0 + a2) + (a1 + a3)`), and the coefficients after the last whole packet, a tree of their own, are joined last. Otherwise one tree, the first half the smaller. So `squaredNorm` of a `Vec3d` is `(x0² + x1²) + x2²` and of a `Vec3f` `x0² + (x1² + x2²)`. A diagonal, or a row of a column-major matrix, has no packet access (the `*_strided` forms).
- **Dot products** (InnerProduct.h): up to four packet accumulators, then the lanes, then the rest one by one; a dot of 2 or 3 coefficients is left to right, of a `Vec4d` `(a0b0 + a2b2) + (a1b1 + a3b3)`.
- **Index searches** (`minCoeff(&i)`, `maxCoeff(&i)`, FindCoeff.h): by whole packets, then one by one. They give the first extreme coefficient, as a plain scan does, except where a NaN shares a packet with a better coefficient: the packet's reduction can be the NaN, and the index then stays on that packet's first coefficient.
- **Small products** (`LazyCoeffBasedProductMode`): the rows of each column that fill whole packets take the packet path, left to right (`((t0 + t1) + t2) + t3`); the rest take the coefficient path, a tree. A 3-row `double` result so has rows 0 and 1 left to right and row 2 `t0 + (t1 + t2)`; 4-row results are left to right throughout; a 3-row `float` result is a tree in every row. `Transform * point` multiplies the whole 4x4 matrix by `(p, 1)`, so all three rows are left to right.
- **Quaternion products** use Eigen's SSE2 kernels (Geometry_SIMD.h), whose order differs from the scalar formula's, one for `double` and one for `float`.
- **Coefficient-wise operations** compute each coefficient alone, so packets change nothing; `cwiseMin` and `cwiseMax` are `std::min` and `std::max` per coefficient (`orca_stl::min`, `orca_stl::max`).
- **`JacobiSVD` of a matrix holding a NaN or an infinity** leaves U, V and the singular values unset in C++ (undefined values); `JacobiSvd3::compute` and `Transform3::compute_rotation_scaling` return `None` there, and the caller handles it.

## Probe gate

`tests/probes.rs` replays the Eigen records of `geom.sop1` (`parity/SOP1.md`, "geom.sop1"), which `bun run engine parity probe --only geom` writes from the C++ engine's own build (`parity/oracle/probe/geom/eigen.cpp`): EVEC (vectors of `double`, `float`, `int64_t` and `int32_t`), EMAT (2x2, 3x3 and 4x4 matrices), ETRF (`Transform3d` and `Transform3f`), EROT (`AngleAxis`, `Quaternion`, `eulerAngles`) and ESVD (`JacobiSVD`), bit for bit. The committed subset in `parity/probes/` runs by default, the full set with `SKETCH_ORCA_PROBES_DEEP=<work>/parity/probes`. On 2026-11-07 every comparison of both sets matched, debug and release. orca-geom replays the file's other records.

Part of the Rust translation of sketch-orca (the workspace in the repository root; see the
repository's `README.md`, `BUILDING.md` and `CONTRIBUTING.md`). Licence: AGPL-3.0-only; the Eigen
code it carries stays under the MPL-2.0, which `NOTICE` names in the crate's own section and each
carrying file's header repeats (Exhibit A).

## Dependencies

Declared up front, before the crate is filled (o2-geom, 2026-11-07), so that the tasks that fill the O2 crates in parallel never change `Cargo.toml` or `Cargo.lock`:

- `orca-crt`: the UCRT's `sin`, `cos`, `atan2`, `pow` and their `float` forms, and the float-to-integer casts
- `orca-stl`: `std::min` and `std::max`
- `orca-probe` (development only: the probe gate)
