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
// Carries OrcaSlicer code: src/libslic3r/PrintBase.hpp lines 1-686 of OrcaSlicer v2.4.2, and the
// members PrintBase holds of PlaceholderParser.hpp (the parser's own config), translated on
// 2026-11-14 (orca-o3-layer-2). Copyright the OrcaSlicer authors, the BambuStudio authors (Bambu
// Lab) and the PrusaSlicer authors (Prusa Research), under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).
//
// Carries OrcaSlicer code: src/libslic3r/PrintBase.cpp lines 1-159, with the members of
// PrintBase.hpp that call it (`PrintBaseWithState::set_done` and `active_step_add_warning`,
// `PrintObjectBaseWithState::set_done` and `active_step_add_warning`), and
// `PlaceholderParser::update_timestamp` and `update_user_name` of PlaceholderParser.cpp lines
// 78-107, of OrcaSlicer v2.4.2, translated on 2026-11-14 (orca-o3-layer-3). Copyright the
// OrcaSlicer authors, the BambuStudio authors (Bambu Lab), the PrusaSlicer authors (Prusa Research)
// and the Slic3r authors, under the GNU Affero General Public License, version 3 (AGPL-3.0;
// LICENSE).
//
// Carries Boost code: put_inf_nan_impl of boost/lexical_cast/detail/inf_nan.hpp and shl_real_type
// of boost/lexical_cast/detail/converter_lexical_streams.hpp (lexical_cast<std::string>(double)),
// of Boost 1.84.0, the Boost OrcaSlicer v2.4.2's dependency build pins, translated on 2026-11-14
// (orca-o3-layer-3). Copyright Kevlin Henney, 2000-2005, Alexander Nasonov, 2006-2010, and Antony
// Polukhin, 2011-2023, under the Boost Software License 1.0 (licenses/BSL-1.0.txt).

//! PrintBase.hpp and PrintBase.cpp: the state machinery a print and its objects share, and the
//! data a print holds whatever its technology.
//!
//! - [`PrintState`] (`PrintState<StepType, COUNT>`): each step's state (invalid, started, done),
//!   the timestamp of its last change and its warnings, and the active step. A timestamp comes
//!   from [`G_LAST_TIMESTAMP`], one counter for the process, as the C++'s static is. The C++
//!   locks the print's state mutex around each change, to keep the UI thread out; a CLI run has
//!   no UI thread, and Rust's borrows give the same exclusion, so the mutex is not translated and
//!   the guarded and unguarded reads are the same read.
//! - [`PrintBase`]: the model copy, the full config and its copy before the extruders were
//!   applied, the placeholder parser, the plate's index, name and no-check flag, the status
//!   callback, the cancel status and the cancel callback. A cancel is the GUI's: in a CLI run
//!   nothing calls `cancel()`, so [`PrintBase::throw_if_canceled`] never fires there. When it
//!   does it panics with a [`CanceledException`] payload, as the C++ throws one.
//! - [`PrintBaseWithState`] and [`PrintObjectBaseWithState`]: the base with the state of its
//!   steps. An object's back-pointer to its print (`m_print`) is not held: the calls that need the
//!   print take its [`PrintBase`] as an argument. The object's `m_model_object` pointer is an
//!   index into the print's `model.objects`.
//!
//! - PrintBase.cpp (orca-o3-layer-3): [`PrintBase::set_status`] (to the callback, else a debug
//!   record), [`PrintBase::status_update_warnings`] and its object overload (to the callback,
//!   else an info record of a message that is not empty, named as MSVC's `__FUNCTION__` names it),
//!   [`PrintObjectBase::status_update_warnings`], [`PrintBase::update_object_placeholders`],
//!   [`PrintBase::output_filename`], [`PrintBase::output_filepath`] and [`PrintTryCancel`]; and
//!   the `set_done` and `active_step_add_warning` of both bases with state, which call them.
//!   The paths go through [`crate::fs_path`], Boost.Filesystem's v3 rules. A scale is written as
//!   `boost::lexical_cast<std::string>(double)` writes it: `%.17g`, or `inf`, `-inf`, `nan`,
//!   `-nan`.
//!
//! Not translated: `PlaceholderParser::process`, the macro language a non-empty
//! `filename_format` runs through, which is orca-gcode's (phase O4): [`PrintBase::output_filename`]
//! stops there with `NotPorted` (gcode-export). An empty `output_filepath` path, which takes the
//! name `Model::propose_export_file_name_and_path` proposes, stops the same way: a CLI run always
//! passes a path. `emptylayer_update_msg` is declared but never defined, so there is nothing to
//! carry. `PrintObjectBase::state_mutex` and `cancel_callback` (PrintBase.cpp 143-152) only
//! forward to the print: the calls here read the print's own [`PrintBase::cancel_callback`]. The
//! virtual members of `PrintBase` (`technology`, `clear`, `empty`, `print_object_ids`, `validate`,
//! `apply`, `set_task`, `process`, `export_cached_data`, `load_cached_data`, `finalize`,
//! `finished`, `output_filename`) are `Print`'s methods, written with their bodies.
//!
//! [`update_timestamp`] and [`update_user_name`] belong in orca-placeholder (PlaceholderParser's
//! statics) and are written here until it has the class.
//!
//! [`PlaceholderParser`] belongs in orca-placeholder, which has the parser's grammar but not its
//! class: this crate holds the parser's own config, the part `PrintBase` constructs (its external
//! config is the print's full config, which a caller passes).

use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use orca_config::OptionValue;
use orca_config::config::Config;
use orca_fmt::printf::{Arg, sprintf_string};
use orca_index::stop::not_ported;
use orca_mesh::log::{Severity, log};
use orca_model::model::Model;
use orca_model::object_id::ObjectID;

use crate::fs_path;

/// `enum StringExceptionType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum StringExceptionType {
    /// `STRING_EXCEPT_NOT_DEFINED`.
    #[default]
    NotDefined = 0,
    /// `STRING_EXCEPT_FILAMENT_NOT_MATCH_BED_TYPE`.
    FilamentNotMatchBedType = 1,
    /// `STRING_EXCEPT_FILAMENTS_DIFFERENT_TEMP`.
    FilamentsDifferentTemp = 2,
    /// `STRING_EXCEPT_OBJECT_COLLISION_IN_SEQ_PRINT`.
    ObjectCollisionInSeqPrint = 3,
    /// `STRING_EXCEPT_OBJECT_COLLISION_IN_LAYER_PRINT`.
    ObjectCollisionInLayerPrint = 4,
    /// `STRING_EXCEPT_LAYER_HEIGHT_EXCEEDS_LIMIT`.
    LayerHeightExceedsLimit = 5,
    /// `STRING_EXCEPT_COUNT`.
    Count = 6,
}

/// The `ObjectBase` a [`StringObjectException`] names: its class, which the CLI's
/// `index_of_object_base` tells apart by `dynamic_cast`, and its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectBaseRef {
    /// A `PrintObject`, by its id.
    PrintObject(ObjectID),
    /// A `ModelObject`, by its id.
    ModelObject(ObjectID),
    /// A `ModelInstance`, by its id.
    ModelInstance(ObjectID),
    /// Any other `ObjectBase`, by its id.
    Other(ObjectID),
}

/// `struct StringObjectException`: an error or warning with the object it is about. `{}`
/// value-initialises it: empty texts, no object, `STRING_EXCEPT_NOT_DEFINED`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringObjectException {
    /// `string`.
    pub string: String,
    /// `object`: null when none.
    pub object: Option<ObjectBaseRef>,
    /// `opt_key`.
    pub opt_key: String,
    /// `type`.
    pub exception_type: StringExceptionType,
    /// `is_warning`.
    pub is_warning: bool,
    /// `params`.
    pub params: Vec<String>,
}

/// `class CanceledException`, the payload of the panic a cancelled step raises.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanceledException;

impl CanceledException {
    /// `what()`.
    #[must_use]
    pub fn what(&self) -> &'static str {
        "Background processing has been canceled"
    }
}

/// `PrintStateBase::State`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// `INVALID`.
    #[default]
    Invalid,
    /// `STARTED`.
    Started,
    /// `DONE`.
    Done,
}

/// `PrintStateBase::WarningLevel`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WarningLevel {
    /// `NON_CRITICAL`.
    #[default]
    NonCritical,
    /// `CRITICAL`.
    Critical,
}

/// `PrintStateBase::SlicingNotificationType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum SlicingNotificationType {
    /// `SlicingDefaultNotification`: a normal status update, from `set_status`.
    #[default]
    SlicingDefaultNotification = 0,
    /// `SlicingReplaceInitEmptyLayers`.
    SlicingReplaceInitEmptyLayers = 1,
    /// `SlicingNeedSupportOn`.
    SlicingNeedSupportOn = 2,
    /// `SlicingEmptyGcodeLayers`.
    SlicingEmptyGcodeLayers = 3,
    /// `SlicingGcodeOverlap`.
    SlicingGcodeOverlap = 4,
}

/// `PrintStateBase::TimeStamp`.
pub type TimeStamp = usize;

/// `PrintStateBase::g_last_timestamp`: the last timestamp given, for the whole process. Its
/// definition (`= 0`) is PrintBase.cpp's line 22; a Rust static is declared where it is defined.
pub static G_LAST_TIMESTAMP: AtomicUsize = AtomicUsize::new(0);

/// `++ g_last_timestamp`.
fn next_timestamp() -> TimeStamp {
    G_LAST_TIMESTAMP
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
}

/// `PrintStateBase::StateWithTimeStamp`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateWithTimeStamp {
    /// `state`.
    pub state: State,
    /// `timestamp`.
    pub timestamp: TimeStamp,
}

/// `PrintStateBase::Warning`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// `level`: a critical warning is shown on G-code export in a modal dialog.
    pub level: WarningLevel,
    /// `current`: a warning that is not current is in an unknown state.
    pub current: bool,
    /// `message`: UTF-8, localized.
    pub message: String,
    /// `message_id`: 0 when the message identifies the warning itself.
    pub message_id: i32,
}

/// `PrintStateBase::StateWithWarnings`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateWithWarnings {
    /// `state`.
    pub state: State,
    /// `timestamp`.
    pub timestamp: TimeStamp,
    /// `warnings`.
    pub warnings: Vec<Warning>,
}

impl StateWithWarnings {
    /// `mark_warnings_non_current()`.
    pub fn mark_warnings_non_current(&mut self) {
        for w in &mut self.warnings {
            w.current = false;
        }
    }

    /// The `StateWithTimeStamp` slice of it.
    #[must_use]
    pub fn with_timestamp(&self) -> StateWithTimeStamp {
        StateWithTimeStamp {
            state: self.state,
            timestamp: self.timestamp,
        }
    }
}

/// A step enum a [`PrintState`] is instantiated over (`PrintStep`, `PrintObjectStep`): its value
/// as an index into the states, and back.
pub trait StepType: Copy {
    /// `static_cast<int>(step)`.
    fn index(self) -> usize;
    /// `static_cast<StepType>(index)`.
    fn from_index(index: usize) -> Self;
}

/// `PrintState<StepType, COUNT>`: the state of each of `COUNT` steps and the active step.
#[derive(Clone, Debug)]
pub struct PrintState<S: StepType, const COUNT: usize> {
    /// `m_state`.
    state: [StateWithWarnings; COUNT],
    /// `m_step_active`: the active step, or -1. A cancelled step may leave it set.
    step_active: i32,
    _step: core::marker::PhantomData<S>,
}

impl<S: StepType, const COUNT: usize> Default for PrintState<S, COUNT> {
    /// `PrintState()`.
    fn default() -> Self {
        Self {
            state: core::array::from_fn(|_| StateWithWarnings::default()),
            step_active: -1,
            _step: core::marker::PhantomData,
        }
    }
}

impl<S: StepType, const COUNT: usize> PrintState<S, COUNT> {
    /// `state_with_timestamp(step, mtx)`.
    #[must_use]
    pub fn state_with_timestamp(&self, step: S) -> StateWithTimeStamp {
        self.state[step.index()].with_timestamp()
    }

    /// `state_with_warnings(step, mtx)`.
    #[must_use]
    pub fn state_with_warnings(&self, step: S) -> StateWithWarnings {
        self.state[step.index()].clone()
    }

    /// `is_started(step, mtx)`.
    #[must_use]
    pub fn is_started(&self, step: S) -> bool {
        self.state_with_timestamp(step).state == State::Started
    }

    /// `is_done(step, mtx)`.
    #[must_use]
    pub fn is_done(&self, step: S) -> bool {
        self.state_with_timestamp(step).state == State::Done
    }

    /// `state_with_timestamp_unguarded(step)`.
    #[must_use]
    pub fn state_with_timestamp_unguarded(&self, step: S) -> StateWithTimeStamp {
        self.state_with_timestamp(step)
    }

    /// `is_started_unguarded(step)`.
    #[must_use]
    pub fn is_started_unguarded(&self, step: S) -> bool {
        self.is_started(step)
    }

    /// `is_done_unguarded(step)`.
    #[must_use]
    pub fn is_done_unguarded(&self, step: S) -> bool {
        self.is_done(step)
    }

    /// The active step, -1 for none.
    #[must_use]
    pub fn step_active(&self) -> i32 {
        self.step_active
    }

    /// `set_started(step, mtx, throw_if_canceled)`: false when the step is done already; else it
    /// starts, with a new timestamp and its warnings no longer current. A cancel throws (here
    /// panics, in `throw_if_canceled`) before anything changes.
    pub fn set_started(&mut self, step: S, throw_if_canceled: impl FnOnce()) -> bool {
        // If canceled, throw before changing the step state.
        throw_if_canceled();
        if self.state[step.index()].state == State::Done {
            return false;
        }
        let state = &mut self.state[step.index()];
        state.state = State::Started;
        state.timestamp = next_timestamp();
        state.mark_warnings_non_current();
        self.step_active = i32::try_from(step.index()).unwrap_or(i32::MAX);
        true
    }

    /// `set_done(step, mtx, throw_if_canceled)`: the step done, with a new timestamp, and its
    /// warnings that are not current removed; the timestamp, and whether any were removed (the
    /// UI then updates its warnings).
    pub fn set_done(&mut self, step: S, throw_if_canceled: impl FnOnce()) -> (TimeStamp, bool) {
        // If canceled, throw before changing the step state.
        throw_if_canceled();
        let state = &mut self.state[step.index()];
        state.state = State::Done;
        state.timestamp = next_timestamp();
        self.step_active = -1;
        // Remove all non-current warnings (std::remove_if keeps the order of the rest).
        let before = state.warnings.len();
        state.warnings.retain(|w| w.current);
        let update_warning_ui = state.warnings.len() != before;
        (state.timestamp, update_warning_ui)
    }

    /// `invalidate(step, cancel)`: a step that is not invalid becomes invalid, with a new
    /// timestamp; then `cancel` stops the background processing, and its warnings stop being
    /// current. Whether it was invalidated.
    pub fn invalidate(&mut self, step: S, cancel: impl FnOnce()) -> bool {
        let invalidated = self.state[step.index()].state != State::Invalid;
        if invalidated {
            let state = &mut self.state[step.index()];
            state.state = State::Invalid;
            state.timestamp = next_timestamp();
            // Raise the mutex, so that the cancel() callback can cancel the background processing.
            cancel();
            // Now the worker thread is stopped; the warnings can be changed.
            self.state[step.index()].mark_warnings_non_current();
            self.step_active = -1;
        }
        invalidated
    }

    /// `invalidate_multiple(step_begin, step_end, cancel)`: each step of the list that is not
    /// invalid becomes invalid with a new timestamp; if any did, `cancel`, then the warnings of
    /// every step of the list stop being current.
    pub fn invalidate_multiple(&mut self, steps: &[S], cancel: impl FnOnce()) -> bool {
        let mut invalidated = false;
        for &it in steps {
            let state = &mut self.state[it.index()];
            if state.state != State::Invalid {
                invalidated = true;
                state.state = State::Invalid;
                state.timestamp = next_timestamp();
            }
        }
        if invalidated {
            cancel();
            for &it in steps {
                self.state[it.index()].mark_warnings_non_current();
            }
            self.step_active = -1;
        }
        invalidated
    }

    /// `invalidate_all(cancel)`: as `invalidate_multiple` over every step.
    pub fn invalidate_all(&mut self, cancel: impl FnOnce()) -> bool {
        let mut invalidated = false;
        for state in &mut self.state {
            if state.state != State::Invalid {
                invalidated = true;
                state.state = State::Invalid;
                state.timestamp = next_timestamp();
            }
        }
        if invalidated {
            cancel();
            for state in &mut self.state {
                state.mark_warnings_non_current();
            }
            self.step_active = -1;
        }
        invalidated
    }

    /// `active_step_add_warning(warning_level, message, message_id, mtx)`: the warning added to
    /// the active step, or the one of the same message (`message_id` 0) or id brought up to date
    /// and made current; the step, and whether the UI must update.
    pub fn active_step_add_warning(
        &mut self,
        warning_level: WarningLevel,
        message: &str,
        message_id: i32,
    ) -> (S, bool) {
        let Ok(active) = usize::try_from(self.step_active) else {
            not_ported(
                "PrintState::active_step_add_warning",
                "no step is active (the C++ asserts, then reads m_state[-1])",
            )
        };
        let state = &mut self.state[active];
        let mut retval = (S::from_index(active), true);
        // Does a warning of the same level and message or message_id exist already?
        let it = if message_id == 0 {
            state
                .warnings
                .iter()
                .position(|w| w.message_id == 0 && w.message == message)
        } else {
            state
                .warnings
                .iter()
                .position(|w| w.message_id == message_id)
        };
        match it {
            // No: create a new warning and update the UI.
            None => state.warnings.push(Warning {
                level: warning_level,
                current: true,
                message: message.to_string(),
                message_id,
            }),
            Some(i) => {
                let w = &mut state.warnings[i];
                if w.message != message || w.level != warning_level {
                    // Yes, but it needs an update.
                    w.message = message.to_string();
                    w.level = warning_level;
                    w.current = true;
                } else if w.current {
                    // Yes, and it is current: no UI update.
                    retval.1 = false;
                } else {
                    // Yes, but not current: mark it current.
                    w.current = true;
                }
            }
        }
        retval
    }
}

/// `class PrintObjectBase`: an object's id (`ObjectBase`) and its model object, an index into
/// the print's `model.objects` (the C++ `m_model_object` points into the print's model copy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintObjectBase {
    /// `ObjectBase::m_id`.
    pub object_id: ObjectID,
    /// `m_model_object`, as an index into the print's `model.objects`.
    pub model_object: usize,
}

impl PrintObjectBase {
    /// `PrintObjectBase(model_object)`: `ObjectBase()` takes a new id.
    #[must_use]
    pub fn new(model_object: usize) -> Self {
        Self {
            object_id: ObjectID::generate_new_id(),
            model_object,
        }
    }

    /// `id()`.
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.object_id
    }

    /// `model_object()`: the print's model object of this object.
    #[must_use]
    pub fn model_object<'a>(&self, model: &'a Model) -> &'a orca_model::model::ModelObject {
        &model.objects[self.model_object]
    }
}

/// `PrintBase::ApplyStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ApplyStatus {
    /// `APPLY_STATUS_UNCHANGED`: no change after `Print::apply()`.
    #[default]
    Unchanged = 0,
    /// `APPLY_STATUS_CHANGED`: data changed, but no result was invalidated.
    Changed = 1,
    /// `APPLY_STATUS_INVALIDATED`: data changed and invalidated calculated steps.
    Invalidated = 2,
}

/// `PrintBase::TaskParams`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskParams {
    /// `single_model_object`: when valid, the processing is limited to this model object.
    pub single_model_object: ObjectID,
    /// `single_model_instance_only`.
    pub single_model_instance_only: bool,
    /// `to_object_step`: when not negative, stop at the successive object step.
    pub to_object_step: i32,
    /// `to_print_step`: when not negative, stop at the successive print step.
    pub to_print_step: i32,
}

impl Default for TaskParams {
    /// `TaskParams()`.
    fn default() -> Self {
        Self {
            single_model_object: ObjectID::new(0),
            single_model_instance_only: false,
            to_object_step: -1,
            to_print_step: -1,
        }
    }
}

/// `SlicingStatus::FlagBits::DEFAULT`.
pub const SLICING_STATUS_DEFAULT: u32 = 0;
/// `SlicingStatus::FlagBits::RELOAD_SCENE`.
pub const SLICING_STATUS_RELOAD_SCENE: u32 = 1 << 1;
/// `SlicingStatus::FlagBits::RELOAD_SLA_SUPPORT_POINTS`.
pub const SLICING_STATUS_RELOAD_SLA_SUPPORT_POINTS: u32 = 1 << 2;
/// `SlicingStatus::FlagBits::RELOAD_SLA_PREVIEW`.
pub const SLICING_STATUS_RELOAD_SLA_PREVIEW: u32 = 1 << 3;
/// `SlicingStatus::FlagBits::UPDATE_PRINT_STEP_WARNINGS`.
pub const SLICING_STATUS_UPDATE_PRINT_STEP_WARNINGS: u32 = 1 << 4;
/// `SlicingStatus::FlagBits::UPDATE_PRINT_OBJECT_STEP_WARNINGS`.
pub const SLICING_STATUS_UPDATE_PRINT_OBJECT_STEP_WARNINGS: u32 = 1 << 5;

/// `PrintBase::SlicingStatus`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlicingStatus {
    /// `percent`: -1 when not given.
    pub percent: i32,
    /// `text`.
    pub text: String,
    /// `flags`: a bitmap of the `SLICING_STATUS_*` flag bits.
    pub flags: u32,
    /// `warning_object_id`: a print's or an object's id, by the flags.
    pub warning_object_id: ObjectID,
    /// `warning_step`: -1 when not given.
    pub warning_step: i32,
    /// `message_type`.
    pub message_type: SlicingNotificationType,
    /// `warning_level`.
    pub warning_level: WarningLevel,
}

impl SlicingStatus {
    /// `SlicingStatus(percent, text, flags, warning_step, msg_type, warning_level)`.
    #[must_use]
    pub fn new(
        percent: i32,
        text: &str,
        flags: u32,
        warning_step: i32,
        msg_type: SlicingNotificationType,
        warning_level: WarningLevel,
    ) -> Self {
        Self {
            percent,
            text: text.to_string(),
            flags,
            warning_object_id: ObjectID::default(),
            warning_step,
            message_type: msg_type,
            warning_level,
        }
    }

    /// `SlicingStatus(const PrintBase &print, warning_step, text, msg_type, warning_level)`: a
    /// print step's warning.
    #[must_use]
    pub fn for_print(
        print: &PrintBase,
        warning_step: i32,
        text: &str,
        msg_type: SlicingNotificationType,
        warning_level: WarningLevel,
    ) -> Self {
        Self {
            percent: -1,
            text: text.to_string(),
            flags: SLICING_STATUS_UPDATE_PRINT_STEP_WARNINGS,
            warning_object_id: print.id(),
            warning_step,
            message_type: msg_type,
            warning_level,
        }
    }

    /// `SlicingStatus(const PrintObjectBase &print_object, warning_step, text, msg_type,
    /// warning_level)`: an object step's warning.
    #[must_use]
    pub fn for_print_object(
        print_object: &PrintObjectBase,
        warning_step: i32,
        text: &str,
        msg_type: SlicingNotificationType,
        warning_level: WarningLevel,
    ) -> Self {
        Self {
            percent: -1,
            text: text.to_string(),
            flags: SLICING_STATUS_UPDATE_PRINT_OBJECT_STEP_WARNINGS,
            warning_object_id: print_object.id(),
            warning_step,
            message_type: msg_type,
            warning_level,
        }
    }
}

/// `PrintBase::status_callback_type`.
pub type StatusCallback = Box<dyn Fn(&SlicingStatus) + Send + Sync>;

/// `PrintBase::cancel_callback_type`.
pub type CancelCallback = Box<dyn Fn() + Send + Sync>;

/// `PrintBase::CancelStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum CancelStatus {
    /// `NOT_CANCELED`: background processing runs.
    #[default]
    NotCanceled = 0,
    /// `CANCELED_BY_USER`: the user pressed Cancel or closed the application.
    CanceledByUser = 1,
    /// `CANCELED_INTERNAL`: cancelled from `Print::apply()` through an invalidation.
    CanceledInternal = 2,
}

impl CancelStatus {
    fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::CanceledByUser,
            2 => Self::CanceledInternal,
            _ => Self::NotCanceled,
        }
    }
}

/// `PlaceholderParser`, the part `PrintBase` constructs: the parser's own config. Its external
/// config (`m_external_config`, the print's full config, which the C++ points at) is not held:
/// whoever evaluates a template passes it. Belongs in orca-placeholder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaceholderParser {
    /// `m_config`.
    config: Config,
}

impl PlaceholderParser {
    /// `PlaceholderParser(external_config)`: an empty config of its own.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `clear_config()`.
    pub fn clear_config(&mut self) {
        self.config = Config::default();
    }

    /// `config()`.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// `config_writable()`.
    pub fn config_writable(&mut self) -> &mut Config {
        &mut self.config
    }
}

/// `class PrintBase`: what a print holds whatever its technology.
pub struct PrintBase {
    /// `ObjectBase::m_id`.
    pub object_id: ObjectID,
    /// `m_model`: the print's copy of the model.
    pub model: Model,
    /// `m_full_print_config`.
    pub full_print_config: Config,
    /// `m_ori_full_print_config`: the full print config before the extruders were applied.
    pub ori_full_print_config: Config,
    /// `m_placeholder_parser`.
    pub placeholder_parser: PlaceholderParser,
    /// `m_plate_index`.
    pub plate_index: i32,
    /// `m_no_check`.
    pub no_check: bool,
    /// `m_plate_name`.
    pub plate_name: String,
    /// `m_status_callback`: none prints the default message.
    pub status_callback: Option<StatusCallback>,
    /// `m_cancel_status`.
    cancel_status: AtomicI32,
    /// `m_cancel_callback`.
    cancel_callback: CancelCallback,
}

impl core::fmt::Debug for PrintBase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrintBase")
            .field("object_id", &self.object_id)
            .field("plate_index", &self.plate_index)
            .field("no_check", &self.no_check)
            .field("plate_name", &self.plate_name)
            .field("cancel_status", &self.cancel_status())
            .finish_non_exhaustive()
    }
}

impl Default for PrintBase {
    fn default() -> Self {
        Self::new()
    }
}

impl PrintBase {
    /// `PrintBase()`: a new id (`ObjectBase()`), then the members in their order (the model copy
    /// takes the next id), and `restart()`.
    #[must_use]
    pub fn new() -> Self {
        let object_id = ObjectID::generate_new_id();
        let model = Model::default();
        let out = Self {
            object_id,
            model,
            full_print_config: Config::default(),
            ori_full_print_config: Config::default(),
            placeholder_parser: PlaceholderParser::new(),
            plate_index: 0,
            no_check: false,
            plate_name: String::new(),
            status_callback: None,
            cancel_status: AtomicI32::new(CancelStatus::NotCanceled as i32),
            cancel_callback: Box::new(|| {}),
        };
        out.restart();
        out
    }

    /// `id()`.
    #[must_use]
    pub fn id(&self) -> ObjectID {
        self.object_id
    }

    /// `model()`.
    #[must_use]
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// `set_status_default()`: the default console status.
    pub fn set_status_default(&mut self) {
        self.status_callback = None;
    }

    /// `set_status_silent()`: no status output.
    pub fn set_status_silent(&mut self) {
        self.status_callback = Some(Box::new(|_| {}));
    }

    /// `set_status_callback(cb)`.
    pub fn set_status_callback(&mut self, cb: StatusCallback) {
        self.status_callback = Some(cb);
    }

    /// `set_cancel_callback(cancel_callback)`.
    pub fn set_cancel_callback(&mut self, cancel_callback: CancelCallback) {
        self.cancel_callback = cancel_callback;
    }

    /// `cancel_status()`.
    #[must_use]
    pub fn cancel_status(&self) -> CancelStatus {
        CancelStatus::from_i32(self.cancel_status.load(Ordering::Acquire))
    }

    /// `canceled()`.
    #[must_use]
    pub fn canceled(&self) -> bool {
        self.cancel_status.load(Ordering::Acquire) != CancelStatus::NotCanceled as i32
    }

    /// `cancel()`: by the user.
    pub fn cancel(&self) {
        self.cancel_status
            .store(CancelStatus::CanceledByUser as i32, Ordering::SeqCst);
    }

    /// `cancel_internal()`.
    pub fn cancel_internal(&self) {
        self.cancel_status
            .store(CancelStatus::CanceledInternal as i32, Ordering::SeqCst);
    }

    /// `restart()`.
    pub fn restart(&self) {
        self.cancel_status
            .store(CancelStatus::NotCanceled as i32, Ordering::SeqCst);
    }

    /// `placeholder_parser()`.
    #[must_use]
    pub fn placeholder_parser(&self) -> &PlaceholderParser {
        &self.placeholder_parser
    }

    /// `full_print_config()`.
    #[must_use]
    pub fn full_print_config(&self) -> &Config {
        &self.full_print_config
    }

    /// `get_plate_index()`.
    #[must_use]
    pub fn get_plate_index(&self) -> i32 {
        self.plate_index
    }

    /// `set_plate_index(index)`.
    pub fn set_plate_index(&mut self, index: i32) {
        self.plate_index = index;
    }

    /// `get_no_check_flag()`.
    #[must_use]
    pub fn get_no_check_flag(&self) -> bool {
        self.no_check
    }

    /// `set_no_check_flag(no_check)`.
    pub fn set_no_check_flag(&mut self, no_check: bool) {
        self.no_check = no_check;
    }

    /// `get_plate_name()`.
    #[must_use]
    pub fn get_plate_name(&self) -> String {
        self.plate_name.clone()
    }

    /// `set_plate_name(name)`.
    pub fn set_plate_name(&mut self, name: &str) {
        self.plate_name = name.to_string();
    }

    /// `cancel_callback()`.
    pub fn cancel_callback(&self) -> &(dyn Fn() + Send + Sync) {
        &*self.cancel_callback
    }

    /// `call_cancel_callback()`.
    pub fn call_cancel_callback(&self) {
        (self.cancel_callback)();
    }

    /// `throw_if_canceled()`: panics with a [`CanceledException`] payload when a cancel was
    /// asked for, which a CLI run never does.
    pub fn throw_if_canceled(&self) {
        if self.cancel_status.load(Ordering::Acquire) != 0 {
            std::panic::panic_any(CanceledException);
        }
    }
}

/// `SoftFever_VERSION`: the OrcaSlicer version the engine carries (version.inc).
pub const SOFTFEVER_VERSION: &str = "2.4.2";

/// `Slic3r::PlaceholderParserError`: a template that failed, with its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceholderParserError(pub String);

impl PlaceholderParserError {
    /// `what()`.
    #[must_use]
    pub fn what(&self) -> String {
        self.0.clone()
    }
}

impl core::fmt::Display for PlaceholderParserError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlaceholderParserError {}

/// `boost::lexical_cast<std::string>(double)`: `inf`, `-inf`, `nan` or `-nan`, else
/// `sprintf("%.*g", 17, v)` (Boost's precision for a double, `max_digits10`).
fn lexical_cast_double(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan" } else { "nan" }.to_string();
    }
    if v.is_infinite() {
        return if v.is_sign_negative() { "-inf" } else { "inf" }.to_string();
    }
    // %.17g of a finite double always formats, in ASCII.
    sprintf_string("%.*g", &[Arg::Int(17), Arg::Double(v)]).unwrap_or_default()
}

/// `PlaceholderParser::update_timestamp(config)`: the local time now as `timestamp`
/// (`YYYYMMDD-hhmmss`) and as `year`, `month`, `day`, `hour`, `minute` and `second`. Belongs in
/// orca-placeholder (PlaceholderParser.cpp 78-101).
pub fn update_timestamp(config: &mut Config) {
    let timeinfo = orca_model::utils::local_time_now();
    {
        let ss = format!(
            "{}{:02}{:02}-{:02}{:02}{:02}",
            1900 + timeinfo.tm_year,
            1 + timeinfo.tm_mon,
            timeinfo.tm_mday,
            timeinfo.tm_hour,
            timeinfo.tm_min,
            timeinfo.tm_sec
        );
        config.insert("timestamp", OptionValue::String(ss));
    }
    config.insert("year", OptionValue::Int(1900 + timeinfo.tm_year));
    config.insert("month", OptionValue::Int(1 + timeinfo.tm_mon));
    config.insert("day", OptionValue::Int(timeinfo.tm_mday));
    config.insert("hour", OptionValue::Int(timeinfo.tm_hour));
    config.insert("minute", OptionValue::Int(timeinfo.tm_min));
    config.insert("second", OptionValue::Int(timeinfo.tm_sec));
}

/// `PlaceholderParser::update_user_name(config)`: `user` from the `USER` variable, else
/// `USERNAME`, else "unknown". Belongs in orca-placeholder (PlaceholderParser.cpp 103-107).
pub fn update_user_name(config: &mut Config) {
    let user = std::env::var_os("USER")
        .or_else(|| std::env::var_os("USERNAME"))
        .map_or_else(
            || "unknown".to_string(),
            |u| u.to_string_lossy().into_owned(),
        );
    config.insert("user", OptionValue::String(user));
}

/// `PrintTryCancel`: the print's `throw_if_canceled`, to pass to code that may not call it.
#[derive(Clone, Copy, Debug)]
pub struct PrintTryCancel<'a> {
    /// `m_print`.
    print: &'a PrintBase,
}

impl PrintTryCancel<'_> {
    /// `operator()()`: `m_print->throw_if_canceled()`.
    pub fn call(&self) {
        self.print.throw_if_canceled();
    }
}

/// The name MSVC's `__FUNCTION__` gives both `PrintBase::status_update_warnings`.
const STATUS_UPDATE_WARNINGS: &str = "Slic3r::PrintBase::status_update_warnings";

impl PrintBase {
    /// `make_try_cancel()`.
    #[must_use]
    pub fn make_try_cancel(&self) -> PrintTryCancel<'_> {
        PrintTryCancel { print: self }
    }

    /// `update_object_placeholders(config, default_ext)`: `num_objects` and `num_instances` (the
    /// model objects and instances that print), `scale` (each printing object's last printable
    /// instance's scale, in percent), `first_object_name`, and, when an object names an input,
    /// `input_filename` (the first printing object's name, or its input file when it has none,
    /// without its folders and last extension, with `default_ext`) and `input_filename_base`.
    pub fn update_object_placeholders(&self, config: &mut Config, default_ext: &str) {
        // The first input file name.
        let mut input_file = String::new();
        let mut first_object_name = String::new();
        let mut v_scale: Vec<String> = Vec::new();
        let mut num_objects: i32 = 0;
        let mut num_instances: i32 = 0;
        for model_object in &self.model.objects {
            let mut printable: Option<&orca_model::model::ModelInstance> = None;
            for model_instance in &model_object.instances {
                if model_instance.is_printable(model_object.printable) {
                    printable = Some(model_instance);
                    num_instances += 1;
                }
            }
            if let Some(printable) = printable {
                num_objects += 1;
                if num_objects == 1 {
                    first_object_name = model_object.name.clone();
                }
                // CHECK_ME -> Is the following correct ?
                let sf = printable.get_scaling_factor();
                v_scale.push(
                    "x:".to_string()
                        + &lexical_cast_double(sf[0] * 100.)
                        + "% y:"
                        + &lexical_cast_double(sf[1] * 100.)
                        + "% z:"
                        + &lexical_cast_double(sf[2] * 100.)
                        + "%",
                );
                if input_file.is_empty() {
                    input_file = if model_object.name.is_empty() {
                        model_object.input_file.clone()
                    } else {
                        model_object.name.clone()
                    };
                }
            }
        }

        config.insert("num_objects", OptionValue::Int(num_objects));
        config.insert("num_instances", OptionValue::Int(num_instances));

        config.insert("scale", OptionValue::Strings(v_scale));
        config.insert("first_object_name", OptionValue::String(first_object_name));
        if !input_file.is_empty() {
            // The base name with and without its suffix.
            let input_filename = fs_path::filename(&input_file).to_string();
            let input_filename_base = match input_filename.rfind('.') {
                Some(pos) => input_filename[..pos].to_string(),
                None => input_filename.clone(),
            };
            config.insert(
                "input_filename",
                OptionValue::String(input_filename_base.clone() + default_ext),
            );
            config.insert(
                "input_filename_base",
                OptionValue::String(input_filename_base),
            );
        }
    }

    /// `output_filename(format, default_ext, filename_base, config_override)`: the file name the
    /// format gives over the override's options, the version, the time, the user and the object
    /// placeholders (`filename_base`, when given, replaces the input file name); with no format,
    /// the input file name's base and `default_ext`. A name without an extension takes
    /// `default_ext`.
    ///
    /// # Errors
    /// "Failed processing of the filename_format template.\n" and the parser's text, for a format
    /// the placeholder parser refuses.
    ///
    /// # Panics
    /// With `NotPorted` for a format that is not empty: `PlaceholderParser::process` (the macro
    /// language) is orca-gcode's, phase O4; and for no format when no object names an input and
    /// no `filename_base` is given (the C++ dereferences the missing `input_filename_base`).
    pub fn output_filename(
        &self,
        format: &str,
        default_ext: &str,
        filename_base: &str,
        config_override: Option<&Config>,
    ) -> Result<String, PlaceholderParserError> {
        let mut cfg = Config::default();
        if let Some(config_override) = config_override {
            cfg = config_override.clone();
        }
        cfg.insert(
            "version",
            OptionValue::String(SOFTFEVER_VERSION.to_string()),
        );
        update_timestamp(&mut cfg);
        update_user_name(&mut cfg);
        self.update_object_placeholders(&mut cfg, default_ext);
        if !filename_base.is_empty() {
            cfg.insert(
                "input_filename",
                OptionValue::String(filename_base.to_string() + default_ext),
            );
            cfg.insert(
                "input_filename_base",
                OptionValue::String(filename_base.to_string()),
            );
        }
        let mut filename = if format.is_empty() {
            match cfg.get("input_filename_base") {
                Some(OptionValue::String(base)) => base.clone() + default_ext,
                _ => not_ported(
                    "PrintBase::output_filename",
                    "gcode-export: no input_filename_base for an empty format (the C++ \
                     dereferences a null option)",
                ),
            }
        } else {
            not_ported(
                "PlaceholderParser::process",
                "gcode-export: the filename_format template (the macro language is orca-gcode's, \
                 phase O4)",
            )
        };
        if fs_path::extension(&filename).is_empty() {
            fs_path::replace_extension(&mut filename, default_ext);
        }
        Ok(filename)
    }

    /// `output_filepath(path, filename_base)`: `path` itself when it names a file; in a folder,
    /// the print's file name there. `output_filename` is the print's virtual
    /// `output_filename(filename_base)` (Print.cpp's, which passes its `filename_format`).
    ///
    /// # Errors
    /// What `output_filename` gives.
    ///
    /// # Panics
    /// With `NotPorted` for an empty `path`: the name the model proposes
    /// (`Model::propose_export_file_name_and_path`) is not translated, and a CLI run always
    /// passes a path.
    pub fn output_filepath(
        &self,
        path: &str,
        filename_base: &str,
        output_filename: impl FnOnce(&str) -> Result<String, PlaceholderParserError>,
    ) -> Result<String, PlaceholderParserError> {
        // No path: one from the first object's input file.
        if path.is_empty() {
            not_ported(
                "Model::propose_export_file_name_and_path",
                "gcode-export: an empty output path (a CLI run always passes one)",
            );
        }

        // A folder: the file name generated in it.
        if std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
            return Ok(fs_path::make_preferred(&fs_path::append(
                path,
                &output_filename(filename_base)?,
            )));
        }

        // A file that is not a folder: the path itself.
        Ok(path.to_string())
    }

    /// `set_status(percent, message, flags, warning_step)`: to the status callback, or else a
    /// debug record "Percent <percent>: <message>".
    pub fn set_status(&self, percent: i32, message: &str, flags: u32, warning_step: i32) {
        if let Some(cb) = &self.status_callback {
            cb(&SlicingStatus::new(
                percent,
                message,
                flags,
                warning_step,
                SlicingNotificationType::SlicingDefaultNotification,
                WarningLevel::NonCritical,
            ));
        } else {
            log(Severity::Debug, &format!("Percent {percent}: {message}\n"));
        }
    }

    /// `status_update_warnings(step, warning_level, message, print_object, message_id)`: a step's
    /// warning to the status callback (the object's when one is given, else the print's), or else
    /// an info record of a message that is not empty.
    pub fn status_update_warnings(
        &self,
        step: i32,
        warning_level: WarningLevel,
        message: &str,
        print_object: Option<&PrintObjectBase>,
        message_id: SlicingNotificationType,
    ) {
        if let Some(cb) = &self.status_callback {
            let status = match print_object {
                Some(print_object) => SlicingStatus::for_print_object(
                    print_object,
                    step,
                    message,
                    message_id,
                    warning_level,
                ),
                None => SlicingStatus::for_print(self, step, message, message_id, warning_level),
            };
            cb(&status);
        } else if !message.is_empty() {
            log(
                Severity::Info,
                &format!("{STATUS_UPDATE_WARNINGS}, Print warning: {message}\n"),
            );
        }
    }

    /// `status_update_warnings(step, warning_level, message, object, message_id)` (BBS): an
    /// object step's warning to the status callback, or else an info record of a message that is
    /// not empty.
    pub fn status_update_warnings_of_object(
        &self,
        step: i32,
        warning_level: WarningLevel,
        message: &str,
        object: &PrintObjectBase,
        message_id: SlicingNotificationType,
    ) {
        if let Some(cb) = &self.status_callback {
            cb(&SlicingStatus::for_print_object(
                object,
                step,
                message,
                message_id,
                warning_level,
            ));
        } else if !message.is_empty() {
            log(
                Severity::Info,
                &format!("{STATUS_UPDATE_WARNINGS}, PrintObject warning: {message}\n"),
            );
        }
    }
}

impl PrintObjectBase {
    /// `status_update_warnings(print, step, warning_level, message, message_id)`: the print's
    /// [`PrintBase::status_update_warnings`] with this object.
    pub fn status_update_warnings(
        &self,
        print: &PrintBase,
        step: i32,
        warning_level: WarningLevel,
        message: &str,
        message_id: SlicingNotificationType,
    ) {
        print.status_update_warnings(step, warning_level, message, Some(self), message_id);
    }
}

/// `static_cast<int>(step)`.
fn step_int<S: StepType>(step: S) -> i32 {
    i32::try_from(step.index()).unwrap_or(i32::MAX)
}

/// `PrintBaseWithState<PrintStepEnum, COUNT>`: a print base with the state of its steps.
#[derive(Debug)]
pub struct PrintBaseWithState<S: StepType, const COUNT: usize> {
    /// The `PrintBase` it derives from.
    pub base: PrintBase,
    /// `m_state`.
    pub state: PrintState<S, COUNT>,
}

impl<S: StepType, const COUNT: usize> Default for PrintBaseWithState<S, COUNT> {
    /// `PrintBase()` and every step invalid.
    fn default() -> Self {
        Self {
            base: PrintBase::new(),
            state: PrintState::default(),
        }
    }
}

impl<S: StepType, const COUNT: usize> core::ops::Deref for PrintBaseWithState<S, COUNT> {
    type Target = PrintBase;
    fn deref(&self) -> &PrintBase {
        &self.base
    }
}

impl<S: StepType, const COUNT: usize> core::ops::DerefMut for PrintBaseWithState<S, COUNT> {
    fn deref_mut(&mut self) -> &mut PrintBase {
        &mut self.base
    }
}

impl<S: StepType, const COUNT: usize> PrintBaseWithState<S, COUNT> {
    /// `is_step_done(step)`.
    #[must_use]
    pub fn is_step_done(&self, step: S) -> bool {
        self.state.is_done(step)
    }

    /// `step_state_with_timestamp(step)`.
    #[must_use]
    pub fn step_state_with_timestamp(&self, step: S) -> StateWithTimeStamp {
        self.state.state_with_timestamp(step)
    }

    /// `step_state_with_warnings(step)`.
    #[must_use]
    pub fn step_state_with_warnings(&self, step: S) -> StateWithWarnings {
        self.state.state_with_warnings(step)
    }

    /// `set_started(step)`: with the print's `throw_if_canceled`.
    pub fn set_started(&mut self, step: S) -> bool {
        let base = &self.base;
        self.state.set_started(step, || base.throw_if_canceled())
    }

    /// `set_done(step)`: with the print's `throw_if_canceled`; when old warnings went, the UI is
    /// told (`status_update_warnings` with no message, which logs nothing without a callback).
    pub fn set_done(&mut self, step: S) -> TimeStamp {
        let base = &self.base;
        let status = self.state.set_done(step, || base.throw_if_canceled());
        if status.1 {
            self.base.status_update_warnings(
                step_int(step),
                WarningLevel::NonCritical,
                "",
                None,
                SlicingNotificationType::SlicingDefaultNotification,
            );
        }
        status.0
    }

    /// `active_step_add_warning(warning_level, message, message_id)`: the warning added to the
    /// active step; the UI told when it changed. Called any number of times between `set_started`
    /// and `set_done`.
    pub fn active_step_add_warning(
        &mut self,
        warning_level: WarningLevel,
        message: &str,
        message_id: SlicingNotificationType,
    ) {
        let active_step =
            self.state
                .active_step_add_warning(warning_level, message, message_id as i32);
        if active_step.1 {
            // Update UI.
            self.base.status_update_warnings(
                step_int(active_step.0),
                warning_level,
                message,
                None,
                message_id,
            );
        }
    }

    /// `invalidate_step(step)`: with the print's cancel callback.
    pub fn invalidate_step(&mut self, step: S) -> bool {
        let base = &self.base;
        self.state.invalidate(step, || base.call_cancel_callback())
    }

    /// `invalidate_steps(step_begin, step_end)` and `invalidate_steps({steps})`.
    pub fn invalidate_steps(&mut self, steps: &[S]) -> bool {
        let base = &self.base;
        self.state
            .invalidate_multiple(steps, || base.call_cancel_callback())
    }

    /// `invalidate_all_steps()`.
    pub fn invalidate_all_steps(&mut self) -> bool {
        let base = &self.base;
        self.state.invalidate_all(|| base.call_cancel_callback())
    }

    /// `is_step_started_unguarded(step)`.
    #[must_use]
    pub fn is_step_started_unguarded(&self, step: S) -> bool {
        self.state.is_started_unguarded(step)
    }

    /// `is_step_done_unguarded(step)`.
    #[must_use]
    pub fn is_step_done_unguarded(&self, step: S) -> bool {
        self.state.is_done_unguarded(step)
    }
}

/// `PrintObjectBaseWithState<PrintType, PrintObjectStepEnum, COUNT>`: an object base with the
/// state of its steps. The print (`m_print`) is an argument of the calls that need it.
#[derive(Clone, Debug)]
pub struct PrintObjectBaseWithState<S: StepType, const COUNT: usize> {
    /// The `PrintObjectBase` it derives from.
    pub base: PrintObjectBase,
    /// `m_state`.
    pub state: PrintState<S, COUNT>,
}

impl<S: StepType, const COUNT: usize> PrintObjectBaseWithState<S, COUNT> {
    /// `PrintObjectBaseWithState(print, model_object)`.
    #[must_use]
    pub fn new(model_object: usize) -> Self {
        Self {
            base: PrintObjectBase::new(model_object),
            state: PrintState::default(),
        }
    }

    /// `is_step_done(step)`.
    #[must_use]
    pub fn is_step_done(&self, step: S) -> bool {
        self.state.is_done(step)
    }

    /// `step_state_with_timestamp(step)`.
    #[must_use]
    pub fn step_state_with_timestamp(&self, step: S) -> StateWithTimeStamp {
        self.state.state_with_timestamp(step)
    }

    /// `step_state_with_warnings(step)`.
    #[must_use]
    pub fn step_state_with_warnings(&self, step: S) -> StateWithWarnings {
        self.state.state_with_warnings(step)
    }

    /// `set_started(step)`: a cancel of the print throws first.
    pub fn set_started(&mut self, step: S, print: &PrintBase) -> bool {
        self.state
            .set_started(step, || Self::throw_if_canceled(print))
    }

    /// `set_done(step)`: a cancel of the print throws first; when old warnings went, the print's
    /// UI is told, with this object.
    pub fn set_done(&mut self, step: S, print: &PrintBase) -> TimeStamp {
        let status = self.state.set_done(step, || Self::throw_if_canceled(print));
        if status.1 {
            self.base.status_update_warnings(
                print,
                step_int(step),
                WarningLevel::NonCritical,
                "",
                SlicingNotificationType::SlicingDefaultNotification,
            );
        }
        status.0
    }

    /// `active_step_add_warning(warning_level, message, message_id)`: the warning added to the
    /// active step; the print's UI told, with this object, when it changed.
    pub fn active_step_add_warning(
        &mut self,
        warning_level: WarningLevel,
        message: &str,
        message_id: SlicingNotificationType,
        print: &PrintBase,
    ) {
        let active_step =
            self.state
                .active_step_add_warning(warning_level, message, message_id as i32);
        if active_step.1 {
            self.base.status_update_warnings(
                print,
                step_int(active_step.0),
                warning_level,
                message,
                message_id,
            );
        }
    }

    /// `invalidate_step(step)`: with the print's cancel callback.
    pub fn invalidate_step(&mut self, step: S, print: &PrintBase) -> bool {
        self.state.invalidate(step, || print.call_cancel_callback())
    }

    /// `invalidate_steps(step_begin, step_end)` and `invalidate_steps({steps})`.
    pub fn invalidate_steps(&mut self, steps: &[S], print: &PrintBase) -> bool {
        self.state
            .invalidate_multiple(steps, || print.call_cancel_callback())
    }

    /// `invalidate_all_steps()`.
    pub fn invalidate_all_steps(&mut self, print: &PrintBase) -> bool {
        self.state.invalidate_all(|| print.call_cancel_callback())
    }

    /// `invalidate_all_steps_without_cancel()`.
    pub fn invalidate_all_steps_without_cancel(&mut self) -> bool {
        self.state.invalidate_all(|| {})
    }

    /// `is_step_started_unguarded(step)`.
    #[must_use]
    pub fn is_step_started_unguarded(&self, step: S) -> bool {
        self.state.is_started_unguarded(step)
    }

    /// `is_step_done_unguarded(step)`.
    #[must_use]
    pub fn is_step_done_unguarded(&self, step: S) -> bool {
        self.state.is_done_unguarded(step)
    }

    /// `throw_if_canceled()`: `if (m_print->canceled()) throw CanceledException();`.
    pub fn throw_if_canceled(print: &PrintBase) {
        if print.canceled() {
            std::panic::panic_any(CanceledException);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        A,
        B,
    }

    impl StepType for Step {
        fn index(self) -> usize {
            self as usize
        }
        fn from_index(index: usize) -> Self {
            if index == 0 { Step::A } else { Step::B }
        }
    }

    #[test]
    fn a_step_starts_finishes_and_drops_old_warnings() {
        let mut s: PrintState<Step, 2> = PrintState::default();
        assert!(s.set_started(Step::A, || {}));
        assert_eq!(
            s.active_step_add_warning(WarningLevel::NonCritical, "w", 0),
            (Step::A, true)
        );
        assert_eq!(
            s.active_step_add_warning(WarningLevel::NonCritical, "w", 0),
            (Step::A, false)
        );
        let (_, update) = s.set_done(Step::A, || {});
        assert!(!update);
        assert!(s.is_done(Step::A));
        assert!(!s.set_started(Step::A, || {}));
        assert!(s.invalidate(Step::A, || {}));
        assert!(
            s.state_with_warnings(Step::A)
                .warnings
                .iter()
                .all(|w| !w.current)
        );
        assert!(s.set_started(Step::A, || {}));
        let (_, update) = s.set_done(Step::A, || {});
        assert!(update);
        assert!(s.state_with_warnings(Step::A).warnings.is_empty());
        assert!(!s.invalidate_multiple(&[Step::B], || panic!("nothing to cancel")));
    }

    #[test]
    fn a_cli_print_is_never_canceled() {
        let p = PrintBase::new();
        assert_eq!(p.cancel_status(), CancelStatus::NotCanceled);
        p.throw_if_canceled();
        p.cancel_internal();
        assert!(p.canceled());
        let caught =
            std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| p.throw_if_canceled()));
        assert!(caught.is_err_and(|e| e.downcast_ref::<CanceledException>().is_some()));
        p.restart();
        assert!(!p.canceled());
    }

    #[test]
    fn placeholders_and_file_names() {
        use orca_eigen::Vec3d;
        use orca_model::model::{ModelInstance, ModelObject};
        let mut p = PrintBase::new();
        for (name, input, scale, printable) in [
            ("", "C:/models/part.v2.stl", 1.5, true),
            ("second", "x.3mf", 1., false),
        ] {
            let mut mo = ModelObject::default();
            mo.name = name.to_string();
            mo.input_file = input.to_string();
            let mut mi = ModelInstance::default();
            mi.set_scaling_factor(&Vec3d::new(scale, 1., 0.25));
            mi.printable = printable;
            mo.instances.push(mi);
            p.model.objects.push(mo);
        }
        let mut cfg = Config::default();
        p.update_object_placeholders(&mut cfg, ".gcode");
        assert_eq!(cfg.int("num_objects"), Some(1));
        assert_eq!(cfg.int("num_instances"), Some(1));
        assert_eq!(
            cfg.strings("scale").cloned(),
            Some(vec!["x:150% y:100% z:25%".to_string()])
        );
        assert_eq!(cfg.string("first_object_name"), "");
        assert_eq!(cfg.string("input_filename"), "part.v2.gcode");
        assert_eq!(cfg.string("input_filename_base"), "part.v2");
        assert_eq!(
            p.output_filename("", ".gcode", "", None),
            Ok("part.v2.gcode".to_string())
        );
        assert_eq!(
            p.output_filename("", ".gcode", "b.x", None),
            Ok("b.x.gcode".to_string())
        );
        let caught = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
            p.output_filename("{input_filename_base}", ".gcode", "", None)
        }));
        assert!(caught.is_err());
        assert_eq!(lexical_cast_double(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(lexical_cast_double(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn steps_tell_the_ui_of_their_warnings() {
        use std::sync::{Arc, Mutex};
        let seen: Arc<Mutex<Vec<SlicingStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let mut p: PrintBaseWithState<Step, 2> = PrintBaseWithState::default();
        let sink = Arc::clone(&seen);
        p.set_status_callback(Box::new(move |s| sink.lock().unwrap().push(s.clone())));
        assert!(p.set_started(Step::B));
        p.active_step_add_warning(
            WarningLevel::Critical,
            "w",
            SlicingNotificationType::SlicingDefaultNotification,
        );
        p.active_step_add_warning(
            WarningLevel::Critical,
            "w",
            SlicingNotificationType::SlicingDefaultNotification,
        );
        p.set_done(Step::B);
        assert!(p.invalidate_step(Step::B));
        assert!(p.set_started(Step::B));
        p.set_done(Step::B);
        let seen = seen.lock().unwrap();
        // The new warning, then the old one dropped by the second set_done; the repeat and the
        // first set_done change nothing the UI shows.
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].flags, SLICING_STATUS_UPDATE_PRINT_STEP_WARNINGS);
        assert_eq!(seen[0].warning_step, 1);
        assert_eq!(seen[0].text, "w");
        assert_eq!(seen[0].warning_level, WarningLevel::Critical);
        assert_eq!(seen[1].text, "");
        assert_eq!(seen[1].warning_level, WarningLevel::NonCritical);
    }
}
