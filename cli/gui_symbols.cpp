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
// Carries OrcaSlicer code: RGB2HSV is OrcaSlicer's function from
// src/slic3r/Utils/ColorSpaceConvert.cpp (v2.4.2, commit 8500fcd), copied as is, so its copyright
// is the OrcaSlicer contributors'; the file also compiles the implementation of NanoSVG (Mikko
// Mononen, zlib licence), which OrcaSlicer keeps in deps_src/nanosvg. Added 2026-10-30.
//
// sketch-orca: the few functions Orca's slicing library calls that Orca compiles only into its GUI,
// so a build with SLIC3R_GUI=OFF leaves them out:
// - the NanoSVG parser (Orca puts NANOSVG_IMPLEMENTATION in slic3r/GUI/BitmapCache.cpp), used by
//   libslic3r for SVG shapes and embossing;
// - RGB2HSV (slic3r/Utils/ColorSpaceConvert.cpp, which also needs wxWidgets), used by
//   FlushVolCalc.cpp for the flush volumes between filament colours. It is Orca's function as is.
#include <algorithm>
#include <cmath>

#define NANOSVG_IMPLEMENTATION
#include "nanosvg/nanosvg.h"

// The input r, g, b values should be in range [0, 1]. The output h is in range [0, 360], s is in
// range [0, 1] and v is in range [0, 1].
void RGB2HSV(float r, float g, float b, float *h, float *s, float *v)
{
    float Cmax  = std::max(std::max(r, g), b);
    float Cmin  = std::min(std::min(r, g), b);
    float delta = Cmax - Cmin;

    if (std::abs(delta) < 0.001) {
        *h = 0.f;
    } else if (Cmax == r) {
        *h = 60.f * std::fmod((g - b) / delta, 6.f);
    } else if (Cmax == g) {
        *h = 60.f * ((b - r) / delta + 2);
    } else {
        *h = 60.f * ((r - g) / delta + 4);
    }

    if (std::abs(Cmax) < 0.001) {
        *s = 0.f;
    } else {
        *s = delta / Cmax;
    }

    *v = Cmax;
}
