// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries OrcaSlicer code: the expr struct and its operators, compare_op, function_2params, digits
// and logical_op of src/libslic3r/PlaceholderParser.cpp of OrcaSlicer v2.4.2, translated on
// 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the
// PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License, version 3
// (AGPL-3.0; LICENSE).

//! The placeholder parser's values (`client::expr`): empty, a bool, an `int`, a `double` or a
//! string, each with the range of the template it came from, and the operators the grammar's
//! actions apply to them, with the C++ code's errors.
//!
//! A range's begin is where an error about the value points; `None` stands for the C++ code's
//! default-constructed (null) iterator, which a value that came from `expr()` (an operator on an
//! empty value), `one_of` or `interpolate_table` carries. An error there makes the C++
//! `process_error_message` build a string from a null iterator, which throws `std::length_error`
//! past the caller's catch; here it declines ([`Fail::Unsupported`]).

use orca_fmt::ostream::{Stream, f64_to_string};
use orca_fmt::printf::{Arg, sprintf};

/// A position in the template, or the null iterator.
pub type Pos = Option<usize>;

/// What stops a parse: Spirit's `expectation_failure` (an error position and the `info` tag, a
/// free-form message starting with `*`), or a case this translation does not take.
#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    /// `qi::expectation_failure`, which the start rule's `on_error` turns into the message.
    Expect {
        /// `x.first`.
        at: Pos,
        /// `x.what_.tag`.
        tag: String,
    },
    /// Undefined or unported behaviour of the C++ code: the job declines.
    Unsupported(String),
}

impl Fail {
    /// `throw_exception(message, range)`: an expectation failure at `at` with `*message`.
    pub fn at(at: Pos, message: &str) -> Fail {
        Fail::Expect {
            at,
            tag: format!("*{message}"),
        }
    }

    /// A case the translation declines.
    pub fn unsupported(what: impl Into<String>) -> Fail {
        Fail::Unsupported(what.into())
    }
}

/// `expr::Type` with its data.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `TYPE_EMPTY`: inside a branch being skipped, or no value.
    Empty,
    /// `TYPE_BOOL`.
    Bool(bool),
    /// `TYPE_INT` (`int`).
    Int(i32),
    /// `TYPE_DOUBLE`.
    Double(f64),
    /// `TYPE_STRING` (bytes, as `std::string`).
    Str(Vec<u8>),
}

/// `client::expr`: a value and its range.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    /// The value.
    pub v: Value,
    /// `it_range.begin()`.
    pub begin: Pos,
    /// `it_range.end()`.
    pub end: Pos,
}

impl Default for Expr {
    /// `expr()`: empty, with a null range.
    fn default() -> Expr {
        Expr {
            v: Value::Empty,
            begin: None,
            end: None,
        }
    }
}

impl Expr {
    /// A value with a range.
    pub fn new(v: Value, begin: Pos, end: Pos) -> Expr {
        Expr { v, begin, end }
    }

    /// `throw_exception(message)`: at this value's begin.
    pub fn throw(&self, message: &str) -> Fail {
        Fail::at(self.begin, message)
    }

    /// `numeric_type()`.
    pub fn numeric(&self) -> bool {
        matches!(self.v, Value::Int(_) | Value::Double(_))
    }

    /// `throw_if_not_numeric(message)`.
    pub fn need_numeric(&self, message: &str) -> Result<(), Fail> {
        if self.numeric() {
            Ok(())
        } else {
            Err(self.throw(message))
        }
    }

    /// `as_d()` of a numeric value.
    pub fn as_d(&self) -> f64 {
        match self.v {
            Value::Double(d) => d,
            Value::Int(i) => f64::from(i),
            _ => unreachable!("as_d of a value checked numeric"),
        }
    }

    /// `as_i()` of a numeric value: a double truncated as MSVC's `int(d)` does.
    pub fn as_i(&self) -> i32 {
        match self.v {
            Value::Int(i) => i,
            Value::Double(d) => orca_crt::f64_to_i32(d),
            _ => unreachable!("as_i of a value checked numeric"),
        }
    }

    /// `as_i_rounded()` of a numeric value.
    pub fn as_i_rounded(&self) -> i32 {
        match self.v {
            Value::Int(i) => i,
            Value::Double(d) => orca_crt::f64_to_i32(orca_crt::round(d)),
            _ => unreachable!("as_i_rounded of a value checked numeric"),
        }
    }

    /// `to_string()`: `std::to_string` of an int, `ostringstream << d` of a double.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Fail> {
        Ok(match &self.v {
            Value::Empty => Vec::new(),
            Value::Bool(b) => if *b { "true" } else { "false" }.into(),
            Value::Int(i) => i.to_string().into_bytes(),
            Value::Double(d) => f64_to_string(&mut Stream::default(), *d)
                .map_err(|_| Fail::unsupported("a double ostringstream cannot print"))?
                .into_bytes(),
            Value::Str(s) => s.clone(),
        })
    }

    /// `reset()`: empty, the range kept.
    pub fn reset(&mut self) {
        self.v = Value::Empty;
    }

    /// `unary_minus(start_pos)`.
    pub fn unary_minus(&self, start: Pos) -> Result<Expr, Fail> {
        match self.v {
            Value::Empty => Ok(Expr::default()),
            Value::Int(i) => Ok(Expr::new(Value::Int(i.wrapping_neg()), start, self.end)),
            Value::Double(d) => Ok(Expr::new(Value::Double(-d), start, self.end)),
            _ => Err(self.throw("Cannot apply unary minus operator.")),
        }
    }

    /// `unary_integer(start_pos)`: `int(...)`.
    pub fn unary_integer(&self, start: Pos) -> Result<Expr, Fail> {
        match self.v {
            Value::Empty => Ok(Expr::default()),
            Value::Int(i) => Ok(Expr::new(Value::Int(i), start, self.end)),
            Value::Double(d) => Ok(Expr::new(
                Value::Int(orca_crt::f64_to_i32(d)),
                start,
                self.end,
            )),
            _ => Err(self.throw("Cannot convert to integer.")),
        }
    }

    /// `round(start_pos)`.
    pub fn round(&self, start: Pos) -> Result<Expr, Fail> {
        match self.v {
            Value::Empty => Ok(Expr::default()),
            Value::Int(i) => Ok(Expr::new(Value::Int(i), start, self.end)),
            Value::Double(d) => Ok(Expr::new(
                Value::Int(orca_crt::f64_to_i32(orca_crt::round(d))),
                start,
                self.end,
            )),
            _ => Err(self.throw("Cannot round a non-numeric value.")),
        }
    }

    /// `floor(start_pos)`: an empty value is refused too, as the C++ switch has no case for it.
    pub fn floor(&self, start: Pos) -> Result<Expr, Fail> {
        match self.v {
            Value::Int(i) => Ok(Expr::new(Value::Int(i), start, self.end)),
            Value::Double(d) => Ok(Expr::new(
                Value::Int(orca_crt::f64_to_i32(orca_crt::floor(d))),
                start,
                self.end,
            )),
            _ => Err(self.throw("Cannot floor a non-numeric value.")),
        }
    }

    /// `ceil(start_pos)`: an empty value is refused too.
    pub fn ceil(&self, start: Pos) -> Result<Expr, Fail> {
        match self.v {
            Value::Int(i) => Ok(Expr::new(Value::Int(i), start, self.end)),
            Value::Double(d) => Ok(Expr::new(
                Value::Int(orca_crt::f64_to_i32(orca_crt::ceil(d))),
                start,
                self.end,
            )),
            _ => Err(self.throw("Cannot ceil a non-numeric value.")),
        }
    }

    /// `unary_not(start_pos)`.
    pub fn unary_not(&self, start: Pos) -> Result<Expr, Fail> {
        match self.v {
            Value::Empty => Ok(Expr::default()),
            Value::Bool(b) => Ok(Expr::new(Value::Bool(!b), start, self.end)),
            _ => Err(self.throw("Cannot apply a not operator.")),
        }
    }

    /// `operator+=`.
    pub fn add(&mut self, rhs: &Expr) -> Result<(), Fail> {
        match &mut self.v {
            Value::Empty => {}
            Value::Str(s) => s.extend(rhs.to_bytes()?),
            _ => {
                if let Value::Str(r) = &rhs.v {
                    let mut s = self.to_bytes()?;
                    s.extend_from_slice(r);
                    self.v = Value::Str(s);
                } else {
                    let msg = "Cannot add non-numeric types.";
                    self.need_numeric(msg)?;
                    rhs.need_numeric(msg)?;
                    self.v = match (&self.v, &rhs.v) {
                        (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_add(*b)),
                        _ => Value::Double(self.as_d() + rhs.as_d()),
                    };
                }
            }
        }
        self.end = rhs.end;
        Ok(())
    }

    /// `operator-=`, `operator*=`: `op` is `-` or `*`.
    pub fn sub_mul(&mut self, rhs: &Expr, op: u8) -> Result<(), Fail> {
        if self.v == Value::Empty {
            self.reset();
            return Ok(());
        }
        let msg = if op == b'-' {
            "Cannot subtract non-numeric types."
        } else {
            "Cannot multiply with non-numeric type."
        };
        self.need_numeric(msg)?;
        rhs.need_numeric(msg)?;
        self.v = match (&self.v, &rhs.v) {
            (Value::Int(a), Value::Int(b)) => Value::Int(if op == b'-' {
                a.wrapping_sub(*b)
            } else {
                a.wrapping_mul(*b)
            }),
            _ => Value::Double(if op == b'-' {
                self.as_d() - rhs.as_d()
            } else {
                self.as_d() * rhs.as_d()
            }),
        };
        self.end = rhs.end;
        Ok(())
    }

    /// `operator/=`, `operator%=`: `op` is `/` or `%`.
    pub fn div_mod(&mut self, rhs: &Expr, op: u8) -> Result<(), Fail> {
        if self.v == Value::Empty {
            self.reset();
            return Ok(());
        }
        self.need_numeric("Cannot divide a non-numeric type.")?;
        rhs.need_numeric("Cannot divide with a non-numeric type.")?;
        let zero = match rhs.v {
            Value::Int(i) => i == 0,
            _ => rhs.as_d() == 0.0,
        };
        if zero {
            return Err(rhs.throw("Division by zero"));
        }
        self.v = match (&self.v, &rhs.v) {
            (Value::Int(a), Value::Int(b)) => {
                if *a == i32::MIN && *b == -1 {
                    // idiv raises a hardware exception there.
                    return Err(Fail::unsupported("INT_MIN divided by -1"));
                }
                Value::Int(if op == b'/' { a / b } else { a % b })
            }
            _ => Value::Double(if op == b'/' {
                self.as_d() / rhs.as_d()
            } else {
                orca_crt::fmod(self.as_d(), rhs.as_d())
            }),
        };
        self.end = rhs.end;
        Ok(())
    }

    /// `compare_op(lhs, rhs, op, invert)`: `op` is `=`, `<` or `>`; the result goes in `self`,
    /// its range kept.
    pub fn compare(&mut self, rhs: &Expr, op: u8, invert: bool) -> Result<(), Fail> {
        let cannot = || Fail::at(self.begin, "Cannot compare the types.");
        let value = match (&self.v, &rhs.v) {
            (Value::Empty, _) => return Ok(()),
            (a, b) if self.numeric() && rhs.numeric() => {
                let double = matches!(a, Value::Double(_)) || matches!(b, Value::Double(_));
                let (x, y) = (self.as_d(), rhs.as_d());
                let (i, j) = match (a, b) {
                    (Value::Int(i), Value::Int(j)) => (*i, *j),
                    _ => (0, 0),
                };
                match op {
                    b'=' => {
                        if double {
                            orca_crt::fabs(x - y) < 1e-8
                        } else {
                            i == j
                        }
                    }
                    b'<' => {
                        if double {
                            x < y
                        } else {
                            i < j
                        }
                    }
                    _ => {
                        if double {
                            x > y
                        } else {
                            i > j
                        }
                    }
                }
            }
            (Value::Bool(a), Value::Bool(b)) => {
                if op != b'=' {
                    return Err(cannot());
                }
                a == b
            }
            (Value::Str(_), _) | (_, Value::Str(_)) => {
                let (l, r) = (self.to_bytes()?, rhs.to_bytes()?);
                match op {
                    b'=' => l == r,
                    b'<' => l < r,
                    _ => l > r,
                }
            }
            _ => return Err(cannot()),
        };
        self.v = Value::Bool(if invert { !value } else { value });
        Ok(())
    }

    /// `logical_op(lhs, rhs, op)`: `op` is `|` or `&`.
    pub fn logical(&mut self, rhs: &Expr, op: u8) -> Result<(), Fail> {
        let value = match (&self.v, &rhs.v) {
            (Value::Empty, _) => return Ok(()),
            (Value::Bool(a), Value::Bool(b)) => {
                if op == b'|' {
                    *a || *b
                } else {
                    *a && *b
                }
            }
            _ => {
                return Err(Fail::at(
                    self.begin,
                    "Cannot apply logical operation to non-boolean operators.",
                ));
            }
        };
        self.v = Value::Bool(value);
        Ok(())
    }

    /// `function_2params(param1, param2, fun)`: `min` (`max` false) or `max`.
    pub fn min_max(&mut self, rhs: &Expr, max: bool) -> Result<(), Fail> {
        if self.v == Value::Empty {
            return Ok(());
        }
        self.need_numeric("Not a numeric type.")?;
        rhs.need_numeric("Not a numeric type.")?;
        self.v = match (&self.v, &rhs.v) {
            (Value::Int(_), Value::Int(_)) => {
                let (a, b) = (self.as_i(), rhs.as_i());
                Value::Int(if max {
                    orca_stl::max(a, b)
                } else {
                    orca_stl::min(a, b)
                })
            }
            _ => {
                let (a, b) = (self.as_d(), rhs.as_d());
                Value::Double(if max {
                    orca_stl::max(a, b)
                } else {
                    orca_stl::min(a, b)
                })
            }
        };
        Ok(())
    }

    /// `digits<leading_zeros>(param1, param2, param3)`.
    pub fn digits(&mut self, p2: &Expr, p3: &Expr, leading_zeros: bool) -> Result<(), Fail> {
        if self.v == Value::Empty {
            return Ok(());
        }
        self.need_numeric("Not a numeric type.")?;
        let Value::Int(n) = p2.v else {
            return Err(p2.throw("digits: second parameter must be integer"));
        };
        let has_decimals = p3.v != Value::Empty;
        let ndigits = i64::from(orca_stl::clamp(n, 0, 64));
        let out = if has_decimals {
            let Value::Int(d) = p3.v else {
                return Err(p3.throw("digits: third parameter must be integer"));
            };
            let decimals = i64::from(orca_stl::clamp(d, 0, 64));
            let fmt: &[u8] = if leading_zeros { b"%0*.*lf" } else { b"%*.*lf" };
            sprintf(
                fmt,
                &[
                    Arg::Int(ndigits),
                    Arg::Int(decimals),
                    Arg::Double(self.as_d()),
                ],
            )
        } else {
            let fmt: &[u8] = if leading_zeros { b"%0*d" } else { b"%*d" };
            sprintf(
                fmt,
                &[Arg::Int(ndigits), Arg::Int(i64::from(self.as_i_rounded()))],
            )
        }
        .map_err(|e| Fail::unsupported(format!("digits(): sprintf failed: {e:?}")))?;
        if out.len() >= 256 {
            // The C++ code prints into a char[256].
            return Err(Fail::unsupported("digits() past its 256-byte buffer"));
        }
        self.v = Value::Str(out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(v: Value) -> Expr {
        Expr::new(v, Some(0), Some(1))
    }

    #[test]
    fn arithmetic_follows_the_cpp_types() {
        let mut a = e(Value::Int(7));
        a.div_mod(&e(Value::Int(2)), b'/').unwrap();
        assert_eq!(a.v, Value::Int(3));
        let mut a = e(Value::Int(-7));
        a.div_mod(&e(Value::Int(2)), b'%').unwrap();
        assert_eq!(a.v, Value::Int(-1));
        let mut s = e(Value::Str(b"n".to_vec()));
        s.add(&e(Value::Double(0.4))).unwrap();
        assert_eq!(s.v, Value::Str(b"n0.4".to_vec()));
        let mut b = e(Value::Bool(true));
        b.add(&e(Value::Str(b"x".to_vec()))).unwrap();
        assert_eq!(b.v, Value::Str(b"truex".to_vec()));
        let mut z = e(Value::Int(1));
        assert_eq!(
            z.div_mod(&e(Value::Double(-0.0)), b'/'),
            Err(Fail::at(Some(0), "Division by zero"))
        );
    }

    #[test]
    fn comparisons_take_doubles_within_1e_8() {
        let mut a = e(Value::Double(0.4));
        a.compare(&e(Value::Double(0.4 + 1e-9)), b'=', false)
            .unwrap();
        assert_eq!(a.v, Value::Bool(true));
        let mut b = e(Value::Bool(true));
        assert!(b.compare(&e(Value::Bool(true)), b'<', false).is_err());
        let mut s = e(Value::Int(10));
        s.compare(&e(Value::Str(b"9".to_vec())), b'<', false)
            .unwrap();
        assert_eq!(s.v, Value::Bool(true));
    }

    #[test]
    fn digits_prints_as_sprintf() {
        let mut a = e(Value::Double(1.23456));
        a.digits(&e(Value::Int(6)), &e(Value::Int(2)), true)
            .unwrap();
        assert_eq!(a.v, Value::Str(b"001.23".to_vec()));
        let mut b = e(Value::Double(2.5));
        b.digits(&e(Value::Int(3)), &Expr::default(), false)
            .unwrap();
        assert_eq!(b.v, Value::Str(b"  3".to_vec()));
    }
}
