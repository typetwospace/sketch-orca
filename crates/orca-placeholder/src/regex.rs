// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Boost code: boost/regex/v5/basic_regex_parser.hpp (parse, fail, parse_all,
// parse_extended, parse_open_paren, parse_extended_escape, unescape_character, parse_repeat,
// parse_repeat_range, parse_alt, parse_set, parse_inner_set, parse_set_literal,
// get_next_set_literal), the default error messages and escape syntax types of
// regex_traits_defaults.hpp, the escape syntax and the one-letter class names of
// w32_regex_traits.hpp, and the line and wildcard tests of perl_matcher_common.hpp, of Boost
// 1.84.0, translated on 2026-11-07. Copyright (c) 1998-2007 John Maddock, under the Boost Software
// License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! `boost::regex` as the placeholder parser uses it: `boost::regex(pattern)` (perl syntax, the
//! default flags) and `boost::regex_match(subject, re)`, with the narrow `w32_regex_traits` of an
//! MSVC build.
//!
//! [`Regex::new`] parses a pattern as `basic_regex_parser` does and fails as it does: the text of
//! the `regex_error` it throws ([`RegexError::Error`], what `what()` returns), with its message,
//! its fragment and its `>>>HERE>>>` marker at the same position. [`Regex::is_match`] answers
//! `regex_match`: whether some way through the pattern spans the whole subject, which a Thompson
//! simulation of the pattern gives exactly where the pattern has no back-references or
//! look-arounds (Boost's backtracking finds a full match whenever one exists).
//!
//! What is taken: literals; `.` (any byte, a newline included, as `match_default` and no `mod_s`
//! flag give); `^` and `$` (the start and end of a line: `\n`, `\r` and `\f` end a line, and no
//! line ends between `\r` and `\n`); sets with ranges, negation and the one-letter classes of
//! `w32_regex_traits` (`\d`, `\w`, `\s`, `\l`, `\u`, `\h`, `\v` and their negations; a set's
//! negated classes joined as Boost joins them); those classes outside sets; escapes of
//! punctuation, the character escapes `\a`, `\e`, `\f`, `\n`, `\r`, `\t` and `\cX` (in a set also
//! `\b` and `\v`), and a letter that names nothing, as itself; groups `(...)` and `(?:...)`; `|`
//! (an empty alternative allowed, as perl syntax does); `*`, `+`, `?`, `{n}`, `{n,}`, `{n,m}` and
//! their lazy forms (a `{` that does not make a repeat is a literal, as perl syntax has it).
//! Everything else Boost reads is [`RegexError::Unsupported`], so that the job declines instead of
//! guessing: back-references, `\b` and the other assertions, named classes (`[[:alpha:]]`),
//! collating elements, perl extensions other than `(?:`, verbs, possessive repeats, `\Q..\E`, hex,
//! octal and named character escapes (`\x`, `\0`, `\N`), a class tested against a byte past ASCII
//! (the Windows locale decides those), more than 399 nested groups and repeats past 1000.
//! Boost's limit on the work of a match (`error_complexity`) is not modelled
//! (`parity/HAZARDS.md`, orca-placeholder).

/// What compiling a pattern or matching it gave besides a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegexError {
    /// The `regex_error` Boost throws, as `what()` gives it.
    Error(String),
    /// A pattern or subject outside what this module takes (see the module's notes).
    Unsupported(String),
}

/// `regex_constants::error_type`, the codes this module fails with.
#[derive(Clone, Copy)]
enum Code {
    Collate = 3,
    Ctype = 4,
    Escape = 5,
    Brack = 7,
    Paren = 8,
    Badbrace = 10,
    Range = 11,
    Badrepeat = 13,
}

/// `get_default_error_string(code)`.
fn default_message(code: Code) -> &'static str {
    match code {
        Code::Collate => "Invalid collation character.",
        Code::Ctype => "Invalid character class name, collating name, or character range.",
        Code::Escape => "Invalid or unterminated escape sequence.",
        Code::Brack => "Unmatched [ or [^ in character class declaration.",
        Code::Paren => "Unmatched marking parenthesis ( or \\(.",
        Code::Badbrace => "Invalid content of repeat range.",
        Code::Range => "Invalid range end in character class",
        Code::Badrepeat => "Invalid preceding regular expression prior to repetition operator.",
    }
}

/// A byte set: a 256-entry map, as Boost builds a narrow set, and whether a class went into it.
#[derive(Clone)]
struct Set {
    map: Box<[bool; 256]>,
    class: bool,
}

/// The pattern, parsed.
#[derive(Clone)]
enum Node {
    Byte(u8),
    Any,
    Set(Set),
    StartLine,
    EndLine,
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Node>, usize, Option<usize>),
}

/// What the last element parsed was, as `m_last_state` tells `parse_repeat`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    /// A literal, a wildcard, a set or a closed group: something a repeat applies to.
    Atom,
    /// The start of the pattern or of a group, an alternation, an assertion or a repeat.
    Other,
}

/// The largest repeat count taken.
const MAX_REPEAT: usize = 1000;
/// The largest number of NFA states taken.
const MAX_STATES: usize = 200_000;

/// Whether `c` is a line separator (`is_separator<char>`).
fn is_separator(c: u8) -> bool {
    c == b'\n' || c == b'\r' || c == 0x0C
}

/// `isctype(c, space)` for an ASCII byte (`C1_SPACE`).
fn is_space(c: u8) -> bool {
    c == b' ' || (0x09..=0x0D).contains(&c)
}

/// `isctype(c, word)` for an ASCII byte (alnum and `_`).
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// A class's membership test for an ASCII byte.
type ClassTest = fn(u8) -> bool;

/// The ASCII members of the class a one-letter name gives (`lookup_classname`, whose default
/// names of one letter are `d`, `h`, `l`, `s`, `u`, `v` and `w`; an upper-case letter is looked up
/// again in lower case), if it names one. `h` is `C1_SPACE` without the vertical bytes, `v` the
/// line separators and `\v` (`w32_regex_traits::isctype`).
fn class_of(letter: u8) -> Option<ClassTest> {
    match letter.to_ascii_lowercase() {
        b'd' => Some(|c: u8| c.is_ascii_digit()),
        b'w' => Some(is_word),
        b's' => Some(is_space),
        b'l' => Some(|c: u8| c.is_ascii_lowercase()),
        b'u' => Some(|c: u8| c.is_ascii_uppercase()),
        b'h' => Some(|c: u8| is_space(c) && !is_vertical(c)),
        b'v' => Some(is_vertical),
        _ => None,
    }
}

/// The `mask_vertical` class: `is_separator<char>` or `\v`.
fn is_vertical(c: u8) -> bool {
    is_separator(c) || c == 0x0B
}

/// Whether an ASCII letter has a syntax of its own in `w32_regex_traits<char>`'s map (the default
/// syntax table's letters). The other letters are `escape_type_class` (lower case, `C1_LOWER`) or
/// `escape_type_not_class` (upper case, `C1_UPPER`).
fn has_letter_syntax(c: u8) -> bool {
    matches!(
        c,
        b'a' | b'b'
            | b'c'
            | b'e'
            | b'f'
            | b'g'
            | b'k'
            | b'n'
            | b'p'
            | b'r'
            | b't'
            | b'v'
            | b'x'
            | b'z'
            | b'A'
            | b'B'
            | b'C'
            | b'E'
            | b'G'
            | b'K'
            | b'N'
            | b'P'
            | b'Q'
            | b'R'
            | b'X'
            | b'Z'
    )
}

/// The class an escape names where Boost tries one (`escape_type_class` or `escape_type_not_class`
/// and a `lookup_classname` that finds it): the class's test and whether it is negated.
fn escape_class(c: u8) -> Option<(ClassTest, bool)> {
    if !c.is_ascii_alphabetic() || has_letter_syntax(c) {
        return None;
    }
    class_of(c).map(|test| (test, c.is_ascii_uppercase()))
}

/// The parser: `basic_regex_parser` for the perl syntax.
struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    depth: usize,
}

type Res<T> = Result<T, RegexError>;

impl Parser<'_> {
    /// `fail(code, position, message, start_pos)`, with `start_pos` equal to `position` as the
    /// three-argument overload passes it.
    fn fail<T>(&self, code: Code, position: usize, message: Option<&str>) -> Res<T> {
        let len = self.s.len();
        let mut msg: Vec<u8> = message.unwrap_or_else(|| default_message(code)).into();
        let start = position.saturating_sub(10);
        let end = (position + 10).min(len);
        if start != 0 || end != len {
            msg.extend_from_slice(
                b"  The error occurred while parsing the regular expression fragment: '",
            );
        } else {
            msg.extend_from_slice(b"  The error occurred while parsing the regular expression: '");
        }
        if start != end {
            msg.extend_from_slice(&self.s[start..position]);
            msg.extend_from_slice(b">>>HERE>>>");
            msg.extend_from_slice(&self.s[position..end]);
        }
        msg.extend_from_slice(b"'.");
        // what() holds the pattern's own bytes, cut ten either side of the position; where they
        // are not UTF-8 a Rust string cannot hold them, so the job declines instead.
        match String::from_utf8(msg) {
            Ok(m) => Err(RegexError::Error(m)),
            Err(_) => Err(RegexError::Unsupported(
                "a regex error whose text is not UTF-8".into(),
            )),
        }
    }

    fn unsupported<T>(&self, what: &str) -> Res<T> {
        Err(RegexError::Unsupported(format!(
            "{what} (at {} of the regular expression)",
            self.pos
        )))
    }

    /// `parse_all` with `parse_extended`: a run of alternatives up to a `)` or the end. Returns the
    /// node and whether the run ended on a `)` or a parser that returned false (`parse_set` at
    /// the end) rather than at the end.
    fn parse_all(&mut self) -> Res<(Node, bool)> {
        self.depth += 1;
        if self.depth > 400 {
            return self.unsupported("more than 399 nested groups");
        }
        let mut alts: Vec<Node> = Vec::new();
        let mut cur: Vec<Node> = Vec::new();
        let mut last = Last::Other;
        let mut stopped = false;
        while self.pos != self.s.len() {
            let c = self.s[self.pos];
            match c {
                b'(' => {
                    let node = self.parse_open_paren()?;
                    cur.push(node);
                    last = Last::Atom;
                }
                b')' => {
                    stopped = true;
                    break;
                }
                b'\\' => {
                    let node = self.parse_extended_escape()?;
                    last = match node {
                        Node::StartLine | Node::EndLine => Last::Other,
                        _ => Last::Atom,
                    };
                    cur.push(node);
                }
                b'.' => {
                    self.pos += 1;
                    cur.push(Node::Any);
                    last = Last::Atom;
                }
                b'^' => {
                    self.pos += 1;
                    cur.push(Node::StartLine);
                    last = Last::Other;
                }
                b'$' => {
                    self.pos += 1;
                    cur.push(Node::EndLine);
                    last = Last::Other;
                }
                b'*' | b'?' | b'+' => {
                    if self.pos == 0 {
                        let msg = format!(
                            "The repeat operator \"{}\" cannot start a regular expression.",
                            c as char
                        );
                        return self.fail(Code::Badrepeat, 0, Some(&msg));
                    }
                    self.pos += 1;
                    let (low, high) = match c {
                        b'*' => (0, None),
                        b'?' => (0, Some(1)),
                        _ => (1, None),
                    };
                    self.parse_repeat(&mut cur, &mut last, low, high)?;
                }
                b'{' => {
                    self.pos += 1;
                    self.parse_repeat_range(&mut cur, &mut last)?;
                }
                b'|' => {
                    self.pos += 1;
                    alts.push(Node::Concat(std::mem::take(&mut cur)));
                    last = Last::Other;
                }
                b'[' => match self.parse_set()? {
                    Some(set) => {
                        cur.push(Node::Set(set));
                        last = Last::Atom;
                    }
                    None => {
                        stopped = true;
                        break;
                    }
                },
                _ => {
                    // parse_literal: '}', '#' and a newline are literals too in perl syntax
                    // without mod_x and newline_alt.
                    self.pos += 1;
                    cur.push(Node::Byte(c));
                    last = Last::Atom;
                }
            }
        }
        self.depth -= 1;
        alts.push(Node::Concat(cur));
        let node = if alts.len() == 1 {
            alts.pop().expect("one alternative")
        } else {
            Node::Alt(alts)
        };
        Ok((node, stopped))
    }

    /// `parse_open_paren`, at the `(`.
    fn parse_open_paren(&mut self) -> Res<Node> {
        self.pos += 1;
        if self.pos == self.s.len() {
            return self.fail(Code::Paren, self.pos, None);
        }
        let mut perl = false;
        match self.s[self.pos] {
            b'?' => {
                if self.pos + 1 < self.s.len() && self.s[self.pos + 1] == b':' {
                    self.pos += 2;
                    perl = true;
                } else {
                    return self.unsupported("a perl extension other than (?:");
                }
            }
            b'*' => return self.unsupported("a perl verb"),
            _ => {}
        }
        let (node, _) = self.parse_all()?;
        if self.pos == self.s.len() {
            if perl {
                return self.unsupported("an unclosed (?:");
            }
            return self.fail(Code::Paren, self.s.len(), None);
        }
        if self.s[self.pos] != b')' {
            return self.unsupported("a group that ends on a set that runs to the end");
        }
        self.pos += 1;
        Ok(node)
    }

    /// `parse_extended_escape`, at the `\`.
    fn parse_extended_escape(&mut self) -> Res<Node> {
        self.pos += 1;
        if self.pos == self.s.len() {
            return self.fail(
                Code::Escape,
                self.pos,
                Some("Incomplete escape sequence found."),
            );
        }
        let c = self.s[self.pos];
        // \v is escape_type_control_v, which perl syntax sends to the class lookup.
        let class = if c == b'v' {
            class_of(c).map(|t| (t, false))
        } else {
            escape_class(c)
        };
        if let Some((test, negate)) = class {
            let mut map = Box::new([false; 256]);
            for (b, m) in map.iter_mut().enumerate().take(128) {
                *m = test(b as u8) != negate;
            }
            if negate {
                for m in map.iter_mut().skip(128) {
                    *m = true;
                }
            }
            self.pos += 1;
            return Ok(Node::Set(Set { map, class: true }));
        }
        // The rest that reach unescape_character: a letter of the class kind that names no class,
        // the control letters, `E` and the bytes with no syntax. The anchors (`\b`, `\B`, `\A`,
        // `\z`, `\Z`, `\<`, `\>`, `\``, `\'`, `\G`), back references, `\Q`, `\C`, `\X`,
        // `\p`, `\P`, `\K`, `\R`, `\g` and `\k` are not taken.
        let literal = match c {
            b'a' | b'e' | b'f' | b'n' | b'r' | b't' | b'c' | b'x' | b'N' | b'E' => true,
            b'0'..=b'9' => false,
            _ if c.is_ascii_alphabetic() => !has_letter_syntax(c),
            b'\'' | b'`' | b'<' | b'>' => false,
            _ => true,
        };
        if literal {
            return Ok(Node::Byte(self.unescape_character()?));
        }
        self.unsupported("an escape this module does not take")
    }

    /// `unescape_character`, after a `\`: the byte an escape stands for.
    fn unescape_character(&mut self) -> Res<u8> {
        if self.pos == self.s.len() {
            return self.fail(
                Code::Escape,
                self.pos,
                Some("Escape sequence terminated prematurely."),
            );
        }
        let c = self.s[self.pos];
        let r = match c {
            b'a' => 0x07,
            b'e' => 27,
            b'f' => 0x0C,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0B,
            b'b' => 0x08,
            b'c' => {
                self.pos += 1;
                if self.pos == self.s.len() {
                    // Rewound to the backslash.
                    return self.fail(
                        Code::Escape,
                        self.pos - 2,
                        Some("ASCII escape sequence terminated prematurely."),
                    );
                }
                // `*m_position % 32` on MSVC's signed char.
                ((self.s[self.pos] as i8) % 32) as u8
            }
            b'x' | b'0'..=b'9' | b'N' => {
                return self.unsupported("a hexadecimal, octal or named character escape");
            }
            _ => c,
        };
        self.pos += 1;
        Ok(r)
    }

    /// `parse_repeat(low, high)`, after the operator.
    fn parse_repeat(
        &mut self,
        cur: &mut Vec<Node>,
        last: &mut Last,
        low: usize,
        high: Option<usize>,
    ) -> Res<()> {
        let mut possessive = false;
        if self.pos != self.s.len() && self.s[self.pos] == b'?' {
            self.pos += 1;
        }
        if self.pos != self.s.len() && self.s[self.pos] == b'+' {
            possessive = true;
            self.pos += 1;
        }
        if *last != Last::Atom {
            return self.fail(Code::Badrepeat, self.pos, None);
        }
        if possessive {
            return self.unsupported("a possessive repeat");
        }
        if low > MAX_REPEAT || high.is_some_and(|h| h > MAX_REPEAT) {
            return self.unsupported("a repeat count past 1000");
        }
        let atom = cur.pop().expect("an atom to repeat");
        cur.push(Node::Repeat(Box::new(atom), low, high));
        *last = Last::Other;
        Ok(())
    }

    /// `isctype(c, space)` where a byte past ASCII would ask the locale.
    fn space_at(&self, i: usize) -> Res<bool> {
        let c = self.s[i];
        if c >= 0x80 {
            return self.unsupported("a byte past ASCII in a repeat range");
        }
        Ok(is_space(c))
    }

    fn skip_spaces(&mut self) -> Res<()> {
        while self.pos != self.s.len() && self.space_at(self.pos)? {
            self.pos += 1;
        }
        Ok(())
    }

    /// `global_toi(p1, p2, 10)`: the digits at the position, -1 when there are none or the value
    /// passes `INTMAX_MAX / 10` (having moved past the digits read).
    fn toi(&mut self) -> i64 {
        let limit = i64::MAX / 10;
        if self.pos == self.s.len() || !self.s[self.pos].is_ascii_digit() {
            return -1;
        }
        let mut result: i64 = 0;
        while self.pos != self.s.len() {
            let c = self.s[self.pos];
            if !c.is_ascii_digit() {
                break;
            }
            result = result * 10 + i64::from(c - b'0');
            self.pos += 1;
            if result > limit {
                return -1;
            }
        }
        result
    }

    /// The perl syntax's way out of a `{` that is not a repeat: back to the `{`, read as a
    /// literal.
    fn brace_literal(&mut self, cur: &mut Vec<Node>, last: &mut Last) -> Res<()> {
        self.pos -= 1;
        while self.s[self.pos] != b'{' {
            self.pos -= 1;
        }
        self.pos += 1;
        cur.push(Node::Byte(b'{'));
        *last = Last::Atom;
        Ok(())
    }

    /// `parse_repeat_range(false)`, after the `{`.
    fn parse_repeat_range(&mut self, cur: &mut Vec<Node>, last: &mut Last) -> Res<()> {
        self.skip_spaces()?;
        if self.pos == self.s.len() {
            return self.brace_literal(cur, last);
        }
        let v = self.toi();
        if v < 0 {
            return self.brace_literal(cur, last);
        }
        self.skip_spaces()?;
        if self.pos == self.s.len() {
            return self.brace_literal(cur, last);
        }
        let min = v as u64;
        let max = if self.s[self.pos] == b',' {
            self.pos += 1;
            self.skip_spaces()?;
            if self.pos == self.s.len() {
                return self.brace_literal(cur, last);
            }
            let v = self.toi();
            if (0..i64::MAX).contains(&v) {
                Some(v as u64)
            } else {
                None
            }
        } else {
            Some(min)
        };
        self.skip_spaces()?;
        if self.pos == self.s.len() {
            return self.brace_literal(cur, last);
        }
        if self.s[self.pos] == b'}' {
            self.pos += 1;
        } else {
            return self.brace_literal(cur, last);
        }
        if max.is_some_and(|m| min > m) {
            let mut p = self.pos - 2;
            while self.s[p] < 0x80 && is_word(self.s[p]) {
                p -= 1;
            }
            p += 1;
            return self.fail(Code::Badbrace, p, None);
        }
        let to_count = |v: u64| usize::try_from(v).unwrap_or(usize::MAX);
        self.parse_repeat(cur, last, to_count(min), max.map(to_count))
    }

    /// `parse_set`, at the `[`. `None` where Boost's returns false: the pattern ended inside the
    /// set after a `[^`.
    fn parse_set(&mut self) -> Res<Option<Set>> {
        self.pos += 1;
        if self.pos == self.s.len() {
            return self.fail(
                Code::Brack,
                self.pos,
                Some("Character set declaration starting with [ terminated prematurely - either no ] was found or the set had no content."),
            );
        }
        let mut b = SetBuilder::default();
        let base = self.pos;
        let mut item_base = self.pos;
        while self.pos != self.s.len() {
            match self.s[self.pos] {
                b'^' if self.pos == base => {
                    b.negate = true;
                    self.pos += 1;
                    item_base = self.pos;
                }
                b']' if self.pos != item_base => {
                    self.pos += 1;
                    if b.bad_range {
                        return self.fail(Code::Ctype, self.pos, None);
                    }
                    return Ok(Some(b.finish()));
                }
                b'[' => self.parse_inner_set(&mut b)?,
                b'\\' => {
                    self.pos += 1;
                    // At the end Boost reads the pattern's terminating NUL, which names no class.
                    if let Some((test, negated)) =
                        self.s.get(self.pos).and_then(|&c| escape_class(c))
                    {
                        b.add_class(test, negated);
                        self.pos += 1;
                    } else {
                        self.pos -= 1;
                        self.parse_set_literal(&mut b)?;
                    }
                }
                _ => self.parse_set_literal(&mut b)?,
            }
        }
        Ok(None)
    }

    /// `parse_inner_set`, at a `[` inside a set.
    fn parse_inner_set(&mut self, b: &mut SetBuilder) -> Res<()> {
        self.pos += 1;
        if self.pos == self.s.len() {
            return self.fail(
                Code::Brack,
                self.pos,
                Some("Character class declaration starting with [ terminated prematurely - either no ] was found or the set had no content."),
            );
        }
        match self.s[self.pos] {
            b':' | b'=' => self.unsupported("a named class or an equivalence class"),
            _ => {
                self.pos -= 1;
                self.parse_set_literal(b)
            }
        }
    }

    /// `parse_set_literal`.
    fn parse_set_literal(&mut self, b: &mut SetBuilder) -> Res<()> {
        let start = self.get_next_set_literal(b)?;
        if self.pos == self.s.len() {
            return self.fail(Code::Brack, self.pos, None);
        }
        if self.s[self.pos] == b'-' {
            self.pos += 1;
            if self.pos == self.s.len() {
                return self.fail(Code::Brack, self.pos, None);
            }
            if self.s[self.pos] != b']' {
                let end = self.get_next_set_literal(b)?;
                b.add_range(start, end);
                // Boost reads the byte at the end of the pattern here (its terminating NUL),
                // which is no dash.
                if self.pos != self.s.len() && self.s[self.pos] == b'-' {
                    self.pos += 1;
                    if self.pos == self.s.len() {
                        return self.fail(Code::Brack, self.pos, None);
                    }
                    if self.s[self.pos] == b']' {
                        self.pos -= 1;
                        return Ok(());
                    }
                    return self.fail(Code::Range, self.pos, None);
                }
                return Ok(());
            }
            self.pos -= 1;
        }
        b.add_single(start);
        Ok(())
    }

    /// `get_next_set_literal`.
    fn get_next_set_literal(&mut self, b: &SetBuilder) -> Res<u8> {
        match self.s[self.pos] {
            b'-' => {
                if b.started {
                    self.pos += 1;
                    if self.pos == self.s.len() || self.s[self.pos] != b']' {
                        return self.fail(Code::Range, self.pos, None);
                    }
                    self.pos -= 1;
                }
                self.pos += 1;
                Ok(b'-')
            }
            b'\\' => {
                self.pos += 1;
                self.unescape_character()
            }
            b'[' => {
                self.pos += 1;
                if self.pos == self.s.len() {
                    return self.fail(Code::Collate, self.pos, None);
                }
                if self.s[self.pos] != b'.' {
                    return Ok(b'[');
                }
                self.unsupported("a collating element")
            }
            c => {
                self.pos += 1;
                Ok(c)
            }
        }
    }
}

/// `basic_char_set` and `append_set` for a narrow set.
#[derive(Default)]
struct SetBuilder {
    singles: Vec<u8>,
    ranges: Vec<(u8, u8)>,
    classes: Vec<(ClassTest, bool)>,
    negate: bool,
    bad_range: bool,
    started: bool,
}

impl SetBuilder {
    fn touch(&mut self) {
        self.started = true;
    }
    fn add_single(&mut self, c: u8) {
        self.touch();
        self.singles.push(c);
    }
    fn add_range(&mut self, a: u8, b: u8) {
        self.touch();
        if b < a {
            self.bad_range = true;
        }
        self.ranges.push((a, b));
    }
    fn add_class(&mut self, test: ClassTest, negated: bool) {
        self.touch();
        self.classes.push((test, negated));
    }
    fn finish(self) -> Set {
        let mut map = Box::new([false; 256]);
        for &c in &self.singles {
            map[usize::from(c)] = true;
        }
        for &(a, b) in &self.ranges {
            for c in a..=b {
                map[usize::from(c)] = true;
            }
        }
        // The classes join into one mask, and so do the negated ones: a byte is in the set where it
        // is in any class, or where it is in none of the negated classes (`append_set`), so
        // `[\H\W]` is what is neither a blank nor a word byte. Bytes past ASCII come from the
        // locale; matching one declines (Regex::is_match), so their values here are never read.
        let in_any = |c: u8, negated: bool| {
            self.classes
                .iter()
                .any(|&(test, n)| n == negated && test(c))
        };
        let has_negated = self.classes.iter().any(|&(_, n)| n);
        for (c, m) in map.iter_mut().enumerate().take(128) {
            let c = c as u8;
            if in_any(c, false) || (has_negated && !in_any(c, true)) {
                *m = true;
            }
        }
        if has_negated {
            for m in map.iter_mut().skip(128) {
                *m = true;
            }
        }
        if self.negate {
            for m in map.iter_mut() {
                *m = !*m;
            }
        }
        Set {
            map,
            class: !self.classes.is_empty(),
        }
    }
}

/// One state of the NFA.
#[derive(Clone)]
enum State {
    Byte(u8, usize),
    Any(usize),
    Set(usize, usize),
    StartLine(usize),
    EndLine(usize),
    Split(usize, usize),
    Jump(usize),
    Match,
}

/// A compiled pattern.
#[derive(Clone)]
pub struct Regex {
    states: Vec<State>,
    sets: Vec<[bool; 256]>,
    start: usize,
    class: bool,
}

/// The NFA's builder: each fragment is (entry, list of dangling exits to patch).
struct Builder {
    states: Vec<State>,
    sets: Vec<[bool; 256]>,
    class: bool,
}

const HOLE: usize = usize::MAX;

impl Builder {
    fn push(&mut self, s: State) -> Res<usize> {
        if self.states.len() >= MAX_STATES {
            return Err(RegexError::Unsupported(
                "a regular expression too large to match".into(),
            ));
        }
        self.states.push(s);
        Ok(self.states.len() - 1)
    }

    fn patch(&mut self, holes: &[(usize, u8)], to: usize) {
        for &(i, which) in holes {
            match &mut self.states[i] {
                State::Byte(_, n)
                | State::Any(n)
                | State::Set(_, n)
                | State::StartLine(n)
                | State::EndLine(n)
                | State::Jump(n) => *n = to,
                State::Split(a, b) => {
                    if which == 0 {
                        *a = to;
                    } else {
                        *b = to;
                    }
                }
                State::Match => {}
            }
        }
    }

    /// Compiles `node`; returns its entry and its exits.
    fn build(&mut self, node: &Node) -> Res<(usize, Vec<(usize, u8)>)> {
        Ok(match node {
            Node::Byte(c) => {
                let i = self.push(State::Byte(*c, HOLE))?;
                (i, vec![(i, 0)])
            }
            Node::Any => {
                let i = self.push(State::Any(HOLE))?;
                (i, vec![(i, 0)])
            }
            Node::Set(set) => {
                self.class |= set.class;
                self.sets.push(*set.map);
                let i = self.push(State::Set(self.sets.len() - 1, HOLE))?;
                (i, vec![(i, 0)])
            }
            Node::StartLine => {
                let i = self.push(State::StartLine(HOLE))?;
                (i, vec![(i, 0)])
            }
            Node::EndLine => {
                let i = self.push(State::EndLine(HOLE))?;
                (i, vec![(i, 0)])
            }
            Node::Concat(items) => {
                if items.is_empty() {
                    let i = self.push(State::Jump(HOLE))?;
                    return Ok((i, vec![(i, 0)]));
                }
                let (entry, mut exits) = self.build(&items[0])?;
                for item in &items[1..] {
                    let (e, x) = self.build(item)?;
                    self.patch(&exits, e);
                    exits = x;
                }
                (entry, exits)
            }
            Node::Alt(alts) => {
                let mut exits = Vec::new();
                let (mut entry, x) = self.build(&alts[alts.len() - 1])?;
                exits.extend(x);
                for alt in alts[..alts.len() - 1].iter().rev() {
                    let (e, x) = self.build(alt)?;
                    exits.extend(x);
                    entry = self.push(State::Split(e, entry))?;
                }
                (entry, exits)
            }
            Node::Repeat(atom, low, high) => {
                // low copies, then (high - low) optional copies, or a loop when unbounded.
                let mut entry: Option<usize> = None;
                let mut exits: Vec<(usize, u8)> = Vec::new();
                let link = |b: &mut Builder,
                            entry: &mut Option<usize>,
                            exits: &mut Vec<(usize, u8)>,
                            e: usize,
                            x: Vec<(usize, u8)>| {
                    match entry {
                        None => *entry = Some(e),
                        Some(_) => b.patch(exits, e),
                    }
                    *exits = x;
                };
                for _ in 0..*low {
                    let (e, x) = self.build(atom)?;
                    link(self, &mut entry, &mut exits, e, x);
                }
                match high {
                    None => {
                        let split = self.push(State::Split(HOLE, HOLE))?;
                        let (e, x) = self.build(atom)?;
                        self.patch(&x, split);
                        if let State::Split(a, _) = &mut self.states[split] {
                            *a = e;
                        }
                        link(self, &mut entry, &mut exits, split, vec![(split, 1)]);
                    }
                    Some(h) => {
                        let mut skips: Vec<(usize, u8)> = Vec::new();
                        for _ in *low..*h {
                            let split = self.push(State::Split(HOLE, HOLE))?;
                            let (e, x) = self.build(atom)?;
                            if let State::Split(a, _) = &mut self.states[split] {
                                *a = e;
                            }
                            link(self, &mut entry, &mut exits, split, x);
                            skips.push((split, 1));
                        }
                        exits.extend(skips);
                    }
                }
                match entry {
                    Some(e) => (e, exits),
                    None => {
                        let i = self.push(State::Jump(HOLE))?;
                        (i, vec![(i, 0)])
                    }
                }
            }
        })
    }
}

impl Regex {
    /// `boost::regex(pattern)`.
    ///
    /// # Errors
    ///
    /// The `regex_error` Boost throws ([`RegexError::Error`]), or a pattern outside what this
    /// module takes ([`RegexError::Unsupported`]).
    pub fn new(pattern: &[u8]) -> Result<Regex, RegexError> {
        let mut p = Parser {
            s: pattern,
            pos: 0,
            depth: 0,
        };
        let (node, stopped) = p.parse_all()?;
        if stopped {
            return p.fail(
                Code::Paren,
                p.pos,
                Some("Found a closing ) with no corresponding opening parenthesis."),
            );
        }
        let mut b = Builder {
            states: Vec::new(),
            sets: Vec::new(),
            class: false,
        };
        let (entry, exits) = b.build(&node)?;
        let m = b.push(State::Match)?;
        b.patch(&exits, m);
        Ok(Regex {
            states: b.states,
            sets: b.sets,
            start: entry,
            class: b.class,
        })
    }

    /// `boost::regex_match(subject, re)`: whether the pattern matches the whole subject.
    ///
    /// # Errors
    ///
    /// [`RegexError::Unsupported`] where a class would be tested against a byte past ASCII.
    pub fn is_match(&self, subject: &[u8]) -> Result<bool, RegexError> {
        if self.class && !subject.is_ascii() {
            return Err(RegexError::Unsupported(
                "a character class tested against a byte past ASCII".into(),
            ));
        }
        let n = self.states.len();
        let mut cur: Vec<usize> = Vec::new();
        let mut seen = vec![usize::MAX; n];
        self.closure(self.start, subject, 0, &mut cur, &mut seen);
        for i in 0..subject.len() {
            let c = subject[i];
            let mut next: Vec<usize> = Vec::new();
            for &s in &cur {
                let to = match &self.states[s] {
                    State::Byte(b, t) if *b == c => Some(*t),
                    State::Any(t) => Some(*t),
                    State::Set(k, t) if self.sets[*k][usize::from(c)] => Some(*t),
                    _ => None,
                };
                if let Some(t) = to {
                    self.closure(t, subject, i + 1, &mut next, &mut seen);
                }
            }
            if next.is_empty() {
                return Ok(false);
            }
            cur = next;
        }
        Ok(cur.iter().any(|&s| matches!(self.states[s], State::Match)))
    }

    /// The states reachable from `s` at position `i` without reading a byte, the assertions
    /// tested there.
    fn closure(&self, s: usize, text: &[u8], i: usize, out: &mut Vec<usize>, seen: &mut [usize]) {
        let mut stack = vec![s];
        while let Some(s) = stack.pop() {
            if seen[s] == i {
                continue;
            }
            seen[s] = i;
            match &self.states[s] {
                State::Split(a, b) => {
                    stack.push(*b);
                    stack.push(*a);
                }
                State::Jump(t) => stack.push(*t),
                State::StartLine(t) => {
                    if start_line(text, i) {
                        stack.push(*t);
                    }
                }
                State::EndLine(t) => {
                    if end_line(text, i) {
                        stack.push(*t);
                    }
                }
                _ => out.push(s),
            }
        }
    }
}

/// `match_start_line` at `i` (`match_default`: no `match_prev_avail`, no `match_not_bol`).
fn start_line(text: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let t = text[i - 1];
    if i != text.len() {
        is_separator(t) && !(t == b'\r' && text[i] == b'\n')
    } else {
        is_separator(t)
    }
}

/// `match_end_line` at `i` (`match_default`: no `match_not_eol`).
fn end_line(text: &[u8], i: usize) -> bool {
    if i != text.len() {
        if is_separator(text[i]) {
            if i != 0 && text[i - 1] == b'\r' && text[i] == b'\n' {
                return false;
            }
            return true;
        }
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        Regex::new(p.as_bytes())
            .unwrap()
            .is_match(s.as_bytes())
            .unwrap()
    }

    fn err(p: &str) -> String {
        match Regex::new(p.as_bytes()) {
            Err(RegexError::Error(e)) => e,
            Err(RegexError::Unsupported(u)) => panic!("unsupported: {u}"),
            Ok(_) => panic!("{p} compiled"),
        }
    }

    #[test]
    fn matches_the_whole_subject() {
        assert!(m(".*MK3.*", "PRINTER_MODEL_MK3S"));
        assert!(!m(".*MK3.*", "MK4"));
        assert!(m(
            ".*nozzle_diameter[^_a-zA-Z0-9]0.4.*",
            "nozzle_diameter=0.4"
        ));
        assert!(m("", ""));
        assert!(!m("", "a"));
        assert!(m("a|", ""));
        assert!(m("(ab)+c?", "ababc"));
        assert!(m("a{2,3}", "aaa"));
        assert!(!m("a{2,3}", "aaaa"));
        assert!(m("a{,2}", "a{,2}"));
        assert!(m("x{2}?", "xx"));
        assert!(m(".", "\n"));
        assert!(m("[^a]", "\n"));
        assert!(m("a$\\n^b", "a\nb"));
        assert!(!m("a\\r$\\n", "a\r\n"));
        assert!(m("\\d+\\.\\d", "12.5"));
        assert!(m("[\\w-]+", "a-b_c"));
        assert!(m("[]a]+", "]a"));
        assert!(m("[a-]+", "-a"));
        assert!(m("(?:a|b)*", "abba"));
        assert!(m("}", "}"));
    }

    #[test]
    fn escapes_read_as_boost_reads_them() {
        // The end of a range goes through unescape_character, where \d is the letter d.
        assert!(m("[a-\\d]", "c"));
        assert!(!m("[a-\\d]", "5"));
        // Class letters with no class are literals; \h, \l, \u and \v are classes.
        assert!(m("\\i\\Y", "iY"));
        assert!(m("[\\i]", "i"));
        assert!(m("\\h\\v", "\t\n"));
        assert!(!m("\\h", "\n"));
        assert!(m("\\l\\u\\L\\U", "aBC4"));
        assert!(m("[\\V]", "a"));
        assert!(!m("[\\V]", "\x0b"));
        // Negated classes join into one mask: neither blank nor word.
        assert!(!m("[\\H\\W]", "M"));
        assert!(m("[\\H\\W]", "-"));
        assert!(m("[\\D\\w]", "5"));
        // In a set \v and \b are control characters, \c a control letter.
        assert!(m("[\\v][\\b][\\cA]", "\x0b\x08\x01"));
        assert!(m("\\e\\a\\E", "\x1b\x07E"));
        assert_eq!(
            err("[\\"),
            "Escape sequence terminated prematurely.  The error occurred while parsing the regular expression: '[\\>>>HERE>>>'."
        );
        assert_eq!(
            err("a\\c"),
            "ASCII escape sequence terminated prematurely.  The error occurred while parsing the regular expression: 'a>>>HERE>>>\\c'."
        );
    }

    #[test]
    fn fails_as_boost_does() {
        assert_eq!(
            err("*a"),
            "The repeat operator \"*\" cannot start a regular expression.  The error occurred while parsing the regular expression: '>>>HERE>>>*a'."
        );
        assert_eq!(
            err("a**"),
            "Invalid preceding regular expression prior to repetition operator.  The error occurred while parsing the regular expression: 'a**>>>HERE>>>'."
        );
        assert_eq!(
            err("(a"),
            "Unmatched marking parenthesis ( or \\(.  The error occurred while parsing the regular expression: '(a>>>HERE>>>'."
        );
        assert_eq!(
            err("a)"),
            "Found a closing ) with no corresponding opening parenthesis.  The error occurred while parsing the regular expression: 'a>>>HERE>>>)'."
        );
        assert_eq!(
            err("a{3,1}"),
            "Invalid content of repeat range.  The error occurred while parsing the regular expression: 'a{3,>>>HERE>>>1}'."
        );
        assert_eq!(
            err("[z-a]"),
            "Invalid character class name, collating name, or character range.  The error occurred while parsing the regular expression: '[z-a]>>>HERE>>>'."
        );
        assert_eq!(
            err("abcdefghijklmnop("),
            "Unmatched marking parenthesis ( or \\(.  The error occurred while parsing the regular expression fragment: 'hijklmnop(>>>HERE>>>'."
        );
        assert_eq!(
            err("[a-z"),
            "Found a closing ) with no corresponding opening parenthesis.  The error occurred while parsing the regular expression: '[a-z>>>HERE>>>'."
        );
        assert_eq!(
            err("[abc"),
            "Unmatched [ or [^ in character class declaration.  The error occurred while parsing the regular expression: '[abc>>>HERE>>>'."
        );
    }

    #[test]
    fn declines_what_it_does_not_take() {
        for p in ["\\b", "(?=a)", "a++", "[[:alpha:]]", "\\1", "a{2000}"] {
            assert!(
                matches!(Regex::new(p.as_bytes()), Err(RegexError::Unsupported(_))),
                "{p}"
            );
        }
        let r = Regex::new(b"\\w").unwrap();
        assert!(matches!(
            r.is_match("\u{e9}".as_bytes()),
            Err(RegexError::Unsupported(_))
        ));
    }
}
