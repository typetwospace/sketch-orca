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
// Carries OrcaSlicer code: the macro_processor grammar's start rule and its expression rules,
// MyContext (symbol lookup, the variable actions, the conditional blocks, process_error_message
// and its tag table), FactorActions, InterpolateTableContext, utf8_char_parser and
// ascii_char_skipper_parser of src/libslic3r/PlaceholderParser.cpp of OrcaSlicer v2.4.2,
// translated on 2026-11-07. Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu Lab)
// and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).
//
// Carries Boost code: the parse semantics of Boost.Spirit Qi's sequence, expect, alternative,
// kleene, optional, difference and not-predicate operators, its rule, raw, lexeme, eps, eoi,
// literal and char parsers, its info tags, and the distinct directive and iter_pos of the Spirit
// repository, of Boost 1.84.0, translated on 2026-11-07. Copyright (c) 2001-2011 Joel de Guzman,
// Hartmut Kaiser and the Spirit contributors, under the Boost Software License, version 1.0
// (BSL-1.0; licenses/BSL-1.0.txt).
//
// Carries Boost code: boost::nowide::narrow(widen()) as an error message's line goes through it
// (utf::convert_string and utf_traits<char, 1>::decode and encode of boost/nowide/utf/convert.hpp
// and utf.hpp of Boost 1.84.0), translated on 2026-11-07. Copyright (c) 2009-2012 Artyom Beilis
// (Tonkikh) and Copyright (c) 2020 Alexander Grund, under the Boost Software License, version 1.0
// (BSL-1.0; licenses/BSL-1.0.txt).

//! The grammar of the placeholder parser's boolean expressions, as Spirit runs it.
//!
//! Each rule is a function on the template and a position, returning `Ok(Some(value))` where the
//! Spirit rule matches (the position moved past it), `Ok(None)` where it fails and Spirit would
//! backtrack, and `Err` where Spirit throws (an `expectation_failure`, from a `>` whose later
//! operand fails or from an action). The positions move as Spirit's do, which only the errors'
//! positions show: every primitive parser skips white space before it reads and keeps the
//! skipped position even when it then fails, a sequence works on a copy it keeps only when it
//! matches, so a rule whose right side is a sequence never moves on failure, while
//! `regular_expression` (a `raw[...]`) does. The skipper (`ascii_char_skipper_parser`) skips
//! `\r`, `\n`, `\t` and space and throws on a byte past ASCII or a bad UTF-8 sequence.
//!
//! The rules are those `evaluate_boolean_expression` reaches: `start` with `just_boolean_expression`
//! set, whose first alternative (`eps(evaluate_full_macro) > text_block`) fails at once, then
//! `conditional_expression` down to `unary_expression` and its helpers. The macro language (text
//! blocks, `{if}`, assignments, `local` and `global`) is phase O4's: `MacroRules` marks where
//! its rules join.

use orca_config::OptionValue;

use crate::Symbols;
use crate::expr::{Expr, Fail, Pos, Value};
use crate::number::{parse_int, parse_strict_double};
use crate::regex::{Regex, RegexError};

/// The grammar's `keywords` symbols.
const KEYWORDS: &[&[u8]] = &[
    b"and",
    b"digits",
    b"zdigits",
    b"empty",
    b"if",
    b"int",
    b"is_nil",
    b"local",
    b"else",
    b"elsif",
    b"endif",
    b"false",
    b"global",
    b"interpolate_table",
    b"min",
    b"max",
    b"random",
    b"filament_change",
    b"repeat",
    b"round",
    b"floor",
    b"ceil",
    b"not",
    b"one_of",
    b"or",
    b"size",
    b"true",
];

/// `MyContext::tag_to_error_message`.
const TAG_MESSAGES: &[(&str, &str)] = &[
    ("eoi", "Unknown syntax error"),
    ("start", "Unknown syntax error"),
    ("text", "Invalid text."),
    ("text_block", "Invalid text block."),
    ("macro", "Invalid macro."),
    ("repeat", "Unknown syntax error"),
    ("if_else_output", "Not an {if}{else}{endif} macro."),
    ("switch_output", "Not a {switch} macro."),
    (
        "legacy_variable_expansion",
        "Expecting a legacy variable expansion format",
    ),
    ("identifier", "Expecting an identifier."),
    (
        "conditional_expression",
        "Expecting a conditional expression.",
    ),
    ("logical_or_expression", "Expecting a boolean expression."),
    ("logical_and_expression", "Expecting a boolean expression."),
    ("equality_expression", "Expecting an expression."),
    ("bool_expr_eval", "Expecting a boolean expression."),
    ("relational_expression", "Expecting an expression."),
    ("additive_expression", "Expecting an expression."),
    ("multiplicative_expression", "Expecting an expression."),
    ("unary_expression", "Expecting an expression."),
    (
        "optional_parameter",
        "Expecting a closing brace or an optional parameter.",
    ),
    (
        "one_of_list",
        "Expecting a list of string patterns (simple text or rexep)",
    ),
    ("variable_reference", "Expecting a variable reference."),
    ("variable", "Expecting a variable name."),
    ("regular_expression", "Expecting a regular expression."),
];

/// `libslic3r`'s `EPSILON`.
const EPSILON: f64 = 1e-4;

/// `std::isalnum` in the "C" locale, or `_` (the `distinct` tail and the identifier's body).
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `OptWithPos`: a resolved variable, its index and its range.
#[derive(Clone)]
struct Var<'a> {
    opt: Option<&'a OptionValue>,
    index: i32,
    begin: usize,
    end: usize,
}

impl Var<'_> {
    fn has_index(&self) -> bool {
        self.index != -1
    }
}

/// Where phase O4's macro language joins: `start`'s first alternative (`eps(evaluate_full_macro)
/// > text_block`), which `evaluate_boolean_expression` never takes. A parser built for
/// `process()` will hold its text-block, macro and assignment rules here.
#[derive(Clone, Copy, Default)]
pub struct MacroRules;

/// `MyContext` with the grammar: the template, the symbols and `m_depth_suppressed`.
pub struct Parser<'a> {
    s: &'a [u8],
    config: &'a dyn Symbols,
    config_override: Option<&'a dyn Symbols>,
    depth: i32,
    /// `just_boolean_expression` is the only mode taken yet.
    _macros: MacroRules,
}

type R<T> = Result<T, Fail>;

/// A failed operand of `>`: Spirit's `expectation_failure` at `at` with the operand's tag.
fn expected<T>(at: usize, tag: &str) -> R<T> {
    Err(Fail::Expect {
        at: Some(at),
        tag: tag.into(),
    })
}

/// `get_at(i)` of a vector: the element, or the first past the end (an empty vector is undefined
/// behaviour).
fn get_at<T: Copy>(v: &[T], i: usize) -> R<T> {
    if i < v.len() {
        Ok(v[i])
    } else {
        v.first()
            .copied()
            .ok_or_else(|| Fail::unsupported("get_at of an empty vector"))
    }
}

impl<'a> Parser<'a> {
    /// A parser for `templ` with `config` and the override (`MyContext` as
    /// `evaluate_boolean_expression` sets it).
    pub fn new(
        templ: &'a [u8],
        config: &'a dyn Symbols,
        config_override: Option<&'a dyn Symbols>,
    ) -> Parser<'a> {
        Parser {
            s: templ,
            config,
            config_override,
            depth: 0,
            _macros: MacroRules,
        }
    }

    fn len(&self) -> usize {
        self.s.len()
    }

    /// `skipping()`.
    fn skipping(&self) -> bool {
        self.depth > 0
    }

    /// `optptr` (`resolve_symbol`): the override first, then the configuration.
    fn resolve_symbol(&self, key: &str) -> Option<&'a OptionValue> {
        if let Some(o) = self.config_override
            && let Some(v) = o.option(key)
        {
            return Some(v);
        }
        self.config.option(key)
    }

    // ----------------------------------------------------------------- primitives

    /// `utf8_char_parser::parse`: the position after the sequence at `p`, `None` at the end,
    /// and the "Invalid utf8 sequence" error.
    fn utf8(&self, p: usize) -> R<Option<usize>> {
        let len = self.len();
        if p == len {
            return Ok(None);
        }
        let bad = || Fail::at(Some(p), "Invalid utf8 sequence");
        let c = self.s[p];
        if c & 0xC0 == 0x80 {
            return Err(bad());
        }
        let ones = c.leading_ones();
        let mut cnt = if ones == 0 { 1 } else { ones.min(4) };
        let mut it = p + 1;
        cnt -= 1;
        while cnt > 0 {
            if it == len {
                return Err(bad());
            }
            let c = self.s[it];
            it += 1;
            if cnt > 1 && c & 0xC0 != 0x80 {
                return Err(bad());
            }
            cnt -= 1;
        }
        Ok(Some(it))
    }

    /// `skip_over` with `ascii_char_skipper_parser`.
    fn skip(&self, p: &mut usize) -> R<()> {
        while let Some(it) = self.utf8(*p)? {
            let c = self.s[*p];
            if it - *p > 1 || c >= 0x80 {
                return Err(Fail::at(
                    Some(*p),
                    "Non-ASCII7 characters are only allowed inside text blocks and string literals, not inside code blocks.",
                ));
            }
            if matches!(c, b'\r' | b'\n' | b'\t' | b' ') {
                *p += 1;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// `lit(c)`.
    fn lit(&self, p: &mut usize, c: u8) -> R<bool> {
        self.skip(p)?;
        if *p < self.len() && self.s[*p] == c {
            *p += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// `lit("..")`.
    fn lit_str(&self, p: &mut usize, t: &[u8]) -> R<bool> {
        self.skip(p)?;
        if self.s[*p..].starts_with(t) {
            *p += t.len();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// `> lit(c)`: the literal or an expectation failure (`literal-char`).
    fn expect_lit(&self, p: &mut usize, c: u8) -> R<()> {
        if self.lit(p, c)? {
            Ok(())
        } else {
            expected(*p, "literal-char")
        }
    }

    /// `distinct(alnum | '_')[...]` on a word: skips on a copy, matches the word, then refuses a
    /// word character after it.
    fn kw(&self, p: &mut usize, w: &[u8]) -> R<bool> {
        let mut i = *p;
        self.skip(&mut i)?;
        if !self.s[i..].starts_with(w) {
            return Ok(false);
        }
        let j = i + w.len();
        if j < self.len() && is_word(self.s[j]) {
            return Ok(false);
        }
        *p = j;
        Ok(true)
    }

    /// `kw[keywords]` as `!kw[keywords]` tests it: the longest keyword at the position, with no
    /// word character after it.
    fn keyword_at(&self, p: usize) -> R<bool> {
        let mut i = p;
        self.skip(&mut i)?;
        let rest = &self.s[i..];
        // The symbol table's longest match (no two keywords of one length match at once).
        let longest = KEYWORDS.iter().filter(|k| rest.starts_with(k)).fold(
            None,
            |best: Option<&&[u8]>, k| match best {
                Some(b) if b.len() >= k.len() => Some(b),
                _ => Some(k),
            },
        );
        let Some(k) = longest else {
            return Ok(false);
        };
        let j = i + k.len();
        Ok(!(j < self.len() && is_word(self.s[j])))
    }

    // ----------------------------------------------------------------- start

    /// `start` with `just_boolean_expression`: the text the macro processor outputs ("true",
    /// "false", or empty where the expression does not parse), or the failure.
    pub fn start(&mut self) -> R<Vec<u8>> {
        let mut i = 0usize;
        // ((eps(evaluate_full_macro) > text_block) | conditional_expression[...]) > eoi
        {
            let mut j = i;
            self.skip(&mut j)?;
            // evaluate_full_macro is false: the first alternative fails here.
        }
        let Some(e) = self.conditional_expression(&mut i)? else {
            return Ok(Vec::new());
        };
        // evaluate_boolean_to_string
        let out: Vec<u8> = match e.v {
            Value::Bool(b) => if b { "true" } else { "false" }.into(),
            _ => return Err(e.throw("Not a boolean expression")),
        };
        self.skip(&mut i)?;
        if i != self.len() {
            return expected(i, "eoi");
        }
        Ok(out)
    }

    // ----------------------------------------------------------------- expression rules

    /// `conditional_expression`.
    fn conditional_expression(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.logical_or(&mut i)? else {
            return Ok(None);
        };
        let mut j = i;
        if self.lit(&mut j, b'?')? {
            let mut a = false;
            self.skip(&mut j)?;
            // evaluate_boolean(_val, _a)
            match &val.v {
                Value::Empty => {}
                Value::Bool(b) => a = *b,
                _ => return Err(val.throw("Not a boolean expression")),
            }
            self.skip(&mut j)?;
            self.block_enter(a);
            let Some(c1) = self.conditional_expression(&mut j)? else {
                return expected(j, "conditional_expression");
            };
            self.block_exit_ternary(a, c1, &mut val);
            self.expect_lit(&mut j, b':')?;
            self.skip(&mut j)?;
            self.block_enter(!a);
            let Some(c2) = self.conditional_expression(&mut j)? else {
                return expected(j, "conditional_expression");
            };
            self.block_exit_ternary(!a, c2, &mut val);
            i = j;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `block_enter(condition)`.
    fn block_enter(&mut self, condition: bool) {
        if self.skipping() || !condition {
            self.depth += 1;
        }
    }

    /// `block_exit_ternary(condition, data_in, data_out)`.
    fn block_exit_ternary(&mut self, condition: bool, data_in: Expr, data_out: &mut Expr) {
        if self.skipping() {
            self.depth -= 1;
        } else if condition {
            *data_out = data_in;
        }
    }

    /// `logical_or_expression`.
    fn logical_or(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.logical_and(&mut i)? else {
            return Ok(None);
        };
        loop {
            let mut j = i;
            if !(self.kw(&mut j, b"or")? || self.lit_str(&mut j, b"||")?) {
                break;
            }
            let Some(rhs) = self.logical_and(&mut j)? else {
                return expected(j, "logical_and_expression");
            };
            val.logical(&rhs, b'|')?;
            i = j;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `logical_and_expression`.
    fn logical_and(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.equality(&mut i)? else {
            return Ok(None);
        };
        loop {
            let mut j = i;
            if !(self.kw(&mut j, b"and")? || self.lit_str(&mut j, b"&&")?) {
                break;
            }
            // equality_expression is named "bool expression".
            let Some(rhs) = self.equality(&mut j)? else {
                return expected(j, "bool expression");
            };
            val.logical(&rhs, b'&')?;
            i = j;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `equality_expression` (named "bool expression").
    fn equality(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.relational(&mut i)? else {
            return Ok(None);
        };
        'next: loop {
            for (op, invert) in [(&b"=="[..], false), (b"!=", true), (b"<>", true)] {
                let mut k = i;
                if self.lit_str(&mut k, op)? {
                    let Some(rhs) = self.relational(&mut k)? else {
                        return expected(k, "relational_expression");
                    };
                    val.compare(&rhs, b'=', invert)?;
                    i = k;
                    continue 'next;
                }
            }
            for (op, negate) in [(&b"=~"[..], false), (b"!~", true)] {
                let mut k = i;
                if self.lit_str(&mut k, op)? {
                    let Some(range) = self.regular_expression(&mut k)? else {
                        return expected(k, "regular_expression");
                    };
                    let lhs = val.clone();
                    self.regex_op(&lhs, range, negate, &mut val)?;
                    i = k;
                    continue 'next;
                }
            }
            break;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `relational_expression`.
    fn relational(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.additive(&mut i)? else {
            return Ok(None);
        };
        'next: loop {
            // leq is compare '>' inverted, geq '<' inverted.
            for (op, cmp, invert) in [
                (&b"<="[..], b'>', true),
                (b">=", b'<', true),
                (b"<", b'<', false),
                (b">", b'>', false),
            ] {
                let mut k = i;
                if self.lit_str(&mut k, op)? {
                    let Some(rhs) = self.additive(&mut k)? else {
                        return expected(k, "additive_expression");
                    };
                    val.compare(&rhs, cmp, invert)?;
                    i = k;
                    continue 'next;
                }
            }
            break;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `additive_expression`.
    fn additive(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.multiplicative(&mut i)? else {
            return Ok(None);
        };
        'next: loop {
            for op in [b'+', b'-'] {
                let mut k = i;
                if self.lit(&mut k, op)? {
                    let Some(rhs) = self.multiplicative(&mut k)? else {
                        return expected(k, "multiplicative_expression");
                    };
                    if op == b'+' {
                        val.add(&rhs)?;
                    } else {
                        val.sub_mul(&rhs, b'-')?;
                    }
                    i = k;
                    continue 'next;
                }
            }
            break;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `multiplicative_expression`.
    fn multiplicative(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(mut val) = self.unary(&mut i)? else {
            return Ok(None);
        };
        'next: loop {
            for op in [b'*', b'/', b'%'] {
                let mut k = i;
                if self.lit(&mut k, op)? {
                    let Some(rhs) = self.unary(&mut k)? else {
                        return expected(k, "unary_expression");
                    };
                    match op {
                        b'*' => val.sub_mul(&rhs, b'*')?,
                        _ => val.div_mod(&rhs, op)?,
                    }
                    i = k;
                    continue 'next;
                }
            }
            break;
        }
        *p = i;
        Ok(Some(val))
    }

    /// `> conditional_expression`.
    fn need_cond(&mut self, p: &mut usize) -> R<Expr> {
        match self.conditional_expression(p)? {
            Some(e) => Ok(e),
            None => expected(*p, "conditional_expression"),
        }
    }

    /// `> unary_expression`.
    fn need_unary(&mut self, p: &mut usize) -> R<Expr> {
        match self.unary(p)? {
            Some(e) => Ok(e),
            None => expected(*p, "unary_expression"),
        }
    }

    /// `> variable_reference`.
    fn need_var(&mut self, p: &mut usize) -> R<Var<'a>> {
        match self.variable_reference(p)? {
            Some(v) => Ok(v),
            None => expected(*p, "variable reference"),
        }
    }

    /// `unary_expression`.
    fn unary(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        // iter_pos[set_start_pos]
        self.skip(&mut i)?;
        let start = Some(i);
        let mut val = Expr::new(Value::Empty, start, start);
        if let Some(e) = self.unary_alternatives(&mut i, &mut val)? {
            *p = i;
            return Ok(Some(e));
        }
        Ok(None)
    }

    /// The alternatives of `unary_expression`, in order, on `i` (each on a copy it keeps on a
    /// match); `val` is the rule's `_val`.
    fn unary_alternatives(&mut self, i: &mut usize, val: &mut Expr) -> R<Option<Expr>> {
        let start = val.begin;
        // variable_reference[variable_value]
        let mut k = *i;
        if let Some(var) = self.variable_reference(&mut k)? {
            self.variable_value(&var, val)?;
            *i = k;
            return Ok(Some(val.clone()));
        }
        // ('(' > conditional_expression > ')' > iter_pos)[expr_]
        let mut k = *i;
        if self.lit(&mut k, b'(')? {
            let c = self.need_cond(&mut k)?;
            self.expect_lit(&mut k, b')')?;
            self.skip(&mut k)?;
            *i = k;
            return Ok(Some(Expr::new(c.v, start, Some(k))));
        }
        // ('-' > unary_expression)[minus_]
        let mut k = *i;
        if self.lit(&mut k, b'-')? {
            let u = self.need_unary(&mut k)?;
            *i = k;
            return Ok(Some(u.unary_minus(start)?));
        }
        // ('+' > unary_expression > iter_pos)[expr_]
        let mut k = *i;
        if self.lit(&mut k, b'+')? {
            let u = self.need_unary(&mut k)?;
            self.skip(&mut k)?;
            *i = k;
            return Ok(Some(Expr::new(u.v, start, Some(k))));
        }
        // ((kw["not"] | '!') > unary_expression > iter_pos)[not_]
        let mut k = *i;
        if self.kw(&mut k, b"not")? || self.lit(&mut k, b'!')? {
            let u = self.need_unary(&mut k)?;
            self.skip(&mut k)?;
            *i = k;
            return Ok(Some(u.unary_not(start)?));
        }
        // min, max, random: kw > '(' > cond[_val = _1] > ',' > cond > ')'
        for (name, which) in [(&b"min"[..], 0), (b"max", 1), (b"random", 2)] {
            let mut k = *i;
            if self.kw(&mut k, name)? {
                self.expect_lit(&mut k, b'(')?;
                *val = self.need_cond(&mut k)?;
                self.expect_lit(&mut k, b',')?;
                let c2 = self.need_cond(&mut k)?;
                self.expect_lit(&mut k, b')')?;
                match which {
                    0 => val.min_max(&c2, false)?,
                    1 => val.min_max(&c2, true)?,
                    _ => {
                        if !self.skipping() {
                            // No context_data in evaluate_boolean_expression.
                            return Err(Fail::at(
                                val.begin,
                                "Random number generator not available in this context.",
                            ));
                        }
                    }
                }
                *i = k;
                return Ok(Some(val.clone()));
            }
        }
        // (kw["filament_change"] > '(' > cond > ')')[filament_change]: it sets
        // current_extruder_id, which only a line width reads (and those decline), and leaves
        // _val empty.
        let mut k = *i;
        if self.kw(&mut k, b"filament_change")? {
            self.expect_lit(&mut k, b'(')?;
            self.need_cond(&mut k)?;
            self.expect_lit(&mut k, b')')?;
            *i = k;
            return Ok(Some(val.clone()));
        }
        // digits, zdigits: kw > '(' > cond[_val = _1] > ',' > cond > optional_parameter
        for (name, zeros) in [(&b"digits"[..], false), (b"zdigits", true)] {
            let mut k = *i;
            if self.kw(&mut k, name)? {
                self.expect_lit(&mut k, b'(')?;
                *val = self.need_cond(&mut k)?;
                self.expect_lit(&mut k, b',')?;
                let c2 = self.need_cond(&mut k)?;
                let Some(c3) = self.optional_parameter(&mut k)? else {
                    return expected(k, "optional_parameter");
                };
                val.digits(&c2, &c3, zeros)?;
                *i = k;
                return Ok(Some(val.clone()));
            }
        }
        // int, round, ceil, floor: kw > '(' > cond > ')'
        for name in [&b"int"[..], b"round", b"ceil", b"floor"] {
            let mut k = *i;
            if self.kw(&mut k, name)? {
                self.expect_lit(&mut k, b'(')?;
                let c = self.need_cond(&mut k)?;
                self.expect_lit(&mut k, b')')?;
                let out = match name {
                    b"int" => c.unary_integer(start)?,
                    b"round" => c.round(start)?,
                    b"ceil" => c.ceil(start)?,
                    _ => c.floor(start)?,
                };
                *i = k;
                return Ok(Some(out));
            }
        }
        // is_nil: kw > '(' > variable_reference > ')'
        let mut k = *i;
        if self.kw(&mut k, b"is_nil")? {
            self.expect_lit(&mut k, b'(')?;
            let var = self.need_var(&mut k)?;
            self.expect_lit(&mut k, b')')?;
            self.is_nil_test(&var, val)?;
            *i = k;
            return Ok(Some(val.clone()));
        }
        // one_of: kw > '(' > one_of > ')'
        let mut k = *i;
        if self.kw(&mut k, b"one_of")? {
            self.expect_lit(&mut k, b'(')?;
            let Some(r) = self.one_of(&mut k)? else {
                return expected(k, "one_of");
            };
            self.expect_lit(&mut k, b')')?;
            *i = k;
            return Ok(Some(r));
        }
        // empty, size: kw > '(' > variable_reference > ')'
        for (name, size) in [(&b"empty"[..], false), (b"size", true)] {
            let mut k = *i;
            if self.kw(&mut k, name)? {
                self.expect_lit(&mut k, b'(')?;
                let var = self.need_var(&mut k)?;
                self.expect_lit(&mut k, b')')?;
                self.vector_empty_or_size(&var, val, size)?;
                *i = k;
                return Ok(Some(val.clone()));
            }
        }
        // interpolate_table: kw > '(' > interpolate_table > ')' (the rule renamed to
        // interpolate_table_list)
        let mut k = *i;
        if self.kw(&mut k, b"interpolate_table")? {
            self.expect_lit(&mut k, b'(')?;
            let Some(r) = self.interpolate_table(&mut k)? else {
                return expected(k, "interpolate_table_list");
            };
            self.expect_lit(&mut k, b')')?;
            *i = k;
            return Ok(Some(r));
        }
        // (strict_double > iter_pos)[double_]
        let mut k = *i;
        self.skip(&mut k)?;
        if let Some(d) = parse_strict_double(self.s, &mut k) {
            self.skip(&mut k)?;
            *i = k;
            return Ok(Some(self.literal(Value::Double(d), start, k)));
        }
        // (int_ > iter_pos)[int_]
        let mut k = *i;
        self.skip(&mut k)?;
        if let Some(n) = parse_int(self.s, &mut k) {
            self.skip(&mut k)?;
            *i = k;
            return Ok(Some(self.literal(Value::Int(n), start, k)));
        }
        // (kw[bool_] > iter_pos)[bool_]
        for (word, b) in [(&b"true"[..], true), (b"false", false)] {
            let mut k = *i;
            if self.kw(&mut k, word)? {
                self.skip(&mut k)?;
                *i = k;
                return Ok(Some(self.literal(Value::Bool(b), start, k)));
            }
        }
        // raw[lexeme['"' > *((utf8char - '\\' - '"') | ('\\' > char_)) > '"']][string_]
        let mut k = *i;
        self.skip(&mut k)?;
        if k < self.len() && self.s[k] == b'"' {
            let b = k;
            k += 1;
            k = self.delimited_body(k, b'"')?;
            *i = k;
            return Ok(Some(self.string_literal(b, k)?));
        }
        Ok(None)
    }

    /// The body of a string literal or a regular expression after its opening delimiter, up to
    /// and past the closing one: `*((utf8char - '\\' - d) | ('\\' > char_)) > d`.
    fn delimited_body(&self, mut k: usize, d: u8) -> R<usize> {
        loop {
            if k == self.len() || self.s[k] == d {
                break;
            }
            if self.s[k] == b'\\' {
                k += 1;
                if k == self.len() {
                    return expected(k, "char");
                }
                k += 1;
                continue;
            }
            match self.utf8(k)? {
                Some(n) => k = n,
                None => break,
            }
        }
        if k < self.len() && self.s[k] == d {
            Ok(k + 1)
        } else {
            expected(k, "literal-char")
        }
    }

    /// `FactorActions::double_`, `int_`, `bool_`: when skipping, empty with the range's end set.
    fn literal(&self, v: Value, start: Pos, end: usize) -> Expr {
        if self.skipping() {
            Expr::new(Value::Empty, start, Some(end))
        } else {
            Expr::new(v, start, Some(end))
        }
    }

    /// `FactorActions::string_`: the literal from `b` (its opening quote) to `e` (past its
    /// closing one), unescaped.
    fn string_literal(&self, b: usize, e: usize) -> R<Expr> {
        if self.skipping() {
            return Ok(Expr::new(Value::Empty, Some(b), Some(e)));
        }
        let body = &self.s[b + 1..e - 1];
        let mut out = Vec::with_capacity(body.len());
        let mut it = 0usize;
        while it < body.len() {
            if body[it] == b'\\' {
                // The grammar always gives the backslash a character.
                let c = body[it + 1];
                match c {
                    b'r' => out.push(b'\r'),
                    b'n' => out.push(b'\n'),
                    b'"' | b'\\' => out.push(c),
                    _ => {
                        return Err(Fail::at(Some(b + 1 + it), "Invalid escape sequence"));
                    }
                }
                it += 2;
            } else {
                // get_utf8_sequence_length groups the bytes as the grammar's utf8char did, but
                // for a lead byte from 0xF8, where it counts five or six.
                if body[it] >= 0xF8 {
                    return Err(Fail::unsupported(
                        "a string literal with a byte from 0xF8, which get_utf8_sequence_length and the grammar count differently",
                    ));
                }
                let n = match body[it] {
                    // ASCII, and a continuation byte the grammar counts alone.
                    0x00..=0xBF => 1,
                    0xC0..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    _ => 4,
                }
                .min(body.len() - it);
                out.extend_from_slice(&body[it..it + n]);
                it += n;
            }
        }
        Ok(Expr::new(Value::Str(out), Some(b), Some(e)))
    }

    /// `optional_parameter`.
    fn optional_parameter(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        self.skip(&mut i)?;
        let start = Some(i);
        let mut k = i;
        if self.lit(&mut k, b')')? {
            *p = k;
            return Ok(Some(Expr::new(Value::Empty, start, start)));
        }
        let mut k = i;
        if self.lit(&mut k, b',')? {
            let c = self.need_cond(&mut k)?;
            self.expect_lit(&mut k, b')')?;
            *p = k;
            return Ok(Some(c));
        }
        Ok(None)
    }

    /// `regular_expression`: `raw[...]`, which skips white space on the caller's position even
    /// when it fails. The range runs from the opening slash past the closing one.
    fn regular_expression(&self, p: &mut usize) -> R<Option<(usize, usize)>> {
        self.skip(p)?;
        let b = *p;
        if !(b < self.len() && self.s[b] == b'/') {
            return Ok(None);
        }
        let e = self.delimited_body(b + 1, b'/')?;
        *p = e;
        Ok(Some((b, e)))
    }

    /// `regex_op(lhs, rhs, op, out)`.
    fn regex_op(&self, lhs: &Expr, range: (usize, usize), negate: bool, out: &mut Expr) -> R<()> {
        let subject = match &lhs.v {
            Value::Empty => return Ok(()),
            Value::Str(s) => s,
            _ => return Err(lhs.throw("Left hand side of a regex match must be a string.")),
        };
        let pattern = &self.s[range.0 + 1..range.1 - 1];
        let re = match Regex::new(pattern) {
            Ok(re) => re,
            Err(RegexError::Error(what)) => {
                return Err(Fail::at(
                    Some(range.0),
                    &format!("Regular expression compilation failed: {what}"),
                ));
            }
            Err(RegexError::Unsupported(u)) => return Err(Fail::unsupported(u)),
        };
        let result = match re.is_match(subject) {
            Ok(m) => m,
            Err(RegexError::Unsupported(u) | RegexError::Error(u)) => {
                return Err(Fail::unsupported(u));
            }
        };
        out.v = Value::Bool(result != negate);
        Ok(())
    }

    /// `one_of`: `(unary_expression[_a = _1] > one_of_list(_a))[_val = _2]`.
    fn one_of(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(a) = self.unary(&mut i)? else {
            return Ok(None);
        };
        let out = self.one_of_list(&mut i, &a)?;
        *p = i;
        Ok(Some(out))
    }

    /// `one_of_list(_a)`, which never fails.
    fn one_of_list(&mut self, p: &mut usize, a: &Expr) -> R<Expr> {
        let mut i = *p;
        self.skip(&mut i)?;
        // one_of_test_init: set_b(false), the range null.
        let mut out = Expr::new(Value::Bool(false), None, None);
        let mut k = i;
        if self.lit(&mut k, b',')? {
            loop {
                let mut j = k;
                let mut m = j;
                if let Some(u) = self.unary(&mut m)? {
                    self.one_of_test(a, &u, &mut out, false)?;
                    j = m;
                } else {
                    let mut m = j;
                    if self.lit(&mut m, b'~')? {
                        let u = self.need_unary(&mut m)?;
                        self.one_of_test(a, &u, &mut out, true)?;
                        j = m;
                    } else {
                        let mut m = j;
                        match self.regular_expression(&mut m)? {
                            Some(range) => {
                                self.one_of_test_regex(a, range, &mut out)?;
                                j = m;
                            }
                            None => break,
                        }
                    }
                }
                // >> -lit(',')
                self.lit(&mut j, b',')?;
                k = j;
            }
            i = k;
        }
        *p = i;
        Ok(out)
    }

    /// `out.b()` of `one_of`'s result, which is a bool whenever it is read.
    fn one_of_done(out: &Expr) -> R<bool> {
        match out.v {
            Value::Bool(b) => Ok(b),
            _ => Err(Fail::unsupported("one_of read a result that is not a bool")),
        }
    }

    /// `one_of_test<RegEx>(match, pattern, out)`.
    fn one_of_test(&self, m: &Expr, pattern: &Expr, out: &mut Expr, regex: bool) -> R<()> {
        if m.v == Value::Empty {
            out.reset();
            return Ok(());
        }
        if Self::one_of_done(out)? {
            return Ok(());
        }
        let Value::Str(subject) = &m.v else {
            return Err(m.throw(
                "one_of(): First parameter (the string to match against) has to be a string value",
            ));
        };
        let Value::Str(pat) = &pattern.v else {
            return Err(m.throw("one_of(): Pattern has to be a string value"));
        };
        let result = if regex {
            match Regex::new(pat) {
                Ok(re) => match re.is_match(subject) {
                    Ok(b) => b,
                    Err(RegexError::Unsupported(u) | RegexError::Error(u)) => {
                        return Err(Fail::unsupported(u));
                    }
                },
                Err(RegexError::Error(_)) => {
                    return Err(pattern.throw("Regular expression compilation failed"));
                }
                Err(RegexError::Unsupported(u)) => return Err(Fail::unsupported(u)),
            }
        } else {
            subject == pat
        };
        out.v = Value::Bool(result);
        Ok(())
    }

    /// `one_of_test_regex(match, pattern, out)`.
    fn one_of_test_regex(&self, m: &Expr, range: (usize, usize), out: &mut Expr) -> R<()> {
        if m.v == Value::Empty {
            out.reset();
            return Ok(());
        }
        if Self::one_of_done(out)? {
            return Ok(());
        }
        if !matches!(m.v, Value::Str(_)) {
            return Err(m.throw(
                "one_of(): First parameter (the string to match against) has to be a string value",
            ));
        }
        self.regex_op(m, range, false, out)
    }

    /// `interpolate_table`: `(unary_expression[_a = _1] > ',' > interpolate_table_list(_a))
    /// [evaluate(_a, _2, _val)]`.
    fn interpolate_table(&mut self, p: &mut usize) -> R<Option<Expr>> {
        let mut i = *p;
        let Some(x) = self.unary(&mut i)? else {
            return Ok(None);
        };
        self.expect_lit(&mut i, b',')?;
        // interpolate_table_list: eps[init(_r2)] > *(...)
        self.skip(&mut i)?;
        if x.v != Value::Empty && !x.numeric() {
            return Err(x.throw("Interpolation value must be a number."));
        }
        let mut table: Vec<(f64, Pos, f64)> = Vec::new();
        loop {
            let mut j = i;
            if !self.lit(&mut j, b'(')? {
                break;
            }
            let tx = self.need_unary(&mut j)?;
            self.expect_lit(&mut j, b',')?;
            let ty = self.need_unary(&mut j)?;
            self.expect_lit(&mut j, b')')?;
            // add_pair
            if tx.v != Value::Empty {
                tx.need_numeric("X value of a table point must be a number.")?;
                ty.need_numeric("Y value of a table point must be a number.")?;
                table.push((tx.as_d(), tx.begin, ty.as_d()));
            }
            self.lit(&mut j, b',')?;
            i = j;
        }
        let mut out = Expr::default();
        self.interpolate_evaluate(&x, &table, &mut out)?;
        *p = i;
        Ok(Some(out))
    }

    /// `InterpolateTableContext::evaluate`.
    fn interpolate_evaluate(&self, x: &Expr, table: &[(f64, Pos, f64)], out: &mut Expr) -> R<()> {
        if x.v == Value::Empty {
            return Ok(());
        }
        let xv = x.as_d();
        let mut evaluated = false;
        for i in 1..table.len() {
            let (x0, b0, y0) = table[i - 1];
            let (x1, _, y1) = table[i];
            if x0 > x1 {
                // The message has no '*': process_error_message looks it up as a tag.
                return Err(Fail::Expect {
                    at: b0,
                    tag: "X coordinates of the table must be increasing".into(),
                });
            }
            if !evaluated && xv >= x0 && xv <= x1 {
                let v = if xv == x0 {
                    y0
                } else if xv == x1 {
                    y1
                } else if orca_crt::fabs(x0 - x1) < EPSILON {
                    0.5 * (y0 + y1)
                } else {
                    let t = (xv - x0) / (x1 - x0);
                    (1.0 - t) * y0 + t * y1
                };
                out.v = Value::Double(v);
                evaluated = true;
            }
        }
        if !evaluated {
            let (Some(front), Some(back)) = (table.first(), table.last()) else {
                return Err(Fail::unsupported("interpolate_table() with an empty table"));
            };
            let x0 = front.0;
            let x1 = back.0;
            if xv > x0 - EPSILON && xv < x0 {
                out.v = Value::Double(front.2);
            } else if xv > x1 && xv < x1 + EPSILON {
                out.v = Value::Double(back.2);
            } else {
                return Err(x.throw("Interpolation value is outside the table range"));
            }
        }
        Ok(())
    }

    // ----------------------------------------------------------------- variables

    /// `identifier`: `!kw[keywords] >> raw[lexeme[(alpha | '_') >> *(alnum | '_')]]`.
    fn identifier(&self, p: &mut usize) -> R<Option<(usize, usize)>> {
        let mut i = *p;
        if self.keyword_at(i)? {
            return Ok(None);
        }
        self.skip(&mut i)?;
        let b = i;
        if i < self.len() && (self.s[i].is_ascii_alphabetic() || self.s[i] == b'_') {
            i += 1;
            while i < self.len() && is_word(self.s[i]) {
                i += 1;
            }
        } else {
            return Ok(None);
        }
        *p = i;
        Ok(Some((b, i)))
    }

    /// `variable`: `identifier[resolve_variable]`.
    fn variable(&self, p: &mut usize) -> R<Option<Var<'a>>> {
        let mut i = *p;
        let Some((b, e)) = self.identifier(&mut i)? else {
            return Ok(None);
        };
        let mut var = Var {
            opt: None,
            index: -1,
            begin: b,
            end: e,
        };
        if !self.skipping() {
            let key = std::str::from_utf8(&self.s[b..e]).expect("an identifier is ASCII");
            match self.resolve_symbol(key) {
                Some(o) => var.opt = Some(o),
                // resolve_output_symbol finds nothing: no outputs, no context data, no locals.
                None => return Err(Fail::at(Some(b), "Not a variable name")),
            }
        }
        *p = i;
        Ok(Some(var))
    }

    /// `variable_reference`.
    fn variable_reference(&mut self, p: &mut usize) -> R<Option<Var<'a>>> {
        let mut i = *p;
        let Some(a) = self.variable(&mut i)? else {
            return Ok(None);
        };
        let mut k = i;
        let out;
        if self.lit(&mut k, b'[')? {
            let Some(index) = self.additive(&mut k)? else {
                return expected(k, "additive_expression");
            };
            // evaluate_index, onto the local _b (0).
            let mut b = 0i32;
            match index.v {
                Value::Empty => {}
                Value::Int(n) => b = n,
                _ => {
                    return Err(index
                        .throw("Non-integer index is not allowed to address a vector variable."));
                }
            }
            self.expect_lit(&mut k, b']')?;
            self.skip(&mut k)?;
            // store_variable_index
            let mut v = a;
            if !self.skipping() {
                if b < 0 {
                    b = 0;
                }
                let opt = v.opt.expect("resolved when not skipping");
                if !opt.ty().is_vector() {
                    b = -1;
                }
                v.index = b;
            }
            v.end = k;
            out = v;
            i = k;
        } else {
            // eps skips on the alternative's position.
            self.skip(&mut i)?;
            out = a;
        }
        *p = i;
        Ok(Some(out))
    }

    /// The text of a variable's range (`std::string(it_range.begin(), it_range.end())`).
    fn key_of(&self, var: &Var<'_>) -> String {
        String::from_utf8_lossy(&self.s[var.begin..var.end]).into_owned()
    }

    /// `variable_value`.
    fn variable_value(&self, var: &Var<'a>, out: &mut Expr) -> R<()> {
        if !self.skipping() {
            let opt = var.opt.expect("resolved when not skipping");
            if opt.ty().is_vector() {
                self.vector_element_to_expr(var, opt, out)?;
            } else {
                self.scalar_variable_to_expr(var, opt, out)?;
            }
        }
        out.begin = Some(var.begin);
        out.end = Some(var.end);
        Ok(())
    }

    /// The ratio chain of a percent (`ratio_over`), from `key`'s definition: the value times
    /// each parent's, as `scalar_variable_to_expr` (`elem` `None`) and `vector_element_to_expr`'s
    /// `resolve_float_or_percent` (`elem` the element's index) walk it.
    fn ratio_chain(&self, var: &Var<'_>, key: &str, value: f64, elem: Option<usize>) -> R<f64> {
        let undefined =
            || Fail::unsupported(format!("{key}: no definition to read ratio_over from"));
        let mut def = orca_config::get(key).ok_or_else(undefined)?;
        let mut v = value * 0.01;
        loop {
            let over = def.ratio_over;
            let parent = if over.is_empty() {
                None
            } else {
                self.resolve_symbol(over)
            };
            let Some(parent) = parent else {
                return Err(Fail::at(
                    Some(var.begin),
                    "FloatOrPercent variable failed to resolve the \"ratio_over\" dependencies",
                ));
            };
            if over.ends_with("line_width") {
                return Err(Fail::unsupported(format!(
                    "{key}: a ratio over {over}, which Flow::extrusion_width gives"
                )));
            }
            match (parent, elem) {
                (OptionValue::Float(f), _) => {
                    v *= f;
                    break;
                }
                (OptionValue::FloatOrPercent { value, percent }, _) => {
                    v *= value;
                    if !percent {
                        break;
                    }
                    v *= 0.01;
                }
                (OptionValue::Floats { values, .. }, Some(e)) => {
                    v *= if values.len() == 1 {
                        values[0]
                    } else {
                        get_at(values, e)?
                    };
                    break;
                }
                (OptionValue::FloatsOrPercents { values, .. }, Some(e)) => {
                    let (pv, pp) = if values.len() == 1 {
                        values[0]
                    } else {
                        get_at(values, e)?
                    };
                    if !pp {
                        v *= pv;
                        break;
                    }
                    v *= pv * 0.01;
                }
                _ => {}
            }
            def = orca_config::get(over).ok_or_else(undefined)?;
        }
        Ok(v)
    }

    /// `scalar_variable_to_expr`.
    fn scalar_variable_to_expr(&self, var: &Var<'_>, opt: &OptionValue, out: &mut Expr) -> R<()> {
        out.v = match opt {
            OptionValue::Float(f) | OptionValue::Percent(f) => Value::Double(*f),
            OptionValue::Int(i) => Value::Int(*i),
            OptionValue::String(s) => Value::Str(s.clone().into_bytes()),
            OptionValue::Enum { .. } | OptionValue::Point(_) => Value::Str(
                opt.serialize()
                    .map_err(|e| Fail::unsupported(format!("serialize: {e:?}")))?
                    .into_bytes(),
            ),
            OptionValue::Bool(b) => Value::Bool(*b),
            OptionValue::FloatOrPercent { value, percent } => {
                let key = self.key_of(var);
                if key.ends_with("line_width") {
                    return Err(Fail::unsupported(format!(
                        "{key}: Flow::extrusion_width is not ported"
                    )));
                }
                if *percent {
                    Value::Double(self.ratio_chain(var, &key, *value, None)?)
                } else {
                    Value::Double(*value)
                }
            }
            _ => {
                return Err(Fail::at(
                    Some(var.begin),
                    "Unsupported scalar variable type",
                ));
            }
        };
        Ok(())
    }

    /// `resolve_float_or_percent` of `vector_element_to_expr`.
    fn float_or_percent(&self, var: &Var<'_>, fop: (f64, bool), elem: usize) -> R<Value> {
        let key = self.key_of(var);
        if key.ends_with("line_width") {
            return Err(Fail::unsupported(format!(
                "{key}: Flow::extrusion_width is not ported"
            )));
        }
        if !fop.1 {
            return Ok(Value::Double(fop.0));
        }
        Ok(Value::Double(self.ratio_chain(
            var,
            &key,
            fop.0,
            Some(elem),
        )?))
    }

    /// `vector_element_to_expr`.
    fn vector_element_to_expr(&self, var: &Var<'_>, opt: &OptionValue, out: &mut Expr) -> R<()> {
        let at = Some(var.begin);
        if vector_len(opt) == 0 {
            return Err(Fail::at(at, "Indexing an empty vector variable"));
        }
        if !var.has_index() {
            // get_extruder_id() is 0: no external configuration.
            out.v = match opt {
                OptionValue::Floats { values, .. } => Value::Double(values[0]),
                OptionValue::FloatsOrPercents { values, .. } => {
                    self.float_or_percent(var, values[0], 0)?
                }
                _ => {
                    return Err(Fail::at(
                        at,
                        "Referencing a vector variable when scalar is expected",
                    ));
                }
            };
            return Ok(());
        }
        let size = vector_len(opt);
        let idx = if var.index < 0 || var.index as usize >= size {
            0
        } else {
            var.index as usize
        };
        if vector_is_nil(opt, idx) {
            return Err(Fail::at(
                at,
                "Trying to reference an undefined (nil) element of vector of optional values",
            ));
        }
        out.v = match opt {
            OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => {
                Value::Double(values[idx])
            }
            OptionValue::Ints { values, .. } | OptionValue::Enums { values, .. } => {
                Value::Int(values[idx])
            }
            OptionValue::Strings(values) => Value::Str(values[idx].clone().into_bytes()),
            OptionValue::Bools { values, .. } => Value::Bool(values[idx] != 0),
            OptionValue::FloatsOrPercents { values, .. } => {
                self.float_or_percent(var, values[idx], idx)?
            }
            OptionValue::Points(_) => {
                return Err(Fail::unsupported(
                    "an element of a points vector, which to_string(Vec2d) prints",
                ));
            }
            _ => return Err(Fail::at(at, "Unsupported vector variable type")),
        };
        Ok(())
    }

    /// `is_nil_test`.
    fn is_nil_test(&self, var: &Var<'_>, out: &mut Expr) -> R<()> {
        if !self.skipping() {
            let opt = var.opt.expect("resolved when not skipping");
            let at = Some(var.begin);
            if opt.ty().is_vector() {
                if !var.has_index() {
                    return Err(Fail::at(
                        at,
                        "Referencing a vector variable when scalar is expected",
                    ));
                }
                let size = vector_len(opt);
                if size == 0 {
                    return Err(Fail::at(at, "Indexing an empty vector variable"));
                }
                let idx = if var.index as usize >= size {
                    0
                } else {
                    var.index as usize
                };
                out.v = Value::Bool(vector_is_nil(opt, idx));
            } else {
                // A scalar option is never nil.
                out.v = Value::Bool(false);
            }
        }
        out.begin = Some(var.begin);
        out.end = Some(var.end);
        Ok(())
    }

    /// `is_vector_empty` (`size` false) and `vector_size` (`size` true).
    fn vector_empty_or_size(&self, var: &Var<'_>, out: &mut Expr, size: bool) -> R<()> {
        if !self.skipping() {
            let opt = var.opt.expect("resolved when not skipping");
            if var.has_index() || !opt.ty().is_vector() {
                let msg = if size {
                    "parameter of size() is not a vector variable"
                } else {
                    "parameter of empty() is not a vector variable"
                };
                return Err(Fail::at(Some(var.begin), msg));
            }
            let n = vector_len(opt);
            out.v = if size {
                Value::Int(i32::try_from(n).unwrap_or(i32::MAX))
            } else {
                Value::Bool(n == 0)
            };
        }
        out.begin = Some(var.begin);
        out.end = Some(var.end);
        Ok(())
    }
}

/// `ConfigOptionVectorBase::size()`.
fn vector_len(opt: &OptionValue) -> usize {
    match opt {
        OptionValue::Floats { values, .. } | OptionValue::Percents { values, .. } => values.len(),
        OptionValue::Ints { values, .. } | OptionValue::Enums { values, .. } => values.len(),
        OptionValue::Strings(v) => v.len(),
        OptionValue::FloatsOrPercents { values, .. } => values.len(),
        OptionValue::Points(v) => v.len(),
        OptionValue::Bools { values, .. } => values.len(),
        OptionValue::PointsGroups(v) => v.len(),
        OptionValue::IntsGroups(v) => v.len(),
        _ => 0,
    }
}

/// `ConfigOptionVectorBase::is_nil(idx)`: a nullable vector's nil element.
fn vector_is_nil(opt: &OptionValue, idx: usize) -> bool {
    match opt {
        OptionValue::Floats {
            values,
            nullable: true,
        }
        | OptionValue::Percents {
            values,
            nullable: true,
        } => values[idx].is_nan(),
        OptionValue::Ints {
            values,
            nullable: true,
        } => values[idx] == i32::MAX,
        OptionValue::Enums {
            values,
            nullable: true,
            ..
        } => values[idx] == i32::MAX,
        OptionValue::FloatsOrPercents {
            values,
            nullable: true,
        } => values[idx].0.is_nan(),
        OptionValue::Bools {
            values,
            nullable: true,
        } => values[idx] == 255,
        _ => false,
    }
}

/// `boost::nowide::narrow(boost::nowide::widen(line))` (Boost 1.84 `utf::convert_string` with
/// `utf_traits<char, 1>::decode`): each sequence that does not decode, as `decode` delimits it, one
/// U+FFFD. A bad continuation byte is swallowed with its lead (`decode` reads it before the check),
/// while an overlong or out-of-range sequence gives back its trail bytes.
fn nowide_round_trip(line: &[u8]) -> String {
    let mut out = String::with_capacity(line.len());
    let mut p = 0usize;
    while p < line.len() {
        let lead = line[p];
        p += 1;
        let trail = match lead {
            0..=127 => {
                out.push(char::from(lead));
                continue;
            }
            128..=193 => None,
            194..=223 => Some(1usize),
            224..=239 => Some(2),
            240..=244 => Some(3),
            _ => None,
        };
        let Some(trail) = trail else {
            out.push('\u{FFFD}');
            continue;
        };
        let mut c = u32::from(lead) & ((1u32 << (6 - trail)) - 1);
        let mut ok = true;
        for _ in 0..trail {
            if p == line.len() {
                // incomplete
                ok = false;
                break;
            }
            let t = line[p];
            p += 1;
            if t & 0xC0 != 0x80 {
                ok = false;
                break;
            }
            c = (c << 6) | u32::from(t & 0x3F);
        }
        if !ok {
            out.push('\u{FFFD}');
            continue;
        }
        let width = match c {
            0..=0x7F => 1,
            0x80..=0x7FF => 2,
            0x800..=0xFFFF => 3,
            _ => 4,
        };
        match char::from_u32(c) {
            Some(ch) if width == trail + 1 => out.push(ch),
            _ => {
                p -= trail;
                out.push('\u{FFFD}');
            }
        }
    }
    out
}

/// `process_error_message(info, it_begin, it_end, it_error)` for the template `s`.
pub fn error_message(s: &[u8], at: Pos, tag: &str) -> Result<String, Fail> {
    let Some(at) = at else {
        return Err(Fail::unsupported(
            "an error at a null iterator, where process_error_message throws std::length_error",
        ));
    };
    let first = &s[..at];
    let last = &s[at..];
    let mut line_nr = 1usize;
    let first_pos = match first.iter().rposition(|&c| c == b'\n') {
        None => 0,
        Some(fp) => {
            line_nr += first[..=fp].iter().filter(|&&c| c == b'\n').count();
            fp + 1
        }
    };
    let last_pos = last.iter().position(|&c| c == b'\n').unwrap_or(last.len());
    let mut error_line = first[first_pos..].to_vec();
    error_line.extend_from_slice(&last[..last_pos]);
    let error_pos = at - first_pos;
    let mut msg = format!("Parsing error at line {line_nr}");
    if let Some(free) = tag.strip_prefix('*') {
        msg.push_str(": ");
        msg.push_str(free);
    } else if let Some((_, m)) = TAG_MESSAGES.iter().find(|(t, _)| *t == tag) {
        msg.push_str(". ");
        msg.push_str(m);
    } else {
        msg.push_str(". Expecting tag ");
        msg.push_str(tag);
    }
    msg.push('\n');
    // "This hack removes all non-UTF8 characters from the source line": narrow(widen(line)).
    msg.push_str(&nowide_round_trip(&error_line));
    msg.push('\n');
    msg.push_str(&" ".repeat(error_pos));
    msg.push_str("^\n");
    Ok(msg)
}
