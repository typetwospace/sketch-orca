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
// sketch-orca: what every part of the front end shares. Named failures with the stock Orca command
// line's exit code for the same meaning, warnings, the progress file, and UTF-8 file helpers.
#pragma once

#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace sketch_orca {

// Key order is kept as written, so the files we write read in a fixed order.
using json = nlohmann::ordered_json;

// A failure the result names. `code` is a kebab-case name; `exit_code` is the stock Orca command
// line's exit code for the same meaning (OrcaSlicer.cpp, cli_errors), which the process exits with.
struct Failure : std::runtime_error
{
    std::string code;
    int         exit_code;
    int         object_index;
    Failure(const std::string &code, const std::string &message, int object_index = -1, int exit_code = 0);
};

// The stock exit code for a failure name (0 for "ok", -1 for a name that is not in the table).
int exit_code_for(const std::string &code);

// Every failure name, with its exit code, in the order of the table (for the help text and tests).
std::vector<std::pair<std::string, int>> failure_table();

[[noreturn]] void fail(const std::string &code, const std::string &message, int object_index = -1, int exit_code = 0);

struct Warning
{
    std::string code;
    std::string message;
    int         object_index = -1;
};

// Collected from several threads (slicing reports warnings from worker threads); identical entries
// are kept once.
class Warnings
{
public:
    void                 add(const std::string &code, const std::string &message, int object_index = -1);
    std::vector<Warning> list() const;

private:
    mutable std::mutex   m_mutex;
    std::vector<Warning> m_items;
};

// The --progress file: one JSON object per line, { "stage", "percent", "message" }, flushed after
// every line. The percent is of the whole run and never goes down.
class Progress
{
public:
    Progress();
    ~Progress();
    // False when the file, or its folder, cannot be created.
    bool open(const std::string &path);
    void line(const std::string &stage, double percent, const std::string &message);
    // The last line: stage "done" at 100 when the run is ok, else stage "failed" at the percent the
    // run reached, with the failure's message.
    void finish(bool ok, const std::string &message);

private:
    void write(const std::string &stage, double percent, const std::string &message);

    struct Impl;
    std::unique_ptr<Impl> m_impl;
    std::mutex            m_mutex;
    double                m_last = 0.;
};

// A crash in Orca's code (an access violation, an abort) ends the process before the result is
// written. install_crash_handler (once, at start) keeps the system's crash dialogs away and, while
// a result is armed, writes it on a crash: arm_crash_result takes the result's path and its text
// around the failure message (`head` ends inside the message string, `tail` starts there), and a
// crash writes head + "the engine crashed (...) during '<stage>'" + tail and exits with -1. The
// stage is the one of the last progress line (also without a progress file).
void install_crash_handler();
void arm_crash_result(const std::string &out_path, const std::string &head, const std::string &tail);
void disarm_crash_result();
void note_crash_stage(const std::string &stage);

// Files, with UTF-8 paths on every platform.
std::string read_file(const std::string &path);
// Writes next to the target and renames over it, so a reader never sees half a file.
void write_file_atomic(const std::string &path, const std::string &text);
bool is_file(const std::string &path);
bool is_directory(const std::string &path);
void make_directories(const std::string &path);
void make_parent_directories(const std::string &path);
std::string parent_directory(const std::string &path);
bool is_absolute_path(const std::string &path);

// Numbers for the result: rounded to a micrometre, so 20.049999999999997 reads 20.05.
double round_um(double v);
std::string lower(std::string s);
bool ends_with_ci(const std::string &s, const std::string &suffix);
bool starts_with(const std::string &s, const std::string &prefix);
std::string join(const std::vector<std::string> &items, const std::string &sep);
// A number as Orca writes it in its settings, without trailing zeros ("0.2", "12").
std::string format_number(double v);

} // namespace sketch_orca
