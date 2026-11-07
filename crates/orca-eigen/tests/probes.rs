// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The probe gate for orca-eigen: the Eigen records of `geom.sop1` (`parity/SOP1.md`,
//! "geom.sop1"), which `parity/oracle/probe/geom/eigen.cpp` recorded from the engine's own build of
//! Eigen 5.0.1, replayed against the crate bit for bit (added 2026-11-07 by o2-geom):
//!
//! - EVEC: the vector operations of `Vec2`, `Vec3` and `Vec4` of `double` and `float`, and of
//!   `Vec2` and `Vec3` of `int64_t` and `int32_t`: reductions, coefficient-wise operations, norms,
//!   cross products and casts, on inputs with signed zeros, infinities and the default NaN;
//! - EMAT: the products, reductions, determinant and inverse of 2x2, 3x3 and 4x4 matrices;
//! - ETRF: `Transform3d` built as `Geometry::assemble_transform` builds it, its products, inverse,
//!   `computeRotationScaling`, the Euler angles of its rotation, its pre-operations, and the same
//!   as `Transform3f`;
//! - EROT: `AngleAxis` and `Quaternion` conversions and products, and `eulerAngles`;
//! - ESVD: `JacobiSVD<Matrix3d>` with full U and V.
//!
//! orca-geom replays the file's other records. The committed subset (`parity/probes/`) runs by
//! default; `SKETCH_ORCA_PROBES_DEEP=<folder>` replays the full set from that folder
//! (`<work>/parity/probes`).

use orca_eigen::{
    AngleAxis, AngleAxisd, Cast, JacobiSvd3, Mat3, Matrix, Quaternion, Quaterniond, Real, Scalar,
    Transform3d, Vec3d, Vector, euler_angles,
};
use orca_probe::{Mismatches, ProbeFile, Reader, load_probe};

fn file() -> ProbeFile {
    load_probe("geom").unwrap_or_else(|e| panic!("{e}"))
}

/// A coefficient as the probe writes it: its bits in a `u64` (a `float`'s zero-extended, an
/// `int32_t` sign-extended).
trait Coef: Scalar {
    fn to_u64(self) -> u64;
    fn from_u64(b: u64) -> Self;
}

impl Coef for f64 {
    fn to_u64(self) -> u64 {
        self.to_bits()
    }
    fn from_u64(b: u64) -> Self {
        f64::from_bits(b)
    }
}

impl Coef for f32 {
    fn to_u64(self) -> u64 {
        u64::from(self.to_bits())
    }
    #[allow(clippy::cast_possible_truncation)]
    fn from_u64(b: u64) -> Self {
        f32::from_bits(b as u32)
    }
}

impl Coef for i64 {
    #[allow(clippy::cast_sign_loss)]
    fn to_u64(self) -> u64 {
        self as u64
    }
    #[allow(clippy::cast_possible_wrap)]
    fn from_u64(b: u64) -> Self {
        b as i64
    }
}

impl Coef for i32 {
    #[allow(clippy::cast_sign_loss)]
    fn to_u64(self) -> u64 {
        i64::from(self) as u64
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn from_u64(b: u64) -> Self {
        b as i64 as i32
    }
}

/// One record being replayed: its fields read in order, each result compared with the crate's.
struct Case<'a, 'm> {
    r: Reader<'a>,
    m: &'m mut Mismatches,
    label: String,
}

impl<'a, 'm> Case<'a, 'm> {
    fn new(r: Reader<'a>, m: &'m mut Mismatches, label: String) -> Self {
        Self { r, m, label }
    }

    fn raw(&mut self) -> u64 {
        let label = &self.label;
        self.r.u64().unwrap_or_else(|e| panic!("{label}: {e}"))
    }

    fn input<T: Coef>(&mut self) -> T {
        T::from_u64(self.raw())
    }

    fn vector<T: Coef, const N: usize>(&mut self) -> Vector<T, N> {
        Vector(core::array::from_fn(|_| self.input()))
    }

    fn matrix<T: Coef, const R: usize, const C: usize>(&mut self) -> Matrix<T, R, C> {
        let mut m = Matrix::zero();
        for c in 0..C {
            for r in 0..R {
                m.set(r, c, self.input());
            }
        }
        m
    }

    /// One result: the probe's bits against `got`'s.
    fn one<T: Coef>(&mut self, field: &str, got: T) {
        self.at(field, None, got);
    }

    fn at<T: Coef>(&mut self, field: &str, index: Option<usize>, got: T) {
        let want = self.raw();
        let label = &self.label;
        self.m.check(want == got.to_u64(), || {
            let place = index.map(|i| format!("[{i}]")).unwrap_or_default();
            format!(
                "{label} {field}{place}: {got:?} ({:#x}), the C++ engine {:?} ({want:#x})",
                got.to_u64(),
                T::from_u64(want)
            )
        });
    }

    /// Coefficients in order: a vector's, or a matrix's column by column.
    fn all<T: Coef>(&mut self, field: &str, got: &[T]) {
        for (i, &g) in got.iter().enumerate() {
            self.at(field, Some(i), g);
        }
    }

    fn vec<T: Coef, const N: usize>(&mut self, field: &str, got: &Vector<T, N>) {
        self.all(field, &got.0);
    }

    fn mat<T: Coef, const R: usize, const C: usize>(&mut self, field: &str, got: &Matrix<T, R, C>) {
        self.all(field, &got.storage());
    }

    /// A count or an index, written as a `uint64_t`.
    fn count(&mut self, field: &str, got: usize) {
        self.one(field, i64::try_from(got).unwrap_or(-1));
    }

    fn flag(&mut self, field: &str, got: bool) {
        self.one(field, i64::from(got));
    }

    fn finish(self) {
        self.r.finish(&self.label).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// EVEC's fields for every type: the inputs, reductions and coefficient-wise operations.
fn evec_common<T: Coef, const N: usize>(c: &mut Case) -> (Vector<T, N>, Vector<T, N>) {
    let a: Vector<T, N> = c.vector();
    let b: Vector<T, N> = c.vector();
    c.one("dot", a.dot(&b));
    c.one("squaredNorm", a.squared_norm());
    c.one("sum", a.sum());
    c.one("prod", a.prod());
    c.one("minCoeff", a.min_coeff());
    c.one("maxCoeff", a.max_coeff());
    c.one("(a-c).squaredNorm", (a - b).squared_norm());
    c.count("minCoeff index", a.min_coeff_index().1);
    c.count("maxCoeff index", a.max_coeff_index().1);
    c.vec("cwiseMin", &a.cwise_min(&b));
    c.vec("cwiseMax", &a.cwise_max(&b));
    c.vec("cwiseAbs", &a.cwise_abs());
    c.vec("cwiseProduct", &a.cwise_product(&b));
    c.vec("a+c", &(a + b));
    c.vec("a-c", &(a - b));
    (a, b)
}

/// EVEC's fields for `double` and `float`.
fn evec_real<T: Coef + Real, const N: usize>(c: &mut Case, a: &Vector<T, N>, b: &Vector<T, N>) {
    c.one("norm", a.norm());
    c.vec("normalized", &a.normalized());
    c.flag("isApprox", a.is_approx(b, T::from_f64(1e-3)));
    c.vec("a*0.375", &(*a * T::from_f64(0.375)));
    c.vec("a/3", &(*a / T::from_f64(3.0)));
}

/// EVEC's casts, the last fields.
fn evec_casts<T, const N: usize>(c: &mut Case, a: &Vector<T, N>)
where
    T: Coef + Cast<f64> + Cast<f32> + Cast<i64> + Cast<i32>,
{
    c.vec("cast<double>", &a.cast::<f64>());
    c.vec("cast<float>", &a.cast::<f32>());
    c.vec("cast<int64_t>", &a.cast::<i64>());
    c.vec("cast<int32_t>", &a.cast::<i32>());
}

fn evec_case(c: &mut Case, ty: u8, n: u8) {
    match (ty, n) {
        (0, 2) => {
            let (a, b) = evec_common::<f64, 2>(c);
            evec_real(c, &a, &b);
            c.one("cross", a.cross2(&b));
            evec_casts(c, &a);
        }
        (0, 3) => {
            let (a, b) = evec_common::<f64, 3>(c);
            evec_real(c, &a, &b);
            c.vec("cross", &a.cross(&b));
            evec_casts(c, &a);
        }
        (0, 4) => {
            let (a, b) = evec_common::<f64, 4>(c);
            evec_real(c, &a, &b);
            evec_casts(c, &a);
        }
        (1, 2) => {
            let (a, b) = evec_common::<f32, 2>(c);
            evec_real(c, &a, &b);
            c.one("cross", a.cross2(&b));
            evec_casts(c, &a);
        }
        (1, 3) => {
            let (a, b) = evec_common::<f32, 3>(c);
            evec_real(c, &a, &b);
            c.vec("cross", &a.cross(&b));
            evec_casts(c, &a);
        }
        (1, 4) => {
            let (a, b) = evec_common::<f32, 4>(c);
            evec_real(c, &a, &b);
            evec_casts(c, &a);
        }
        (2, 2) => {
            let (a, b) = evec_common::<i64, 2>(c);
            c.one("cross", a.cross2(&b));
            evec_casts(c, &a);
        }
        (2, 3) => {
            let (a, b) = evec_common::<i64, 3>(c);
            c.vec("cross", &a.cross(&b));
            evec_casts(c, &a);
        }
        (3, 2) => {
            let (a, b) = evec_common::<i32, 2>(c);
            c.one("cross", a.cross2(&b));
            evec_casts(c, &a);
        }
        (3, 3) => {
            let (a, b) = evec_common::<i32, 3>(c);
            c.vec("cross", &a.cross(&b));
            evec_casts(c, &a);
        }
        _ => panic!("{}: an EVEC of type {ty} and size {n}", c.label),
    }
}

/// The type names EVEC and EMAT number.
fn type_name(ty: u8) -> &'static str {
    ["double", "float", "int64_t", "int32_t"]
        .get(usize::from(ty))
        .copied()
        .unwrap_or("?")
}

#[test]
fn vector_operations_match_eigen() {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind("EVEC").enumerate() {
        let mut r = rec.reader();
        let (ty, n) = (r.u8().unwrap(), r.u8().unwrap());
        let label = format!("EVEC {i} (Vec{n} of {})", type_name(ty));
        let mut c = Case::new(r, &mut m, label);
        evec_case(&mut c, ty, n);
        c.finish();
    }
    m.assert_none("EVEC");
}

/// EMAT's fields for every size, the matrix `a` returned for the 3x3 fields.
fn emat_common<T: Coef + Real, const N: usize>(c: &mut Case) -> Matrix<T, N, N> {
    let a: Matrix<T, N, N> = c.matrix();
    let b: Matrix<T, N, N> = c.matrix();
    let v: Vector<T, N> = c.vector();
    c.mat("a*c", &a.mul(&b));
    c.vec("a*v", &a.mul_vec(&v));
    c.mat("a*c^T", &a.mul(&b.transpose()));
    c.vec("-a*v", &(-a).mul_vec(&v));
    c.mat("a*diag(v)", &a.mul_diagonal(&v));
    c.mat("diag(v)*a", &Matrix::diagonal_mul(&v, &a));
    c.one("sum", a.sum());
    c.one("squaredNorm", a.squared_norm());
    c.one("trace", a.trace());
    c.flag("isDiagonal", a.is_diagonal(T::DUMMY_PRECISION));
    a
}

fn emat_3x3<T: Coef + Real>(c: &mut Case, a: &Mat3<T>) {
    c.one("determinant", a.determinant());
    c.mat("inverse", &a.inverse());
}

#[test]
fn matrix_operations_match_eigen() {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind("EMAT").enumerate() {
        let mut r = rec.reader();
        let (ty, n) = (r.u8().unwrap(), r.u8().unwrap());
        let label = format!("EMAT {i} ({n}x{n} of {})", type_name(ty));
        let mut c = Case::new(r, &mut m, label);
        match (ty, n) {
            (0, 2) => {
                emat_common::<f64, 2>(&mut c);
            }
            (0, 3) => {
                let a = emat_common::<f64, 3>(&mut c);
                emat_3x3(&mut c, &a);
            }
            (0, 4) => {
                emat_common::<f64, 4>(&mut c);
            }
            (1, 3) => {
                let a = emat_common::<f32, 3>(&mut c);
                emat_3x3(&mut c, &a);
            }
            (1, 4) => {
                emat_common::<f32, 4>(&mut c);
            }
            _ => panic!("{}: an EMAT of type {ty} and size {n}", c.label),
        }
        c.finish();
    }
    m.assert_none("EMAT");
}

/// `Geometry::assemble_transform(t, r, s)`: translate, rotate by the Z, Y and X angle-axes'
/// quaternion product, scale.
fn assemble(t: &Vec3d, r: &Vec3d, s: &Vec3d) -> Transform3d {
    let mut tr = Transform3d::identity();
    tr.translate(t);
    let q = AngleAxisd::new(r[2], Vec3d::unit_z())
        .mul(&AngleAxisd::new(r[1], Vec3d::unit_y()))
        .mul_angle_axis(&AngleAxisd::new(r[0], Vec3d::unit_x()));
    tr.rotate_quaternion(&q);
    tr.scale(s);
    tr
}

#[test]
fn transforms_match_eigen() {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind("ETRF").enumerate() {
        let mut c = Case::new(rec.reader(), &mut m, format!("ETRF {i}"));
        let [t1, r1, s1, t2, r2, s2, pt]: [Vec3d; 7] = core::array::from_fn(|_| c.vector());
        let (a, b) = (assemble(&t1, &r1, &s1), assemble(&t2, &r2, &s2));
        c.mat("a", &a.m);
        c.mat("c", &b.m);
        c.mat("a*c", &a.mul(&b).m);
        c.vec("a*pt", &a.mul_point(&pt));
        c.mat("a.inverse()", &a.inverse().m);
        c.vec("a.linear()*pt", &a.linear().mul_vec(&pt));
        let (rot, scl) = a
            .compute_rotation_scaling()
            .unwrap_or_else(|| panic!("{}: no SVD of a finite matrix", c.label));
        c.mat("rotation", &rot);
        c.mat("scaling", &scl);
        c.vec("rotation.eulerAngles(2,1,0)", &euler_angles(&rot, 2, 1, 0));
        let mut x = a;
        x.pretranslate(&pt);
        c.mat("pretranslate", &x.m);
        let mut x = a;
        x.prerotate(&AngleAxisd::new(r2[0], Vec3d::unit_x()).to_rotation_matrix());
        c.mat("prerotate", &x.m);
        let mut x = a;
        x.prescale(&s2);
        c.mat("prescale", &x.m);
        let mut x = a;
        x.scale_uniform(s2[0]);
        c.mat("scale(s)", &x.m);
        let af = a.cast::<f32>();
        let pf = pt.cast::<f32>();
        c.mat("a.cast<float>()", &af.m);
        c.vec("af*pf", &af.mul_point(&pf));
        c.vec("af.linear()*pf", &af.linear().mul_vec(&pf));
        c.mat("af*c.cast<float>()", &af.mul(&b.cast::<f32>()).m);
        c.finish();
    }
    m.assert_none("ETRF");
}

#[test]
fn rotations_match_eigen() {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind("EROT").enumerate() {
        let mut c = Case::new(rec.reader(), &mut m, format!("EROT {i}"));
        let (a1, a2): (f64, f64) = (c.input(), c.input());
        let (ax1, ax2): (Vec3d, Vec3d) = (c.vector(), c.vector());
        let tiny: f64 = c.input();
        let rnd: Mat3<f64> = c.matrix();
        let (aa1, aa2) = (AngleAxisd::new(a1, ax1), AngleAxisd::new(a2, ax2));
        let r1 = aa1.to_rotation_matrix();
        c.mat("aa1.toRotationMatrix()", &r1);
        let (q1, q2) = (
            Quaterniond::from_angle_axis(&aa1),
            Quaterniond::from_angle_axis(&aa2),
        );
        c.all("q1", &q1.coeffs);
        c.all("q1*q2", &q1.mul(&q2).coeffs);
        c.all("aa1*aa2", &aa1.mul(&aa2).coeffs);
        let af1 = AngleAxis::new(Cast::<f32>::cast(a1), ax1.cast::<f32>());
        let af2 = AngleAxis::new(Cast::<f32>::cast(a2), ax2.cast::<f32>());
        let qf = Quaternion::from_angle_axis(&af1).mul(&Quaternion::from_angle_axis(&af2));
        c.all("Quaternionf product", &qf.coeffs);
        c.mat("q1.toRotationMatrix()", &q1.to_rotation_matrix());
        c.all(
            "Quaterniond(r1)",
            &Quaterniond::from_rotation_matrix(&r1).coeffs,
        );
        c.all(
            "Quaterniond(rnd)",
            &Quaterniond::from_rotation_matrix(&rnd).coeffs,
        );
        let back = AngleAxisd::from_quaternion(&q1);
        c.one("AngleAxisd(q1).angle", back.angle);
        c.vec("AngleAxisd(q1).axis", &back.axis);
        let from = AngleAxisd::from_rotation_matrix(&r1);
        c.one("AngleAxisd(r1).angle", from.angle);
        c.vec("AngleAxisd(r1).axis", &from.axis);
        let near =
            AngleAxisd::from_quaternion(&Quaterniond::new(1.0, tiny, -tiny * 0.5, tiny * 0.25));
        c.one("near.angle", near.angle);
        c.vec("near.axis", &near.axis);
        let q = Quaterniond::new(rnd.at(0, 0), rnd.at(1, 0), rnd.at(2, 0), rnd.at(0, 1));
        c.all("normalized", &q.normalized().coeffs);
        c.vec("r1.eulerAngles(2,1,0)", &euler_angles(&r1, 2, 1, 0));
        c.vec("r1.eulerAngles(0,1,2)", &euler_angles(&r1, 0, 1, 2));
        c.vec("r1.eulerAngles(2,0,2)", &euler_angles(&r1, 2, 0, 2));
        c.vec("rnd.eulerAngles(1,0,1)", &euler_angles(&rnd, 1, 0, 1));
        c.finish();
    }
    m.assert_none("EROT");
}

#[test]
fn svds_match_eigen() {
    let file = file();
    let mut m = Mismatches::default();
    for (i, rec) in file.of_kind("ESVD").enumerate() {
        let mut c = Case::new(rec.reader(), &mut m, format!("ESVD {i}"));
        let a: Mat3<f64> = c.matrix();
        let svd = JacobiSvd3::compute(&a)
            .unwrap_or_else(|| panic!("{}: no SVD of a finite matrix", c.label));
        c.mat("U", &svd.u);
        c.mat("V", &svd.v);
        c.vec("singularValues", &svd.singular_values);
        c.count("nonzeroSingularValues", svd.nonzero);
        c.finish();
    }
    m.assert_none("ESVD");
}

/// The file holds every Eigen kind this file replays; orca-geom's tests/probes.rs replays the rest.
#[test]
fn every_eigen_kind_is_recorded() {
    let file = file();
    for kind in ["EVEC", "EMAT", "ETRF", "EROT", "ESVD"] {
        assert!(
            file.of_kind(kind).next().is_some(),
            "{}: no {kind} record",
            file.path.display()
        );
    }
    assert_eq!(orca_eigen::NAME, "orca-eigen");
}
