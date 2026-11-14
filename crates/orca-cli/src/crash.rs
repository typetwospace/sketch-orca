// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

//! The crash result, as `cli/common.cpp` keeps it: a crash (an access violation, an abort, and in
//! this program a panic) still writes the run's result, `internal-error` with "the engine crashed
//! (<what>) during '<stage>', in Orca's code", and the process ends with -1.
//!
//! - [`install`] (once, at start): no system crash dialog (`SetErrorMode`, `_set_abort_behavior`),
//!   an unhandled-exception filter, a `SIGABRT` handler, and a panic hook. The filter names the
//!   exception's code and where it happened (`exception 0xC0000005 at <module>+0x<offset>`), the
//!   abort handler `abort`, and the panic hook `panic at <file>:<line>` (the file's name only,
//!   in the characters a module name keeps).
//! - [`arm`] and [`disarm`]: the result's path and its text around the message, published whole
//!   and taken by a handler with one exchange; a handler allocates nothing and writes with the
//!   system's own calls, into `<out>.crash.tmp`, then renamed over `<out>`.
//! - [`note_stage`]: the stage a crash names (the last progress line's), and the smoke test's
//!   trigger: `SKETCH_ORCA_TEST_CRASH=<stage>` crashes with an access violation when the run
//!   reaches that stage, `<stage>:abort` with an abort, as in the C++ program, and `<stage>:panic`
//!   (this program only) with a panic. The access violation is raised with `RaiseException`, so
//!   its place is in the system's KERNELBASE.dll (the crash-location mask of parity/masks.json
//!   covers any place). This program reaches the stages `job`, `presets`, `model`, for a job
//!   that orients its objects `orient`, and for a layers or slice job that arranges its objects
//!   `arrange` (`run.rs`), so a trigger at one of them crashes as the C++ program does there.
//!   [`test_point`] is the trigger alone, for a point that is not a stage: `decline`, just before
//!   sketch-orca-rs writes a decline (`run.rs`), while the result is armed, since it reaches no
//!   stage past the one it declines in; a crash there names that stage (`presets` for a
//!   compatibility condition the parser does not take, `model`, `orient` for a job that orients
//!   its objects, and for a layers or slice job declined once its objects are placed, `arrange`,
//!   or `model` when it does not arrange them).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// `CRASH_STAGES`: the stages a crash can name; the last, "other", for any other.
const CRASH_STAGES: [&str; 11] = [
    "start", "job", "presets", "model", "orient", "arrange", "slice", "gcode", "3mf", "stats",
    "other",
];
const CRASH_STAGE_OTHER: usize = CRASH_STAGES.len() - 1;

static STAGE: AtomicUsize = AtomicUsize::new(0);

/// `struct CrashResult`: the result's text around the message, and its paths as UTF-16 with their
/// terminating zero.
struct CrashResult {
    head: Vec<u8>,
    tail: Vec<u8>,
    out: Vec<u16>,
    tmp: Vec<u16>,
}

static RESULT: AtomicPtr<CrashResult> = AtomicPtr::new(std::ptr::null_mut());

/// `arm_crash_result(out_path, head, tail)`: what a crash writes from now on.
pub fn arm(out_path: &str, head: &str, tail: &str) {
    let wide = |s: &str| -> Vec<u16> {
        let mut v: Vec<u16> = s.encode_utf16().collect();
        v.push(0);
        v
    };
    let r = Box::into_raw(Box::new(CrashResult {
        head: head.as_bytes().to_vec(),
        tail: tail.as_bytes().to_vec(),
        out: wide(out_path),
        tmp: wide(&format!("{out_path}.crash.tmp")),
    }));
    // The one this replaces is no handler's: a handler that took it left null in its place.
    let old = RESULT.swap(r, Ordering::SeqCst);
    if !old.is_null() {
        // SAFETY: every non-null pointer stored here came from Box::into_raw and is taken once.
        drop(unsafe { Box::from_raw(old) });
    }
}

/// `disarm_crash_result()`.
pub fn disarm() {
    let old = RESULT.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !old.is_null() {
        // SAFETY: as in arm.
        drop(unsafe { Box::from_raw(old) });
    }
}

/// The value of `SKETCH_ORCA_TEST_CRASH`, read once; `None` when it is unset or empty.
fn test_crash() -> Option<&'static str> {
    static VALUE: OnceLock<Option<String>> = OnceLock::new();
    VALUE
        .get_or_init(|| {
            std::env::var_os("SKETCH_ORCA_TEST_CRASH")
                .map(|v| v.to_string_lossy().into_owned())
                .filter(|v| !v.is_empty())
        })
        .as_deref()
}

/// `note_crash_stage(stage)`: the stage a crash names from now on, then the test trigger.
pub fn note_stage(stage: &str) {
    let index = CRASH_STAGES[..CRASH_STAGE_OTHER]
        .iter()
        .position(|s| *s == stage)
        .unwrap_or(CRASH_STAGE_OTHER);
    STAGE.store(index, Ordering::SeqCst);
    test_point(stage);
}

/// The test trigger at `point` (see the top of this file).
pub fn test_point(point: &str) {
    let Some(t) = test_crash() else {
        return;
    };
    if t == point {
        raise_access_violation();
    }
    if t.strip_suffix(":abort") == Some(point) {
        sys::abort();
    }
    if t.strip_suffix(":panic") == Some(point) {
        panic!("SKETCH_ORCA_TEST_CRASH={t}");
    }
}

/// A crash on purpose: an access violation raised as the system raises one.
fn raise_access_violation() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::EXCEPTION_ACCESS_VIOLATION;
        use windows_sys::Win32::System::Diagnostics::Debug::RaiseException;
        // SAFETY: no arguments; the exception goes to the unhandled-exception filter.
        unsafe {
            RaiseException(
                EXCEPTION_ACCESS_VIOLATION.cast_unsigned(),
                0,
                0,
                std::ptr::null(),
            )
        };
    }
    #[cfg(not(windows))]
    sys::abort();
}

/// A message built in a fixed buffer, with no allocation (`struct CrashText`).
struct CrashText {
    text: [u8; 384],
    size: usize,
}

impl CrashText {
    const fn new() -> CrashText {
        CrashText {
            text: [0; 384],
            size: 0,
        }
    }

    fn add(&mut self, s: &[u8]) -> &mut CrashText {
        for &c in s {
            if c == 0 || self.size + 1 >= self.text.len() {
                break;
            }
            self.text[self.size] = c;
            self.size += 1;
        }
        self
    }

    /// Hexadecimal, at least `digits` digits.
    fn hex(&mut self, mut v: u64, digits: usize, upper: bool) -> &mut CrashText {
        let mut buf = [0u8; 16];
        let mut n = 0;
        loop {
            let d = (v & 0xf) as u8;
            buf[n] = if d < 10 {
                b'0' + d
            } else if upper {
                b'A' + d - 10
            } else {
                b'a' + d - 10
            };
            n += 1;
            v >>= 4;
            if !((v != 0 || n < digits) && n < 16) {
                break;
            }
        }
        let mut out = [0u8; 16];
        for i in 0..n {
            out[i] = buf[n - 1 - i];
        }
        self.add(&out[..n])
    }

    fn bytes(&self) -> &[u8] {
        &self.text[..self.size]
    }
}

/// `write_crash_result(what)`: the armed result, if any, with the crash's message.
fn write_crash_result(what: &[u8]) {
    let r = RESULT.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if r.is_null() {
        return;
    }
    // SAFETY: taken by this exchange alone; never freed, since the process ends next.
    let r = unsafe { &*r };
    let stage = STAGE.load(Ordering::SeqCst);
    let mut message = CrashText::new();
    message
        .add(b"the engine crashed (")
        .add(what)
        .add(b") during '")
        .add(
            CRASH_STAGES[if stage <= CRASH_STAGE_OTHER {
                stage
            } else {
                CRASH_STAGE_OTHER
            }]
            .as_bytes(),
        )
        .add(b"', in Orca's code");
    sys::write_and_rename(r, message.bytes());
}

/// `crash_exit()`: the process ends with -1, at once.
fn crash_exit() -> ! {
    sys::terminate()
}

/// The panic hook: the armed result with "panic at <file>:<line>", then -1.
fn on_panic(info: &std::panic::PanicHookInfo<'_>) {
    let mut what = CrashText::new();
    what.add(b"panic");
    if let Some(at) = info.location() {
        let file = at.file().as_bytes();
        let start = file
            .iter()
            .rposition(|&c| c == b'/' || c == b'\\')
            .map_or(0, |i| i + 1);
        let mut name = [0u8; 64];
        let mut k = 0;
        for &c in &file[start..] {
            if k + 1 >= name.len() {
                break;
            }
            if c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_' {
                name[k] = c;
                k += 1;
            }
        }
        what.add(b" at ").add(&name[..k]).add(b":");
        let mut line = [0u8; 10];
        let mut n = 0;
        let mut v = at.line();
        loop {
            line[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        line[..n].reverse();
        what.add(&line[..n]);
    }
    write_crash_result(what.bytes());
    crash_exit();
}

/// `install_crash_handler()`.
pub fn install() {
    sys::install();
    std::panic::set_hook(Box::new(on_panic));
}

#[cfg(windows)]
mod sys {
    use super::{CrashResult, CrashText, crash_exit, write_crash_result};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_WRITE, HMODULE, INVALID_HANDLE_VALUE, MAX_PATH,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CREATE_ALWAYS, CreateFileW, DeleteFileW, FILE_ATTRIBUTE_NORMAL, MOVEFILE_REPLACE_EXISTING,
        MoveFileExW, WriteFile,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
        SetErrorMode, SetUnhandledExceptionFilter,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };
    use windows_sys::Win32::System::Threading::{ExitProcess, GetCurrentProcess, TerminateProcess};

    const SIGABRT: i32 = 22;
    const WRITE_ABORT_MSG: u32 = 0x1;
    const CALL_REPORTFAULT: u32 = 0x2;

    // The C runtime's own (the static UCRT, as the C++ engine links it).
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
        fn _set_abort_behavior(flags: u32, mask: u32) -> u32;
        #[link_name = "abort"]
        fn crt_abort() -> !;
    }

    /// The C runtime's `abort()`: `SIGABRT`'s handler runs.
    pub fn abort() -> ! {
        // SAFETY: the C runtime's abort takes nothing and does not return.
        unsafe { crt_abort() }
    }

    /// `TerminateProcess(GetCurrentProcess(), -1)`, then `ExitProcess` should that return.
    pub fn terminate() -> ! {
        // SAFETY: ends this process.
        unsafe {
            TerminateProcess(GetCurrentProcess(), u32::MAX);
            ExitProcess(u32::MAX)
        }
    }

    /// The result's head, the message and its tail into `<out>.crash.tmp`, renamed over `<out>`;
    /// the temporary file removed when that fails.
    pub fn write_and_rename(r: &CrashResult, message: &[u8]) {
        let len = |b: &[u8]| u32::try_from(b.len()).unwrap_or(u32::MAX);
        // SAFETY: the paths are NUL-terminated; the buffers are valid for their lengths.
        unsafe {
            let h = CreateFileW(
                r.tmp.as_ptr(),
                GENERIC_WRITE,
                0,
                std::ptr::null(),
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                return;
            }
            let mut written = 0u32;
            let ok = WriteFile(
                h,
                r.head.as_ptr(),
                len(&r.head),
                &raw mut written,
                std::ptr::null_mut(),
            ) != 0
                && WriteFile(
                    h,
                    message.as_ptr(),
                    len(message),
                    &raw mut written,
                    std::ptr::null_mut(),
                ) != 0
                && WriteFile(
                    h,
                    r.tail.as_ptr(),
                    len(&r.tail),
                    &raw mut written,
                    std::ptr::null_mut(),
                ) != 0;
            CloseHandle(h);
            if !ok || MoveFileExW(r.tmp.as_ptr(), r.out.as_ptr(), MOVEFILE_REPLACE_EXISTING) == 0 {
                DeleteFileW(r.tmp.as_ptr());
            }
        }
    }

    /// `on_unhandled_exception(info)`: "exception 0x<code> at <module>+0x<offset>".
    unsafe extern "system" fn on_unhandled_exception(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system passes valid pointers, or null.
        let record = unsafe { info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) };
        let mut what = CrashText::new();
        what.add(b"exception 0x").hex(
            record.map_or(0, |r| u64::from(r.ExceptionCode.cast_unsigned())),
            8,
            true,
        );
        // Where: the module's file name (not its folder) and the offset in it.
        let mut module: HMODULE = std::ptr::null_mut();
        if let Some(r) = record {
            // SAFETY: the address is the exception's; the module handle is written once.
            let found = unsafe {
                GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    r.ExceptionAddress.cast::<u16>(),
                    &raw mut module,
                )
            } != 0;
            if found {
                let mut path = [0u16; MAX_PATH as usize];
                // SAFETY: the buffer holds MAX_PATH units.
                let len =
                    unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), MAX_PATH) } as usize;
                let len = if len < path.len() { len } else { path.len() };
                let mut start = 0;
                for (i, &c) in path[..len].iter().enumerate() {
                    if c == u16::from(b'\\') || c == u16::from(b'/') {
                        start = i + 1;
                    }
                }
                let mut name = [0u8; 64];
                let mut k = 0;
                for &c in &path[start..len] {
                    if k + 1 >= name.len() {
                        break;
                    }
                    if let Ok(b) = u8::try_from(c)
                        && (b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
                    {
                        name[k] = b;
                        k += 1;
                    }
                }
                let offset = (r.ExceptionAddress as usize).wrapping_sub(module as usize);
                what.add(b" at ")
                    .add(&name[..k])
                    .add(b"+0x")
                    .hex(offset as u64, 1, false);
            }
        }
        write_crash_result(what.bytes());
        crash_exit()
    }

    /// `on_signal(sig)`.
    extern "C" fn on_signal(sig: i32) {
        let what: &[u8] = match sig {
            SIGABRT => b"abort",
            11 => b"access violation",
            8 => b"arithmetic error",
            4 => b"illegal instruction",
            _ => b"signal",
        };
        write_crash_result(what);
        crash_exit();
    }

    pub fn install() {
        // SAFETY: process-wide settings, made once at start.
        unsafe {
            // No error or crash dialog from Windows or the C runtime: the engine runs hidden.
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
            _set_abort_behavior(0, WRITE_ABORT_MSG | CALL_REPORTFAULT);
            SetUnhandledExceptionFilter(Some(on_unhandled_exception));
            signal(SIGABRT, on_signal as extern "C" fn(i32) as usize);
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use super::CrashResult;

    pub fn abort() -> ! {
        std::process::abort()
    }

    pub fn terminate() -> ! {
        std::process::exit(-1)
    }

    pub fn write_and_rename(r: &CrashResult, message: &[u8]) {
        let path = |w: &[u16]| String::from_utf16_lossy(&w[..w.len() - 1]);
        let (tmp, out) = (path(&r.tmp), path(&r.out));
        let text = [r.head.as_slice(), message, r.tail.as_slice()].concat();
        if std::fs::write(&tmp, text).is_err() || std::fs::rename(&tmp, &out).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    pub fn install() {}
}

#[cfg(test)]
mod tests {
    use super::{CRASH_STAGE_OTHER, CRASH_STAGES, CrashText};

    #[test]
    fn hex_is_written_as_the_cpp_writes_it() {
        let mut t = CrashText::new();
        t.add(b"0x")
            .hex(0xC000_0005, 8, true)
            .add(b" +0x")
            .hex(0x7a7e5, 1, false);
        assert_eq!(t.bytes(), b"0xC0000005 +0x7a7e5");
        let mut z = CrashText::new();
        z.hex(0, 1, false);
        assert_eq!(z.bytes(), b"0");
    }

    #[test]
    fn the_stages_are_the_cpp_ones() {
        assert_eq!(CRASH_STAGES[0], "start");
        assert_eq!(CRASH_STAGES[CRASH_STAGE_OTHER], "other");
        assert_eq!(CRASH_STAGES.len(), 11);
    }
}
