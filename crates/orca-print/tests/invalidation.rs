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
// Carries no OrcaSlicer code: it reads OrcaSlicer's src/libslic3r/Print.cpp and PrintObject.cpp
// (v2.4.2) from the work folder at run time. Written on 2026-11-14 (orca-o3-print-1).

//! The check of the invalidation tables (`orca_print::invalidation`) against the C++ they were
//! translated from: this test reads Print.cpp and PrintObject.cpp again, with a reader of its own
//! (comments, and `#if 0` blocks, dropped), and fails while they differ.
//!
//! - `Print::invalidate_state_by_config_options`: `steps_gcode` key for key in order, the empty
//!   `steps_ignore`, and each branch of the chain after them: the keys its condition compares
//!   `opt_key` with, in order, and the print and object steps its body adds, in order; the last
//!   branch invalidates all steps.
//! - `Print::invalidate_step`: the base's step, then, for any other step, the G-code export.
//! - `PrintObject::invalidate_state_by_config_options`: each branch's keys, and the steps its body
//!   names in order, against each table branch's action.
//! - `PrintObject::invalidate_step`: each propagation branch's step, the object steps and print
//!   steps it invalidates and whether it resets the slicing parameters, then the wipe tower and
//!   the G-code export.
//! - `PrintObject::invalidate_all_steps`: both bases' `invalidate_all_steps` and the reset.
//!
//! The work folder is `SKETCH_ORCA_HOME`, else `%LOCALAPPDATA%\SketchDev\orca-engine` (Windows)
//! or `~/.local/share/SketchDev/orca-engine`; the test fails naming the source's path when the
//! file is missing.

use std::path::PathBuf;

use orca_layer::print_object::{PrintObjectStep, PrintStep};
use orca_print::invalidation::{
    OBJECT_BRANCHES, OBJECT_STEP_PROPAGATION, ObjectAction, PRINT_BRANCHES, PRINT_STEPS_GCODE,
    PRINT_STEPS_IGNORE,
};

fn work_home() -> PathBuf {
    if let Some(h) = std::env::var_os("SKETCH_ORCA_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|u| PathBuf::from(u).join("AppData").join("Local"))
            })
            .expect("LOCALAPPDATA or USERPROFILE is set");
        return local.join("SketchDev").join("orca-engine");
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .expect("XDG_DATA_HOME or HOME is set");
    data.join("SketchDev").join("orca-engine")
}

fn source(name: &str) -> String {
    let path = work_home()
        .join("src")
        .join("src")
        .join("libslic3r")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (the work folder's OrcaSlicer source, which the invalidation tables are \
             checked against)",
            path.display()
        )
    })
}

/// The body (inside its outer braces) of the function whose definition starts with `signature`,
/// with comments dropped, preprocessor lines blanked and `#if 0` / `#if (0)` blocks dropped;
/// string literals kept.
fn function_body(text: &str, signature: &str) -> Vec<char> {
    let text = text.replace("\r\n", "\n");
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("no definition starting {signature:?}"));
    let src: Vec<char> = text[start..].chars().collect();
    let mut out: Vec<char> = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    let mut depth = 0usize;
    let mut body_start = None;
    while i < src.len() {
        let c = src[i];
        if line_start {
            let mut j = i;
            while j < src.len() && (src[j] == ' ' || src[j] == '\t') {
                j += 1;
            }
            if j < src.len() && src[j] == '#' {
                let line_end = |k: usize| {
                    (k..src.len())
                        .find(|&e| src[e] == '\n')
                        .unwrap_or(src.len())
                };
                let end = line_end(j);
                let line: String = src[j..end].iter().filter(|c| !c.is_whitespace()).collect();
                i = (end + 1).min(src.len());
                if line == "#if0" || line == "#if(0)" {
                    // Skip to the matching #endif.
                    let mut nest = 1;
                    while i < src.len() && nest > 0 {
                        let end = line_end(i);
                        let l: String = src[i..end].iter().filter(|c| !c.is_whitespace()).collect();
                        if l.starts_with("#if") {
                            nest += 1;
                        } else if l.starts_with("#endif") {
                            nest -= 1;
                        }
                        i = (end + 1).min(src.len());
                    }
                }
                out.push('\n');
                continue;
            }
        }
        line_start = false;
        if c == '/' && src.get(i + 1) == Some(&'/') {
            while i < src.len() && src[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && src.get(i + 1) == Some(&'*') {
            i += 2;
            while i < src.len() && !(src[i] == '*' && src.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '"' || c == '\'' {
            out.push(c);
            i += 1;
            while i < src.len() && src[i] != c {
                if src[i] == '\\' {
                    out.push(src[i]);
                    i += 1;
                }
                out.push(src[i]);
                i += 1;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '\n' {
            line_start = true;
        }
        if c == '{' {
            depth += 1;
            if body_start.is_none() {
                body_start = Some(out.len() + 1);
            }
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                let from = body_start.expect("the body opened");
                return out[from..].to_vec();
            }
        }
        out.push(c);
        i += 1;
    }
    panic!("the body of {signature:?} does not close");
}

/// The index of the bracket that closes the one at `open`, skipping string literals.
fn matching(text: &[char], open: usize) -> usize {
    let (o, c) = match text[open] {
        '(' => ('(', ')'),
        '{' => ('{', '}'),
        other => panic!("no bracket at {open}: {other:?}"),
    };
    let mut depth = 0usize;
    let mut i = open;
    while i < text.len() {
        let ch = text[i];
        if ch == '"' || ch == '\'' {
            i += 1;
            while text[i] != ch {
                if text[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
        } else if ch == o {
            depth += 1;
        } else if ch == c {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    panic!("the bracket at {open} does not close");
}

fn skip_ws(text: &[char], mut i: usize) -> usize {
    while i < text.len() && text[i].is_whitespace() {
        i += 1;
    }
    i
}

fn starts_with(text: &[char], i: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    text.len() >= i + w.len() && text[i..i + w.len()] == w[..]
}

fn find(text: &[char], from: usize, word: &str) -> Option<usize> {
    (from..text.len()).find(|&i| starts_with(text, i, word))
}

/// One branch of an `if ... else if ... else` chain: its condition (none for the last `else`)
/// and its body.
struct Branch {
    cond: Option<Vec<char>>,
    body: Vec<char>,
}

/// The chain whose `if` starts at `start`, and the index after it.
fn parse_chain(text: &[char], start: usize) -> (Vec<Branch>, usize) {
    let mut branches = Vec::new();
    let mut i = start;
    loop {
        assert!(starts_with(text, i, "if"), "an if at {i}");
        let open = skip_ws(text, i + 2);
        assert_eq!(text[open], '(');
        let close = matching(text, open);
        let cond = text[open + 1..close].to_vec();
        let b = skip_ws(text, close + 1);
        assert_eq!(text[b], '{', "a braced body");
        let e = matching(text, b);
        branches.push(Branch {
            cond: Some(cond),
            body: text[b + 1..e].to_vec(),
        });
        let next = skip_ws(text, e + 1);
        if !starts_with(text, next, "else") {
            return (branches, next);
        }
        let after = skip_ws(text, next + 4);
        if starts_with(text, after, "if") {
            i = after;
            continue;
        }
        assert_eq!(text[after], '{', "a braced else");
        let e = matching(text, after);
        branches.push(Branch {
            cond: None,
            body: text[after + 1..e].to_vec(),
        });
        return (branches, skip_ws(text, e + 1));
    }
}

/// The string literals of a text, in order, each with the text before it.
fn literals(text: &[char]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] == '"' {
            let before: String = text[..i].iter().collect();
            let mut j = i + 1;
            let mut s = String::new();
            while text[j] != '"' {
                if text[j] == '\\' {
                    j += 1;
                }
                s.push(text[j]);
                j += 1;
            }
            out.push((before, s));
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// The keys a condition compares `opt_key` with, in order; every literal of it must be one.
fn cond_keys(cond: &[char]) -> Vec<String> {
    let mut keys = Vec::new();
    for (before, key) in literals(cond) {
        let squeezed: String = before.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            squeezed.ends_with("opt_key=="),
            "a literal {key:?} not compared with opt_key: {squeezed}"
        );
        keys.push(key);
    }
    keys
}

/// The identifiers of a text outside its string literals, in order.
fn identifiers(text: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == '"' || c == '\'' {
            i += 1;
            while text[i] != c {
                if text[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < text.len() && (text[i].is_ascii_alphanumeric() || text[i] == '_') {
                s.push(text[i]);
                i += 1;
            }
            out.push(s);
        } else {
            i += 1;
        }
    }
    out
}

fn is_step_name(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase())
}

/// The print steps (`ps...`) a text names, in order.
fn print_steps_named(text: &[char]) -> Vec<String> {
    identifiers(text)
        .into_iter()
        .filter(|id| is_step_name(id, "ps"))
        .collect()
}

/// The object steps (`pos...`) a text names, in order.
fn object_steps_named(text: &[char]) -> Vec<String> {
    identifiers(text)
        .into_iter()
        .filter(|id| is_step_name(id, "pos"))
        .collect()
}

fn ps(steps: &[PrintStep]) -> Vec<String> {
    steps.iter().map(|s| format!("ps{s:?}")).collect()
}

fn pos(steps: &[PrintObjectStep]) -> Vec<String> {
    steps.iter().map(|s| format!("pos{s:?}")).collect()
}

fn squeeze(text: &[char]) -> String {
    text.iter().filter(|c| !c.is_whitespace()).collect()
}

/// The body of the loop over the option keys.
fn key_loop(body: &[char]) -> Vec<char> {
    let at = find(body, 0, "opt_key : opt_keys)").expect("the loop over opt_keys");
    let open = find(body, at, "{").expect("the loop's body");
    let close = matching(body, open);
    body[open + 1..close].to_vec()
}

#[test]
fn print_options_match_the_cpp() {
    let text = source("Print.cpp");
    let body = function_body(&text, "bool Print::invalidate_state_by_config_options(");

    // steps_gcode, key for key.
    let at = find(&body, 0, "steps_gcode = {").expect("steps_gcode's list");
    let open = find(&body, at, "{").unwrap();
    let close = matching(&body, open);
    let gcode: Vec<String> = literals(&body[open + 1..close])
        .into_iter()
        .map(|(_, s)| s)
        .collect();
    let table: Vec<String> = PRINT_STEPS_GCODE.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(gcode, table, "steps_gcode");

    // steps_ignore is declared empty.
    let at = find(&body, 0, "steps_ignore").expect("steps_ignore");
    let after = skip_ws(&body, at + "steps_ignore".len());
    assert_eq!(body[after], ';', "steps_ignore has no list");
    assert!(PRINT_STEPS_IGNORE.is_empty());

    let lp = key_loop(&body);
    let start = skip_ws(&lp, 0);
    let (chain, end) = parse_chain(&lp, start);
    assert_eq!(skip_ws(&lp, end), lp.len(), "the loop holds only the chain");
    assert_eq!(chain.len(), 2 + PRINT_BRANCHES.len() + 1, "branch count");

    // The two sets.
    let c0 = squeeze(chain[0].cond.as_ref().unwrap());
    assert!(c0.contains("steps_gcode.find(opt_key)"), "{c0}");
    assert_eq!(
        print_steps_named(&chain[0].body),
        ps(&[PrintStep::GCodeExport])
    );
    assert!(object_steps_named(&chain[0].body).is_empty());
    let c1 = squeeze(chain[1].cond.as_ref().unwrap());
    assert!(c1.contains("steps_ignore.find(opt_key)"), "{c1}");
    assert!(print_steps_named(&chain[1].body).is_empty());
    assert!(object_steps_named(&chain[1].body).is_empty());

    for (n, (branch, table)) in chain[2..].iter().zip(PRINT_BRANCHES).enumerate() {
        let keys = cond_keys(branch.cond.as_ref().expect("a condition"));
        let table_keys: Vec<String> = table.keys.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(keys, table_keys, "the keys of branch {n}");
        assert_eq!(
            print_steps_named(&branch.body),
            ps(table.steps),
            "the steps of branch {n}"
        );
        assert_eq!(
            object_steps_named(&branch.body),
            pos(table.osteps),
            "the object steps of branch {n}"
        );
    }

    // The last else invalidates all the print's steps.
    let last = chain.last().unwrap();
    assert!(last.cond.is_none());
    assert!(squeeze(&last.body).contains("invalidated|=this->invalidate_all_steps();"));

    // After the loop: the print steps, then each object's steps.
    let tail = squeeze(&body[find(&body, 0, "sort_remove_duplicates(steps)").unwrap()..]);
    let s = tail
        .find("invalidated|=this->invalidate_step(step);")
        .unwrap();
    let o = tail
        .find("invalidated|=object->invalidate_step(ostep);")
        .unwrap();
    assert!(s < o);
}

#[test]
fn print_invalidate_step_matches_the_cpp() {
    let text = source("Print.cpp");
    let body = squeeze(&function_body(
        &text,
        "bool Print::invalidate_step(PrintStep step)",
    ));
    let a = body.find("Inherited::invalidate_step(step)").unwrap();
    let b = body.find("if(step!=psGCodeExport)").unwrap();
    let c = body
        .find("invalidated|=Inherited::invalidate_step(psGCodeExport)")
        .unwrap();
    assert!(a < b && b < c, "{body}");
}

/// The steps a table action names in the C++ body it stands for, in order.
fn action_steps(action: ObjectAction) -> Vec<String> {
    use PrintObjectStep as S;
    match action {
        ObjectAction::Steps(s) => pos(s),
        ObjectAction::Brim => pos(&[S::SupportMaterial, S::Perimeters]),
        ObjectAction::GapFill => pos(&[S::Slice, S::Perimeters]),
        ObjectAction::EnableSupport => pos(&[S::SupportMaterial, S::Slice]),
        ObjectAction::SparseInfillDensity => pos(&[S::Perimeters, S::PrepareInfill]),
        ObjectAction::BridgeFlow => pos(&[S::Perimeters, S::Infill, S::SupportMaterial]),
        ObjectAction::GCodeExport => ps(&[PrintStep::GCodeExport]),
        ObjectAction::FlushInto => ps(&[PrintStep::WipeTower, PrintStep::GCodeExport]),
    }
}

#[test]
fn object_options_match_the_cpp() {
    let text = source("PrintObject.cpp");
    let body = function_body(
        &text,
        "bool PrintObject::invalidate_state_by_config_options(",
    );
    let lp = key_loop(&body);
    let start = skip_ws(&lp, 0);
    let (chain, end) = parse_chain(&lp, start);
    assert_eq!(skip_ws(&lp, end), lp.len(), "the loop holds only the chain");
    assert_eq!(chain.len(), OBJECT_BRANCHES.len() + 1, "branch count");

    for (n, (branch, table)) in chain.iter().zip(OBJECT_BRANCHES).enumerate() {
        let keys = cond_keys(branch.cond.as_ref().expect("a condition"));
        let table_keys: Vec<String> = table.keys.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(keys, table_keys, "the keys of branch {n}");
        let mut named = object_steps_named(&branch.body);
        named.extend(print_steps_named(&branch.body));
        assert_eq!(named, action_steps(table.action), "the steps of branch {n}");
        // The bodies the actions translate: what each reads or calls.
        let squeezed = squeeze(&branch.body);
        match table.action {
            ObjectAction::Steps(_) => {
                assert!(
                    !squeezed.contains("if("),
                    "branch {n} adds its steps unconditionally"
                );
            }
            ObjectAction::Brim => {
                assert!(squeezed.contains("if(opt_key==\"brim_type\")"));
                assert!(squeezed.contains("ConfigOptionEnum<BrimType>"));
                assert!(squeezed.contains("btOuterOnly"));
            }
            ObjectAction::GapFill => {
                assert!(squeezed.contains(
                    "if(this->is_mm_painted()&&(opt_key==\"filter_out_gap_fill\"&&(opt_key==\"gap_infill_speed\"&&"
                ));
            }
            ObjectAction::EnableSupport => {
                assert!(squeezed.contains("if(m_config.support_top_z_distance==0.)"));
            }
            ObjectAction::SparseInfillDensity => {
                assert!(squeezed.contains("ConfigOptionPercent"));
                assert_eq!(squeezed.matches("is_approx(").count(), 4);
            }
            ObjectAction::BridgeFlow => {
                assert!(squeezed.contains("if(m_config.support_top_z_distance>0.)"));
            }
            ObjectAction::GCodeExport | ObjectAction::FlushInto => {
                assert!(squeezed.starts_with("invalidated|=m_print->invalidate_step("));
            }
        }
    }

    let last = chain.last().unwrap();
    assert!(last.cond.is_none());
    let squeezed = squeeze(&last.body);
    assert!(squeezed.contains("this->invalidate_all_steps();"));
    assert!(squeezed.contains("invalidated=true;"));
}

#[test]
fn object_step_propagation_matches_the_cpp() {
    let text = source("PrintObject.cpp");
    let body = function_body(
        &text,
        "bool PrintObject::invalidate_step(PrintObjectStep step)",
    );
    let start = find(&body, 0, "if (step").expect("the propagation chain");
    let head = squeeze(&body[..start]);
    assert_eq!(head, "boolinvalidated=Inherited::invalidate_step(step);");
    let (chain, end) = parse_chain(&body, start);
    assert_eq!(chain.len(), OBJECT_STEP_PROPAGATION.len(), "branch count");
    for (n, (branch, table)) in chain.iter().zip(OBJECT_STEP_PROPAGATION).enumerate() {
        let cond = squeeze(branch.cond.as_ref().unwrap());
        assert_eq!(cond, format!("step==pos{:?}", table.step), "branch {n}");
        let squeezed = squeeze(&branch.body);
        assert_eq!(
            object_steps_named(&branch.body),
            pos(table.object_steps),
            "branch {n}"
        );
        assert_eq!(
            print_steps_named(&branch.body),
            ps(table.print_steps),
            "branch {n}"
        );
        assert!(
            squeezed.starts_with("invalidated|=this->invalidate_steps({"),
            "branch {n}"
        );
        assert_eq!(
            squeezed.contains("invalidated|=m_print->invalidate_steps({"),
            !table.print_steps.is_empty(),
            "branch {n}"
        );
        assert_eq!(
            squeezed.contains("m_slicing_params.valid=false;"),
            table.resets_slicing_params,
            "branch {n}"
        );
    }
    let tail = squeeze(&body[end..]);
    assert_eq!(
        tail,
        "invalidated|=m_print->invalidate_step(psWipeTower);\
         invalidated|=m_print->invalidate_step(psGCodeExport);returninvalidated;"
    );
}

#[test]
fn object_invalidate_all_steps_matches_the_cpp() {
    let text = source("PrintObject.cpp");
    let body = squeeze(&function_body(
        &text,
        "bool PrintObject::invalidate_all_steps()",
    ));
    let a = body
        .find("boolresult=Inherited::invalidate_all_steps()|m_print->invalidate_all_steps();")
        .expect(&body);
    let b = body.find("m_slicing_params.valid=false;").expect(&body);
    assert!(a < b);
}
