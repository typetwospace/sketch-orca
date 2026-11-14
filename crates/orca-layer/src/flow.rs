// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Flow.hpp (1-141 and 146-149: FlowRole, the flow errors and
// class Flow) and Flow.cpp (1-231 and 273: the errors' texts, auto_extrusion_width,
// opt_key_to_flow_role, extrusion_width, new_from_config_width, with_spacing, with_cross_section,
// the spacing helpers and mm3_per_mm) of OrcaSlicer v2.4.2, translated on 2026-11-13
// (orca-o3-layer-1), and Flow.hpp 142-145 and Flow.cpp 232-272 (the support flows) translated on
// 2026-11-14 (orca-o3-layer-3). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! [`Flow`]: an extrusion's width, height, spacing and nozzle, and the cross section that follows
//! from them, with the rounded-rectangle model of a line (a rectangle with half circles at its
//! sides) or the round thread of a bridge.
//!
//! The C++ throws where this module returns a [`FlowError`], whose [`FlowError::what`] is the
//! exception's text byte for byte: a spacing or a flow that comes out zero or negative
//! (`FlowErrorNegativeSpacing`, `FlowErrorNegativeFlow`), a line width that cannot be resolved
//! (`FlowErrorMissingVariable`), a flow height that is not positive, a spacing narrower than the
//! layer (`InvalidArgument`) and a key without a flow role (`RuntimeError`). `L()` marks a text for
//! translation and returns it unchanged in the engine. So `Flow::new`, which computes the spacing,
//! and everything that builds a new flow return a `Result`.
//!
//! Floats are `f32` where the C++'s are `float`, and the mixed expressions promote where the C++
//! does: `mm3_per_mm` computes in `double` and rounds to `float` before it tests the sign,
//! `rounded_rectangle_extrusion_spacing` subtracts in `float`, `bridge_extrusion_spacing` adds the
//! `double` 0.05 and rounds back.
//!
//! The support flows ([`support_material_flow`], [`support_transition_flow`],
//! [`support_material_1st_layer_flow`] and [`support_material_interface_flow`]; Flow.hpp 142-145
//! and Flow.cpp 232-272, orca-o3-layer-3) read the object's config and its print's: the print
//! (`object->print()`) is an argument. A filament id of 0 (the current filament) gives
//! `get_at(-1)`, a huge `size_t`, so the first nozzle. A `layer_height` of 0, the C++'s default
//! argument, takes the object's layer height (the print's first layer height for the first-layer
//! flow). The asserts of the C++ (a bridge's `with_width`, `with_cross_section` of a bridge or of
//! a width below its height) do not run in its release build, and do not here: the code below
//! them runs as written.

use core::fmt;

use orca_config::{ConfigBase, OptionType, OptionValue, PrintConfig};
use orca_geom::point::{Coord, PI, scale_};
use orca_index::stop::not_ported;

use crate::print::Print;
use crate::print_object::PrintObject;

/// `BRIDGE_EXTRA_SPACING`: the extra spacing of bridge threads, in mm.
pub const BRIDGE_EXTRA_SPACING: f64 = 0.05;

/// `EPSILON` of `libslic3r.h`.
const EPSILON: f64 = 1e-4;

/// `M_PI`.
const M_PI: f64 = core::f64::consts::PI;

/// `FlowRole`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FlowRole {
    /// `frExternalPerimeter`.
    #[default]
    ExternalPerimeter = 0,
    /// `frPerimeter`.
    Perimeter = 1,
    /// `frInfill`.
    Infill = 2,
    /// `frSolidInfill`.
    SolidInfill = 3,
    /// `frTopSolidInfill`.
    TopSolidInfill = 4,
    /// `frSupportMaterial`.
    SupportMaterial = 5,
    /// `frSupportMaterialInterface`.
    SupportMaterialInterface = 6,
    /// `frSupportTransition`.
    SupportTransition = 7,
}

impl FlowRole {
    /// Every role, by value.
    pub const ALL: [FlowRole; 8] = [
        Self::ExternalPerimeter,
        Self::Perimeter,
        Self::Infill,
        Self::SolidInfill,
        Self::TopSolidInfill,
        Self::SupportMaterial,
        Self::SupportMaterialInterface,
        Self::SupportTransition,
    ];

    /// The role of a value, `None` past `frSupportTransition`.
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }
}

/// What a flow computation throws in the C++.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowError {
    /// `FlowErrorNegativeSpacing`.
    NegativeSpacing,
    /// `FlowErrorNegativeFlow`.
    NegativeFlow,
    /// `FlowErrorMissingVariable`, with its text.
    MissingVariable(String),
    /// `Slic3r::InvalidArgument`, with its text.
    InvalidArgument(String),
    /// `Slic3r::RuntimeError`, with its text.
    Runtime(String),
}

impl FlowError {
    /// The exception's `what()`.
    #[must_use]
    pub fn what(&self) -> String {
        match self {
            Self::NegativeSpacing => {
                "Flow::spacing() produced negative spacing. Did you set some extrusion width too \
                 small?"
                    .to_string()
            }
            Self::NegativeFlow => {
                "Flow::mm3_per_mm() produced negative flow. Did you set some extrusion width too \
                 small?"
                    .to_string()
            }
            Self::MissingVariable(s) | Self::InvalidArgument(s) | Self::Runtime(s) => s.clone(),
        }
    }
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what())
    }
}

impl std::error::Error for FlowError {}

/// `Flow`. `==` is the C++'s: width, height, nozzle and bridge, not the spacing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flow {
    width: f32,
    height: f32,
    spacing: f32,
    nozzle_diameter: f32,
    bridge: bool,
}

impl PartialEq for Flow {
    fn eq(&self, rhs: &Self) -> bool {
        self.width == rhs.width
            && self.height == rhs.height
            && self.nozzle_diameter == rhs.nozzle_diameter
            && self.bridge == rhs.bridge
    }
}

impl Flow {
    /// `Flow(width, height, nozzle_diameter)`: a line, its spacing from the rounded-rectangle
    /// model.
    ///
    /// # Errors
    /// [`FlowError::NegativeSpacing`] when the spacing is not positive.
    pub fn new(width: f32, height: f32, nozzle_diameter: f32) -> Result<Flow, FlowError> {
        Ok(Self::from_parts(
            width,
            height,
            Self::rounded_rectangle_extrusion_spacing(width, height)?,
            nozzle_diameter,
            false,
        ))
    }

    /// The private `Flow(width, height, spacing, nozzle_diameter, bridge)`.
    fn from_parts(
        width: f32,
        height: f32,
        spacing: f32,
        nozzle_diameter: f32,
        bridge: bool,
    ) -> Self {
        Self {
            width,
            height,
            spacing,
            nozzle_diameter,
            bridge,
        }
    }

    /// `width()`: a line's full width, or a bridge's thread diameter.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.width
    }

    /// `scaled_width()`: `coord_t(scale_(width))`, truncated.
    #[must_use]
    pub fn scaled_width(&self) -> Coord {
        orca_crt::f64_to_i64(scale_(f64::from(self.width)))
    }

    /// `height()`: the layer height, or a bridge's thread diameter.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.height
    }

    /// `spacing()`: between the centre lines.
    #[must_use]
    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    /// `set_spacing(spacing)`.
    pub fn set_spacing(&mut self, spacing: f32) {
        self.spacing = spacing;
    }

    /// `scaled_spacing()`: `coord_t(scale_(spacing))`, truncated.
    #[must_use]
    pub fn scaled_spacing(&self) -> Coord {
        orca_crt::f64_to_i64(scale_(f64::from(self.spacing)))
    }

    /// `nozzle_diameter()`.
    #[must_use]
    pub fn nozzle_diameter(&self) -> f32 {
        self.nozzle_diameter
    }

    /// `bridge()`.
    #[must_use]
    pub fn bridge(&self) -> bool {
        self.bridge
    }

    /// `mm3_per_mm()`: the cross section, a circle of the width for a bridge, else
    /// `h (w - h (1 - π/4))`, computed in `double` and rounded to `float`.
    ///
    /// # Errors
    /// [`FlowError::NegativeFlow`] when it is not positive.
    #[allow(clippy::cast_possible_truncation)]
    pub fn mm3_per_mm(&self) -> Result<f64, FlowError> {
        let res: f32 = if self.bridge {
            // The area of a circle of the width's diameter.
            (f64::from(self.width * self.width) * 0.25 * PI) as f32
        } else {
            // A rectangle with half circles at its sides.
            (f64::from(self.height)
                * (f64::from(self.width) - f64::from(self.height) * (1. - 0.25 * PI)))
                as f32
        };
        if f64::from(res) <= 0. {
            return Err(FlowError::NegativeFlow);
        }
        Ok(f64::from(res))
    }

    /// `scaled_elephant_foot_spacing()`: `coord_t(0.5f * float(scaled_width() + 0.6f *
    /// scaled_spacing()))`, in `float`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn scaled_elephant_foot_spacing(&self) -> Coord {
        let sum = self.scaled_width() as f32 + 0.6f32 * self.scaled_spacing() as f32;
        orca_crt::f32_to_i64(0.5f32 * sum)
    }

    /// `operator<`: by `mm3_per_mm()`.
    ///
    /// # Errors
    /// What `mm3_per_mm()` of either gives.
    pub fn less_than(&self, rhs: &Flow) -> Result<bool, FlowError> {
        Ok(self.mm3_per_mm()? < rhs.mm3_per_mm()?)
    }

    /// `with_width(width)`: this flow with another width and its spacing (a bridge stays a
    /// bridge: the C++'s assert does not run).
    ///
    /// # Errors
    /// [`FlowError::NegativeSpacing`].
    pub fn with_width(&self, width: f32) -> Result<Flow, FlowError> {
        Ok(Self::from_parts(
            width,
            self.height,
            Self::rounded_rectangle_extrusion_spacing(width, self.height)?,
            self.nozzle_diameter,
            self.bridge,
        ))
    }

    /// `with_height(height)`: this flow with another height and its spacing.
    ///
    /// # Errors
    /// [`FlowError::NegativeSpacing`].
    pub fn with_height(&self, height: f32) -> Result<Flow, FlowError> {
        Ok(Self::from_parts(
            self.width,
            height,
            Self::rounded_rectangle_extrusion_spacing(self.width, height)?,
            self.nozzle_diameter,
            self.bridge,
        ))
    }

    /// `with_spacing(new_spacing)`: the width moved with the spacing, keeping the gap between
    /// lines; a bridge's diameter (width and height) moved by the gap.
    ///
    /// # Errors
    /// `InvalidArgument` when a line's new width is below its height.
    pub fn with_spacing(&self, new_spacing: f32) -> Result<Flow, FlowError> {
        let mut out = *self;
        if self.bridge {
            // The diameter of the round extrusion.
            let gap = self.spacing - self.width;
            let new_diameter = new_spacing - gap;
            out.width = new_diameter;
            out.height = new_diameter;
        } else {
            out.width += new_spacing - self.spacing;
            if out.width < out.height {
                return Err(FlowError::InvalidArgument(
                    "Invalid spacing supplied to Flow::with_spacing(), check your layer height \
                     and extrusion width"
                        .to_string(),
                ));
            }
        }
        out.spacing = new_spacing;
        Ok(out)
    }

    /// `with_cross_section(area_new)`: the width or height changed to reach the cross section,
    /// keeping the spacing; a round extrusion when it shrinks below the height.
    ///
    /// # Errors
    /// What `mm3_per_mm()` and `with_width` give.
    #[allow(clippy::cast_possible_truncation)]
    pub fn with_cross_section(&self, area_new: f32) -> Result<Flow, FlowError> {
        // Keep the spacing.
        let area = self.mm3_per_mm()? as f32;
        if f64::from(area_new) > f64::from(area) + EPSILON {
            // More flow.
            let new_full_spacing = area_new / self.height;
            if new_full_spacing > self.spacing {
                // The spacing filled with no gap: the extrusion grows in height.
                let height = area_new / self.spacing;
                Ok(Self::from_parts(
                    Self::rounded_rectangle_extrusion_width_from_spacing(self.spacing, height),
                    height,
                    self.spacing,
                    self.nozzle_diameter,
                    false,
                ))
            } else {
                self.with_width(Self::rounded_rectangle_extrusion_width_from_spacing(
                    area / self.height,
                    self.height,
                ))
            }
        } else if f64::from(area_new) < f64::from(area) - EPSILON {
            // Less flow.
            let width_new = self.width - (area - area_new) / self.height;
            if width_new > self.height {
                // A narrower line.
                self.with_width(width_new)
            } else {
                // A round extrusion.
                let dmr = orca_crt::sqrt(f64::from(area_new) / M_PI) as f32;
                Ok(Self::from_parts(
                    dmr,
                    dmr,
                    self.spacing,
                    self.nozzle_diameter,
                    false,
                ))
            }
        } else {
            Ok(*self)
        }
    }

    /// `with_flow_ratio(ratio)`: `with_cross_section(mm3_per_mm() * ratio)`, the product rounded
    /// to the `float` parameter.
    ///
    /// # Errors
    /// What `mm3_per_mm()` and `with_cross_section` give.
    #[allow(clippy::cast_possible_truncation)]
    pub fn with_flow_ratio(&self, ratio: f64) -> Result<Flow, FlowError> {
        self.with_cross_section((self.mm3_per_mm()? * ratio) as f32)
    }

    /// `bridging_flow(dmr, nozzle_diameter)`: a round thread of diameter `dmr`.
    #[must_use]
    pub fn bridging_flow(dmr: f32, nozzle_diameter: f32) -> Flow {
        Self::from_parts(
            dmr,
            dmr,
            Self::bridge_extrusion_spacing(dmr),
            nozzle_diameter,
            true,
        )
    }

    /// `new_from_config_width(role, width, nozzle_diameter, height)`: the width of the setting
    /// (a percent of the nozzle), or the role's automatic width when it is zero or less.
    ///
    /// # Errors
    /// `InvalidArgument` for a height not above 0; [`FlowError::NegativeSpacing`].
    #[allow(clippy::cast_possible_truncation)]
    pub fn new_from_config_width(
        role: FlowRole,
        width: &orca_config::ConfigOptionFloatOrPercent,
        nozzle_diameter: f32,
        height: f32,
    ) -> Result<Flow, FlowError> {
        if height <= 0. {
            return Err(FlowError::InvalidArgument(
                "Invalid flow height supplied to new_from_config_width()".to_string(),
            ));
        }
        let w = if !width.percent && width.value <= 0. {
            // Left at 0: a sane default.
            Self::auto_extrusion_width(role, nozzle_diameter)
        } else {
            // The setting's value.
            width.get_abs_value(f64::from(nozzle_diameter)) as f32
        };
        Ok(Self::from_parts(
            w,
            height,
            Self::rounded_rectangle_extrusion_spacing(w, height)?,
            nozzle_diameter,
            false,
        ))
    }

    /// `rounded_rectangle_extrusion_spacing(width, height)`: `width - height * float(1 - π/4)`,
    /// in `float`.
    ///
    /// # Errors
    /// [`FlowError::NegativeSpacing`] when it is not positive.
    #[allow(clippy::cast_possible_truncation)]
    pub fn rounded_rectangle_extrusion_spacing(width: f32, height: f32) -> Result<f32, FlowError> {
        let out = width - height * ((1. - 0.25 * PI) as f32);
        if out <= 0.0f32 {
            return Err(FlowError::NegativeSpacing);
        }
        Ok(out)
    }

    /// `rounded_rectangle_extrusion_width_from_spacing(spacing, height)`: `spacing + height (1 -
    /// π/4)` in `double`, rounded to `float`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn rounded_rectangle_extrusion_width_from_spacing(spacing: f32, height: f32) -> f32 {
        (f64::from(spacing) + f64::from(height) * (1. - 0.25 * PI)) as f32
    }

    /// `bridge_extrusion_spacing(dmr)`: `dmr + 0.05` in `double`, rounded to `float`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn bridge_extrusion_spacing(dmr: f32) -> f32 {
        (f64::from(dmr) + BRIDGE_EXTRA_SPACING) as f32
    }

    /// `auto_extrusion_width(role, nozzle_diameter)`: the nozzle for support and top surfaces,
    /// else 1.125 times it.
    #[must_use]
    pub fn auto_extrusion_width(role: FlowRole, nozzle_diameter: f32) -> f32 {
        match role {
            FlowRole::SupportMaterial
            | FlowRole::SupportMaterialInterface
            | FlowRole::SupportTransition
            | FlowRole::TopSolidInfill => nozzle_diameter,
            FlowRole::ExternalPerimeter
            | FlowRole::Perimeter
            | FlowRole::SolidInfill
            | FlowRole::Infill => 1.125f32 * nozzle_diameter,
        }
    }

    /// `extrusion_width(opt_key, config, first_printing_extruder)`: [`Flow::extrusion_width_of`]
    /// of the key's own float-or-percent option.
    ///
    /// # Errors
    /// As [`Flow::extrusion_width_of`].
    pub fn extrusion_width(
        opt_key: &str,
        config: &dyn ConfigBase,
        first_printing_extruder: u32,
    ) -> Result<f64, FlowError> {
        let opt = float_or_percent(config, opt_key);
        Self::extrusion_width_of(opt_key, opt, config, first_printing_extruder)
    }

    /// `extrusion_width(opt_key, opt, config, first_printing_extruder)`: the line width a setting
    /// gives, a hint for the user and the placeholders. A zero setting falls back to `line_width`,
    /// a percent is of the first extruder's nozzle, and a zero `line_width` gives the role's
    /// automatic width; `bridge_line_width` falls back to `internal_solid_infill_line_width`.
    ///
    /// # Errors
    /// [`FlowError::MissingVariable`] when `nozzle_diameter` or a fallback is missing;
    /// [`FlowError::Runtime`] for a key with no flow role that reaches the automatic width.
    ///
    /// # Panics
    /// With a `NotPorted` payload when `opt` is missing, or the nozzles are an empty list, which
    /// the C++ dereferences.
    pub fn extrusion_width_of(
        opt_key: &str,
        opt: Option<(f64, bool)>,
        config: &dyn ConfigBase,
        first_printing_extruder: u32,
    ) -> Result<f64, FlowError> {
        let Some(nozzles) = floats(config, "nozzle_diameter") else {
            return Err(throw_on_missing_variable(opt_key, "nozzle_diameter"));
        };
        let Some(&nozzle) = nozzles
            .get(first_printing_extruder as usize)
            .or_else(|| nozzles.first())
        else {
            not_ported(
                "Flow::extrusion_width",
                "nozzle_diameter is an empty list (get_at reads its front)",
            )
        };
        #[allow(clippy::cast_possible_truncation)]
        let nozzle_diameter = nozzle as f32;

        let Some(mut opt) = opt else {
            not_ported(
                "Flow::extrusion_width",
                "the key has no float-or-percent option (the C++ dereferences null)",
            )
        };

        if opt_key == "bridge_line_width" {
            if opt.1 {
                let bridge_width = get_abs_value(opt, f64::from(nozzle_diameter));
                if bridge_width > 0. {
                    return Ok(bridge_width);
                }
            } else if opt.0 > 0. {
                return Ok(opt.0);
            }

            let Some(solid) = float_or_percent(config, "internal_solid_infill_line_width") else {
                return Err(throw_on_missing_variable(
                    opt_key,
                    "internal_solid_infill_line_width",
                ));
            };
            return Self::extrusion_width_of(
                "internal_solid_infill_line_width",
                Some(solid),
                config,
                first_printing_extruder,
            );
        }

        if opt.0 == 0. {
            // The role's own width is 0: try the width of every role.
            let Some(line_width) = float_or_percent(config, "line_width") else {
                return Err(throw_on_missing_variable(opt_key, "line_width"));
            };
            opt = line_width;
        }

        if opt.1 {
            return Ok(get_abs_value(opt, f64::from(nozzle_diameter)));
        }

        if opt.0 == 0. {
            // Left at 0: a sane default.
            return Ok(f64::from(Self::auto_extrusion_width(
                opt_key_to_flow_role(opt_key)?,
                nozzle_diameter,
            )));
        }

        Ok(opt.0)
    }
}

/// `opt_key_to_flow_role(opt_key)`: the role a line width key stands for.
fn opt_key_to_flow_role(opt_key: &str) -> Result<FlowRole, FlowError> {
    if opt_key == "inner_wall_line_width"
        // or all the defaults:
        || opt_key == "line_width"
        || opt_key == "initial_layer_line_width"
    {
        Ok(FlowRole::Perimeter)
    } else if opt_key == "outer_wall_line_width" {
        Ok(FlowRole::ExternalPerimeter)
    } else if opt_key == "sparse_infill_line_width" {
        Ok(FlowRole::Infill)
    } else if opt_key == "internal_solid_infill_line_width" || opt_key == "bridge_line_width" {
        Ok(FlowRole::SolidInfill)
    } else if opt_key == "top_surface_line_width" {
        Ok(FlowRole::TopSolidInfill)
    } else if opt_key == "support_line_width" {
        Ok(FlowRole::SupportMaterial)
    } else {
        Err(FlowError::Runtime(
            "opt_key_to_flow_role: invalid argument".to_string(),
        ))
    }
}

/// `throw_on_missing_variable(opt_key, dependent_opt_key)`: the boost::format text of
/// "Failed to calculate line width of %1%. Cannot get value of \"%2%\" ".
fn throw_on_missing_variable(opt_key: &str, dependent_opt_key: &str) -> FlowError {
    FlowError::MissingVariable(format!(
        "Failed to calculate line width of {opt_key}. Cannot get value of \"{dependent_opt_key}\" "
    ))
}

/// `print_config.nozzle_diameter.get_at(i)`: nozzle `i`, or the first past the end.
///
/// # Panics
/// With a `NotPorted` payload naming `function` when the list is empty (the C++ asserts, then
/// reads its front).
pub(crate) fn nozzle_diameter_at(
    print_config: &PrintConfig,
    i: usize,
    function: &'static str,
) -> f64 {
    if print_config.nozzle_diameter.values.is_empty() {
        not_ported(
            function,
            "nozzle_diameter is an empty list (get_at reads its front)",
        )
    }
    *print_config.nozzle_diameter.get_at(i)
}

/// `get_at(id - 1)` of an `int` filament id: the difference converted to `size_t`, so filament 0
/// reads past the end (the first nozzle).
#[allow(clippy::cast_sign_loss)]
fn filament_index(id: i32) -> usize {
    id.wrapping_sub(1) as isize as usize
}

/// `support_material_flow(object, layer_height)`: the support's flow, from `support_line_width`
/// (the object's `line_width` when it is not above 0) and the support filament's nozzle (filament
/// 0, the current one, reads the first nozzle), at `layer_height` or, when that is not above 0,
/// the object's layer height. `print` is the object's print.
///
/// # Errors
/// What `Flow::new_from_config_width` gives.
///
/// # Panics
/// When the print's `nozzle_diameter` is empty.
#[allow(clippy::cast_possible_truncation)]
pub fn support_material_flow(
    object: &PrintObject,
    print: &Print,
    layer_height: f32,
) -> Result<Flow, FlowError> {
    let width = if object.config().support_line_width.value > 0. {
        object.config().support_line_width
    } else {
        object.config().line_width
    };
    Flow::new_from_config_width(
        FlowRole::SupportMaterial,
        &width,
        nozzle_diameter_at(
            print.config(),
            filament_index(object.config().support_filament.value),
            "support_material_flow",
        ) as f32,
        if layer_height > 0. {
            layer_height
        } else {
            object.config().layer_height.value as f32
        },
    )
}

/// `support_transition_flow(object)` (BBS): tree support's transition is a bridge flow of the
/// support filament's nozzle.
///
/// # Panics
/// When the print's `nozzle_diameter` is empty.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn support_transition_flow(object: &PrintObject, print: &Print) -> Flow {
    let dmr = nozzle_diameter_at(
        print.config(),
        filament_index(object.config().support_filament.value),
        "support_transition_flow",
    ) as f32;
    Flow::bridging_flow(dmr, dmr)
}

/// `support_material_1st_layer_flow(object, layer_height)`: the support's first-layer flow, from
/// the first layer's width when set, else `support_line_width` (either replaced by the object's
/// `line_width` when not above 0), at `layer_height` or the first layer's height.
///
/// # Errors
/// What `Flow::new_from_config_width` gives.
///
/// # Panics
/// When the print's `nozzle_diameter` is empty.
#[allow(clippy::cast_possible_truncation)]
pub fn support_material_1st_layer_flow(
    object: &PrintObject,
    print: &Print,
    layer_height: f32,
) -> Result<Flow, FlowError> {
    let print_config = print.config();
    let width = if print_config.initial_layer_line_width.value > 0. {
        print_config.initial_layer_line_width
    } else {
        object.config().support_line_width
    };
    Flow::new_from_config_width(
        FlowRole::SupportMaterial,
        &(if width.value > 0. {
            width
        } else {
            object.config().line_width
        }),
        nozzle_diameter_at(
            print_config,
            filament_index(object.config().support_filament.value),
            "support_material_1st_layer_flow",
        ) as f32,
        if layer_height > 0. {
            layer_height
        } else {
            print_config.initial_layer_print_height.value as f32
        },
    )
}

/// `support_material_interface_flow(object, layer_height)`: the support interface's flow, from
/// `support_line_width` (or the object's `line_width`) and the interface filament's nozzle, at
/// `layer_height` or the object's layer height.
///
/// # Errors
/// What `Flow::new_from_config_width` gives.
///
/// # Panics
/// When the print's `nozzle_diameter` is empty.
#[allow(clippy::cast_possible_truncation)]
pub fn support_material_interface_flow(
    object: &PrintObject,
    print: &Print,
    layer_height: f32,
) -> Result<Flow, FlowError> {
    let width = if object.config().support_line_width.value > 0. {
        object.config().support_line_width
    } else {
        object.config().line_width
    };
    Flow::new_from_config_width(
        FlowRole::SupportMaterialInterface,
        &width,
        nozzle_diameter_at(
            print.config(),
            filament_index(object.config().support_interface_filament.value),
            "support_material_interface_flow",
        ) as f32,
        if layer_height > 0. {
            layer_height
        } else {
            object.config().layer_height.value as f32
        },
    )
}

/// `ConfigOptionFloatOrPercent::get_abs_value(ratio_over)` of a value and its percent flag.
fn get_abs_value(opt: (f64, bool), ratio_over: f64) -> f64 {
    if opt.1 {
        ratio_over * opt.0 / 100.0
    } else {
        opt.0
    }
}

/// `config.option<ConfigOptionFloatOrPercent>(key)`: the value and percent flag, or none when the
/// config lacks the key or holds it as another type.
fn float_or_percent(config: &dyn ConfigBase, key: &str) -> Option<(f64, bool)> {
    let o = config.option(key)?;
    if o.opt_type() != OptionType::FloatOrPercent {
        return None;
    }
    match o.to_value() {
        OptionValue::FloatOrPercent { value, percent } => Some((value, percent)),
        _ => None,
    }
}

/// `config.option<ConfigOptionFloats>(key)`: the values, or none when the config lacks the key
/// or holds it as another type.
fn floats(config: &dyn ConfigBase, key: &str) -> Option<Vec<f64>> {
    let o = config.option(key)?;
    if o.opt_type() != OptionType::Floats {
        return None;
    }
    match o.to_value() {
        OptionValue::Floats { values, .. } => Some(values),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_flow_has_no_cross_section() {
        let f = Flow::default();
        assert_eq!(f.mm3_per_mm(), Err(FlowError::NegativeFlow));
    }

    #[test]
    fn equality_ignores_the_spacing() {
        let mut a = Flow::new(0.45, 0.2, 0.4).unwrap();
        let b = a;
        a.set_spacing(1.0);
        assert_eq!(a, b);
    }

    #[test]
    fn a_line_narrower_than_its_rounding_has_no_spacing() {
        assert_eq!(
            Flow::rounded_rectangle_extrusion_spacing(0.0, 0.2),
            Err(FlowError::NegativeSpacing)
        );
    }
}
