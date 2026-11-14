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
// surfaces.sop1's SFLP records (parity/sop1/surfaces.md, added 2026-11-13 by
// orca-o3-probe-surfaces), which orca-surface replays for the piece surfaces-2:
// check_layer_id_pattern (utils.cpp), which discover_horizontal_shells asks whether the
// extra_solid_infills pattern puts a solid layer at a layer index. Its std::stoi calls (strtol
// under them, and the exceptions they throw and it catches) decide most answers, so the patterns
// are the forms it reads and the texts std::stoi reads differently: signs, white space strtol
// skips but the pattern's own filter keeps, digits followed by other text, numbers past int,
// empty tokens, quotes.
#include "shared.hpp"

#include <climits>

#include "libslic3r/Utils.hpp"

namespace probe {

void record_pattern(Sets &sets);

namespace {

using namespace probe::surfaces;

const char *const hand_patterns[] = {
    "",
    "5",
    "5#1",
    "5#2",
    "5#",
    "#3",
    "3#3",
    "4#7",
    "1",
    "0",
    "-3",
    "+5",
    "5#0",
    "5#-1",
    "0#2",
    "1,7,9",
    "1, 7, 9",
    " 1 ,\t7\r\n,9 ",
    "5,9#2,18",
    "5,9#,18",
    "5,#2,18",
    "9#3,2",
    "0#2,3",
    ",",
    ",,",
    "3,",
    ",3",
    "a",
    "a,3",
    "3,a,7",
    "5x",
    "5x,7",
    "5#2x",
    "x5",
    "0x10",
    "1e1",
    "007",
    "\"5\"",
    "'5#2'",
    "\"1,7,9\"",
    "\"",
    "''",
    "\"5",
    "5'",
    "\v5",
    "\f5,6",
    "5\v",
    "2147483647",
    "2147483648",
    "-2147483648",
    "99999999999",
    "3,99999999999,4",
    "2147483647#2",
    "2#2147483647",
    "1#2147483647",
    "\xc2\xa0" "5",
    "5" "\xc2\xa0",
    "#",
    "##",
    "5##2",
    "5#2#3",
    "2,4#2#1",
    "  ",
    "\t\r\n",
    "--5",
    "+-5",
    "5-",
};

// The layer indices each pattern is asked about: -2 to 60, then larger ones and the ends of int
// (INT_MAX itself is left out: the function adds 1 to the index, which would overflow).
const int extra_ids[] = {INT_MAX - 1, INT_MIN, INT_MIN + 1, 99, 100, 1000, 65535};

void put_pattern(Sets &sets, Picker &pick, const std::string &pattern)
{
    Payload p;
    p.str(pattern);
    const int first = -2, count = 63;
    p.i32(first).u32(uint32_t(count + std::size(extra_ids)));
    for (int i = 0; i < count; ++i)
        p.i32(first + i).boolean(Slic3r::check_layer_id_pattern(pattern, first + i));
    for (int id : extra_ids)
        p.i32(id).boolean(Slic3r::check_layer_id_pattern(pattern, id));
    add(sets, "SFLP", p, pick);
}

} // namespace

// SFLP: str the pattern, i32 the first index (-2), u32 the number of indices, then each index's
// i32 and u8 the answer.
void record_pattern(Sets &sets)
{
    Picker pick{100, 1024};
    for (const char *s : hand_patterns)
        put_pattern(sets, pick, s);
    // Seeded patterns from the pieces the parser reads.
    SplitMix          g(0x5F3E0007u);
    const char *const pieces[] = {"1", "2", "3", "5", "7", "10", "12", "0", "-", "+", "#", ",", " ", "\t", "\"", "'", "x", "9", "4", "11"};
    for (int i = 0; i < 300; ++i) {
        std::string s;
        const int   n = 1 + int(g.below(8));
        for (int j = 0; j < n; ++j)
            s += pieces[g.below(std::size(pieces))];
        put_pattern(sets, pick, s);
    }
}

} // namespace probe
