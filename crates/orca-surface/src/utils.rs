// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/utils.cpp lines 1749-1830 of OrcaSlicer v2.4.2
// (check_layer_id_pattern), translated on 2026-11-14 by orca-o3-surfaces-2. Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab) and the PrusaSlicer authors (Prusa
// Research), under the GNU Affero General Public License, version 3 (AGPL-3.0; LICENSE).

//! `check_layer_id_pattern` of `libslic3r/utils.cpp`: whether the `extra_solid_infills` pattern
//! puts a solid layer at a 0-based layer index. `discover_horizontal_shells` (`crate::shells`)
//! asks it for every layer.
//!
//! The pattern is `N` (every `N`th layer, 1-based), `N#K` (`K` layers from every `N`th) or a comma
//! list of layers and `N#K` runs (`"1,7,9"`, `"5,9#2,18"`). The function first drops the spaces,
//! tabs, carriage returns and line feeds and one quote at each end; `\v` and `\f` stay, and
//! `std::stoi` (`orca_fmt::parse::stoi`, MSVC's `strtol` with its 32-bit `long`) skips them as
//! leading white space. A token `std::stoi` cannot read, or whose number is past `int`, throws: a
//! list ignores that token, the interval form answers false.
//!
//! The pattern is bytes, as the C++ `std::string` is: a config string may hold any byte.
//!
//! Two sums can pass `int`. The index plus 1 cannot, as the caller passes a layer index, but it
//! wraps as MSVC's `int` does; and `base_layer + local_count` of a list run wraps the same way, so
//! a run that ends past `int` matches no layer, as in the C++. Nothing here reads a setting
//! domain A leaves out, so nothing stops with `NotPorted`.
//!
//! This function belongs in a `libslic3r/utils` crate that no piece carries yet; it is
//! translated here because only `discover_horizontal_shells` calls it.

use orca_fmt::parse::{StoError, stoi};

/// `check_layer_id_pattern(pattern, layer_id)`: whether `pattern` puts a solid layer at the
/// 0-based `layer_id`.
#[must_use]
pub fn check_layer_id_pattern(pattern: &[u8], layer_id: i32) -> bool {
    if pattern.is_empty() || layer_id < 0 {
        return false;
    }

    // layer_id is 0-based, so we need to add 1 to make it 1-based
    let layer_id = layer_id.wrapping_add(1);

    // Remove whitespace and surrounding quotes.
    let mut p: Vec<u8> = Vec::with_capacity(pattern.len());
    for &c in pattern {
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            continue;
        }
        p.push(c);
    }
    if p.first().is_some_and(|&c| c == b'"' || c == b'\'') {
        p.remove(0);
    }
    if p.last().is_some_and(|&c| c == b'"' || c == b'\'') {
        p.pop();
    }
    if p.is_empty() {
        return false;
    }

    // Explicit list form: "1,7,9" or with counts per entry: "5,9#2,18"
    if p.contains(&b',') {
        let mut start = 0usize;
        while start < p.len() {
            let end = find(&p, b',', start);
            let token = match end {
                None => &p[start..],
                Some(end) => &p[start..end],
            };
            // A token std::stoi throws on is ignored.
            if !token.is_empty() && list_token_matches(token, layer_id) == Ok(true) {
                return true;
            }
            match end {
                None => break,
                Some(end) => start = end + 1,
            }
        }
        return false;
    }

    // Interval form: "N#K" or simplified "N" (equals to N#1)
    let Ok((interval, count)) = interval_and_count(&p) else {
        return false;
    };

    if interval <= 0 || count <= 0 {
        return false;
    }

    // Layers are 1-based. Match layers interval, interval+1, ..., interval+count-1, then repeat
    // every interval.
    if layer_id < interval {
        return false;
    }
    let m = layer_id % interval; // For multiples, mod == 0
    (0..count).contains(&m)
}

/// `p.find(c, from)`.
fn find(p: &[u8], c: u8, from: usize) -> Option<usize> {
    p[from..].iter().position(|&b| b == c).map(|k| from + k)
}

/// The body of the list form's `try` block for one token: whether it matches `layer_id`, or the
/// exception `std::stoi` throws.
fn list_token_matches(token: &[u8], layer_id: i32) -> Result<bool, StoError> {
    match token.iter().position(|&b| b == b'#') {
        None => {
            let (value, _) = stoi(token, 10)?;
            Ok(value == layer_id)
        }
        Some(hash_pos_token) => {
            let (base_layer, _) = stoi(&token[..hash_pos_token], 10)?;
            let count_str = &token[hash_pos_token + 1..];
            let mut local_count = 1;
            if !count_str.is_empty() {
                local_count = stoi(count_str, 10)?.0;
            }
            Ok(base_layer > 0
                && local_count > 0
                && layer_id >= base_layer
                && layer_id < base_layer.wrapping_add(local_count))
        }
    }
}

/// The interval form's `try` block: `interval` and `count`, or the exception `std::stoi` throws.
fn interval_and_count(p: &[u8]) -> Result<(i32, i32), StoError> {
    let mut count = 1;
    let interval;
    match p.iter().position(|&b| b == b'#') {
        None => {
            interval = stoi(p, 10)?.0;
        }
        Some(hash_pos) => {
            interval = stoi(&p[..hash_pos], 10)?.0;
            let count_str = &p[hash_pos + 1..];
            if !count_str.is_empty() {
                count = stoi(count_str, 10)?.0;
            }
        }
    }
    Ok((interval, count))
}

#[cfg(test)]
mod tests {
    use super::check_layer_id_pattern;

    #[test]
    fn interval_and_list_forms() {
        // "5": the 5th, 10th, ... layers, 1-based.
        assert!(!check_layer_id_pattern(b"5", 3));
        assert!(check_layer_id_pattern(b"5", 4));
        assert!(check_layer_id_pattern(b"5", 9));
        // "5#2": two layers from every 5th.
        assert!(check_layer_id_pattern(b"5#2", 5));
        assert!(!check_layer_id_pattern(b"5#2", 6));
        // A list, white space and quotes dropped, a bad token ignored.
        assert!(check_layer_id_pattern(b"\" 1 ,a, 7\"", 6));
        assert!(!check_layer_id_pattern(b"1,7,9", 1));
        assert!(check_layer_id_pattern(b"5,9#2,18", 9));
        assert!(!check_layer_id_pattern(b"5,9#2,18", 10));
        // A negative index, an empty pattern, a number past int.
        assert!(!check_layer_id_pattern(b"5", -1));
        assert!(!check_layer_id_pattern(b"", 4));
        assert!(!check_layer_id_pattern(b"99999999999", 4));
    }
}
