// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

#include "common.hpp"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cwchar>
#include <iterator>
#include <sstream>

#include <boost/filesystem.hpp>
#include <boost/nowide/convert.hpp>
#include <boost/nowide/fstream.hpp>

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

namespace sketch_orca {

namespace {

// Name -> exit code of the stock Orca command line for the same meaning (OrcaSlicer.cpp,
// cli_errors in v2.4.2). Several names share a code where our front end tells apart what the stock
// command line does not (a bad job, a bad setting and a bad layer height profile are all -2, invalid
// parameters).
const std::vector<std::pair<std::string, int>> &table()
{
    static const std::vector<std::pair<std::string, int>> t = {
        {"ok", 0},
        {"environment", -1},               // CLI_ENVIRONMENT_ERROR: the resources folder is missing
        {"internal-error", -1},            // an error the front end did not expect
        {"invalid-params", -2},            // CLI_INVALID_PARAMS: the command line itself
        {"job-invalid", -2},               // the job file
        {"setting-invalid", -2},           // an override names no setting or has a bad value
        {"layer-profile-invalid", -2},     // a layer height profile that does not fit the object
        {"custom-gcode-invalid", -2},      // a custom G-code that does not fit the layers or the filaments
        {"file-not-found", -3},            // CLI_FILE_NOTFOUND
        {"preset-invalid", -5},            // CLI_CONFIG_FILE_ERROR
        {"model-unreadable", -6},          // CLI_DATA_FILE_ERROR
        {"not-fdm", -7},                   // CLI_INVALID_PRINTER_TECH
        {"unsupported", -8},               // CLI_UNSUPPORTED_OPERATION
        {"output-failed", -13},            // CLI_EXPORT_3MF_ERROR (and any output we cannot write)
        {"out-of-memory", -14},            // CLI_OUT_OF_MEMORY
        {"process-not-compatible", -17},   // CLI_PROCESS_NOT_COMPATIBLE
        {"config-invalid", -18},           // CLI_INVALID_VALUES_IN_3MF: the merged settings fail Orca's checks
        {"arrange-failed", -21},           // CLI_OBJECT_ARRANGE_FAILED
        {"orient-failed", -22},            // CLI_OBJECT_ORIENT_FAILED
        {"empty-plate", -50},              // CLI_NO_SUITABLE_OBJECTS: nothing left to slice
        {"validate-error", -51},           // CLI_VALIDATE_ERROR
        {"not-printable", -52},            // CLI_OBJECTS_PARTLY_INSIDE (-50 when no object is inside)
        {"filament-bed-mismatch", -61},    // CLI_FILAMENT_NOT_MATCH_BED_TYPE
        {"temperature-difference", -62},   // CLI_FILAMENTS_DIFFERENT_TEMP
        {"collision", -63},                // CLI_OBJECT_COLLISION_IN_SEQ_PRINT (-64 by layer)
        {"slice-failed", -100},            // CLI_SLICING_ERROR
        {"path-conflicts", -101},          // CLI_GCODE_PATH_CONFLICTS
        {"gcode-in-unprintable-area", -102}, // CLI_GCODE_PATH_IN_UNPRINTABLE_AREA
    };
    return t;
}

} // namespace

int exit_code_for(const std::string &code)
{
    for (const auto &e : table())
        if (e.first == code)
            return e.second;
    return -1;
}

std::vector<std::pair<std::string, int>> failure_table() { return table(); }

Failure::Failure(const std::string &code, const std::string &message, int object_index, int exit_code)
    : std::runtime_error(message), code(code), exit_code(exit_code != 0 ? exit_code : exit_code_for(code)), object_index(object_index)
{}

void fail(const std::string &code, const std::string &message, int object_index, int exit_code)
{
    throw Failure(code, message, object_index, exit_code);
}

void Warnings::add(const std::string &code, const std::string &message, int object_index)
{
    std::lock_guard<std::mutex> lock(m_mutex);
    for (const Warning &w : m_items)
        if (w.code == code && w.message == message && w.object_index == object_index)
            return;
    m_items.push_back({code, message, object_index});
}

std::vector<Warning> Warnings::list() const
{
    std::lock_guard<std::mutex> lock(m_mutex);
    return m_items;
}

struct Progress::Impl
{
    boost::nowide::ofstream out;
};

Progress::Progress() = default;
Progress::~Progress() = default;

bool Progress::open(const std::string &path)
{
    try {
        make_parent_directories(path);
        auto impl = std::make_unique<Impl>();
        impl->out.open(path, std::ios::out | std::ios::trunc | std::ios::binary);
        if (!impl->out.is_open())
            return false;
        m_impl = std::move(impl);
        return true;
    } catch (const std::exception &) {
        return false;
    }
}

void Progress::line(const std::string &stage, double percent, const std::string &message)
{
    std::lock_guard<std::mutex> lock(m_mutex);
    note_crash_stage(stage);
    if (!std::isfinite(percent))
        percent = m_last;
    percent = std::max(m_last, std::min(100., std::max(0., percent)));
    m_last  = percent;
    write(stage, percent, message);
}

void Progress::finish(bool ok, const std::string &message)
{
    std::lock_guard<std::mutex> lock(m_mutex);
    if (ok)
        m_last = 100.;
    write(ok ? "done" : "failed", m_last, message);
}

void Progress::write(const std::string &stage, double percent, const std::string &message)
{
    if (!m_impl)
        return;
    json j;
    j["stage"]   = stage;
    j["percent"] = std::round(percent * 10.) / 10.;
    j["message"] = message;
    m_impl->out << j.dump(-1, ' ', false, json::error_handler_t::replace) << "\n";
    m_impl->out.flush();
}

std::string read_file(const std::string &path)
{
    boost::nowide::ifstream in(path, std::ios::in | std::ios::binary);
    if (!in.is_open())
        throw std::runtime_error("cannot open " + path);
    std::ostringstream ss;
    ss << in.rdbuf();
    if (in.bad())
        throw std::runtime_error("cannot read " + path);
    return ss.str();
}

void write_file_atomic(const std::string &path, const std::string &text)
{
    make_parent_directories(path);
    const std::string tmp = path + ".tmp";
    {
        boost::nowide::ofstream out(tmp, std::ios::out | std::ios::trunc | std::ios::binary);
        if (!out.is_open())
            throw std::runtime_error("cannot create " + tmp);
        out << text;
        out.flush();
        if (!out.good())
            throw std::runtime_error("cannot write " + tmp);
    }
    boost::system::error_code ec;
    boost::filesystem::rename(boost::filesystem::path(tmp), boost::filesystem::path(path), ec);
    if (ec) {
        boost::filesystem::remove(boost::filesystem::path(tmp), ec);
        throw std::runtime_error("cannot replace " + path);
    }
}

bool is_file(const std::string &path)
{
    boost::system::error_code ec;
    return boost::filesystem::is_regular_file(boost::filesystem::path(path), ec);
}

bool is_directory(const std::string &path)
{
    boost::system::error_code ec;
    return boost::filesystem::is_directory(boost::filesystem::path(path), ec);
}

void make_directories(const std::string &path)
{
    if (path.empty())
        return;
    boost::system::error_code ec;
    boost::filesystem::create_directories(boost::filesystem::path(path), ec);
    if (ec && !is_directory(path))
        throw std::runtime_error("cannot create the folder " + path + ": " + ec.message());
}

std::string parent_directory(const std::string &path)
{
    return boost::filesystem::path(path).parent_path().string();
}

void make_parent_directories(const std::string &path) { make_directories(parent_directory(path)); }

bool is_absolute_path(const std::string &path)
{
    if (path.empty())
        return false;
    return boost::filesystem::path(path).is_absolute();
}

namespace {

#ifdef _WIN32
// The full path Windows' file functions open for `p`: GetFullPathNameW, the conversion CreateFileW
// makes of a path before it opens it: . and .. resolved, / read as \, a name's single trailing dot
// dropped, and the last name's trailing dots and spaces dropped ("out.gcode." and "out.gcode " open
// out.gcode). A path it does not convert is kept as it is.
std::wstring win32_full_path(const std::wstring &p)
{
    DWORD size = GetFullPathNameW(p.c_str(), 0, nullptr, nullptr);
    for (int tries = 0; size > 0 && tries < 4; ++tries) {
        std::wstring out(size, L'\0');
        const DWORD got = GetFullPathNameW(p.c_str(), size, out.data(), nullptr);
        if (got == 0)
            break;
        if (got < size) {
            out.resize(got);
            return out;
        }
        size = got;
    }
    return p;
}

bool starts_with_ci(const std::wstring &s, const std::wstring &prefix)
{
    return s.size() >= prefix.size() && CompareStringOrdinal(s.c_str(), int(prefix.size()), prefix.c_str(), int(prefix.size()), TRUE) == CSTR_EQUAL;
}

// `p` without the prefixes that hand a path to the system as it is (\\?\ and \??\) or as a device
// path (\\.\): \\?\C:\x is C:\x, and \\?\UNC\server\share is \\server\share. What is left is
// converted as a plain path is, so a name given literally through \\?\ ("out.gcode.") counts as the
// name a plain path would open; it may be another file, which only refuses more.
std::wstring without_device_prefix(std::wstring p)
{
    for (int round = 0; round < 4; ++round) {
        bool stripped = false;
        for (const wchar_t *prefix : {L"\\\\?\\", L"\\\\.\\", L"\\??\\"}) {
            if (!starts_with_ci(p, prefix))
                continue;
            p = p.substr(std::wcslen(prefix));
            if (starts_with_ci(p, L"UNC\\"))
                p = L"\\\\" + p.substr(4);
            stripped = true;
            break;
        }
        if (!stripped)
            break;
        p = win32_full_path(p);
    }
    return p;
}

// `p` without NTFS stream names: a name's ":stream", ":stream:$DATA" and "::$DATA" (the file's own
// data) are the file, and "folder::$INDEX_ALLOCATION" is the folder. The drive's colon stays.
std::wstring without_streams(const std::wstring &p)
{
    std::wstring out;
    out.reserve(p.size());
    size_t start = 0;
    // A drive letter's colon (C:) and a UNC path's server and share are not names with streams.
    if (p.size() >= 2 && p[1] == L':') {
        out.append(p, 0, 2);
        start = 2;
    }
    bool in_stream = false;
    for (size_t i = start; i < p.size(); ++i) {
        const wchar_t c = p[i];
        if (c == L'\\' || c == L'/') {
            in_stream = false;
            out.push_back(L'\\');
        } else if (c == L':') {
            in_stream = true;
        } else if (!in_stream) {
            out.push_back(c);
        }
    }
    return out;
}

// The final path of an existing file or folder, as the file system resolves it (links, junctions,
// short names, letter case): GetFinalPathNameByHandleW, without its \\?\ prefix; "" when it cannot
// be opened.
std::wstring final_path(const std::wstring &p)
{
    const HANDLE h = CreateFileW(p.c_str(), FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr, OPEN_EXISTING,
                                 FILE_FLAG_BACKUP_SEMANTICS, nullptr);
    if (h == INVALID_HANDLE_VALUE)
        return std::wstring();
    std::wstring out;
    DWORD        size = GetFinalPathNameByHandleW(h, nullptr, 0, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
    for (int tries = 0; size > 0 && tries < 4; ++tries) {
        std::wstring buf(size, L'\0');
        const DWORD  got = GetFinalPathNameByHandleW(h, buf.data(), size, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
        if (got == 0)
            break;
        if (got < size) {
            buf.resize(got);
            out = buf;
            break;
        }
        size = got;
    }
    CloseHandle(h);
    return out.empty() ? out : without_device_prefix(out);
}

// What a Windows path names, as Windows opens it, for comparing paths without letter case: the full
// path (win32_full_path) without device prefixes and stream names, and its longest part that exists
// replaced by the file system's own path for it (final_path), so a junction, a symbolic link or a
// short name in it reads as the folder it is.
std::wstring win32_file_key(const std::string &path)
{
    std::wstring p = without_streams(without_device_prefix(win32_full_path(boost::nowide::widen(path))));
    while (p.size() > 3 && p.back() == L'\\')
        p.pop_back();
    // The longest part of p that exists, from p itself up to the root.
    for (size_t end = p.size(); end > 0;) {
        const std::wstring head = p.substr(0, end);
        if (const std::wstring real = final_path(head.size() == 2 && head[1] == L':' ? head + L"\\" : head); !real.empty()) {
            std::wstring key = real;
            if (end < p.size()) {
                if (key.empty() || key.back() != L'\\')
                    key.push_back(L'\\');
                key += p.substr(end + 1);
            }
            return key;
        }
        const size_t slash = head.find_last_of(L'\\');
        if (slash == std::wstring::npos || slash < 2)
            break;
        end = slash;
    }
    return p;
}
#endif

} // namespace

bool same_file(const std::string &a, const std::string &b)
{
    namespace fs = boost::filesystem;
    if (a.empty() || b.empty())
        return false;
#ifdef _WIN32
    // The paths as Windows opens them, compared as NTFS compares names: without letter case, by the
    // same upper-case table as this. Two names of a file that does not exist yet (the G-code a run
    // will write, as "out.gcode" and "out.gcode.") are one file only this way (sketch-orca,
    // 2026-11-01).
    const std::wstring ka = win32_file_key(a), kb = win32_file_key(b);
    if (CompareStringOrdinal(ka.c_str(), int(ka.size()), kb.c_str(), int(kb.size()), TRUE) == CSTR_EQUAL)
        return true;
#else
    const auto normal = [](const fs::path &p) {
        boost::system::error_code ec;
        const fs::path            full = fs::absolute(p);
        const fs::path            real = fs::weakly_canonical(full, ec);
        return (ec ? full : real).lexically_normal();
    };
    if (normal(fs::path(a)) == normal(fs::path(b)))
        return true;
#endif
    // Two existing names of one file that the paths do not show (a hard link): the file system says.
    const fs::path            pa(a), pb(b);
    boost::system::error_code ea, eb, ec;
    if (fs::exists(pa, ea) && fs::exists(pb, eb)) {
        const bool same = fs::equivalent(pa, pb, ec);
        if (!ec)
            return same;
    }
    return false;
}

double round_um(double v)
{
    if (!std::isfinite(v))
        return 0.;
    return std::round(v * 1e6) / 1e6;
}

std::string lower(std::string s)
{
    std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return char(std::tolower(c)); });
    return s;
}

bool ends_with_ci(const std::string &s, const std::string &suffix)
{
    if (s.size() < suffix.size())
        return false;
    return lower(s.substr(s.size() - suffix.size())) == lower(suffix);
}

bool starts_with(const std::string &s, const std::string &prefix) { return s.compare(0, prefix.size(), prefix) == 0; }

std::string join(const std::vector<std::string> &items, const std::string &sep)
{
    std::string out;
    for (size_t i = 0; i < items.size(); ++i) {
        if (i > 0)
            out += sep;
        out += items[i];
    }
    return out;
}

std::string format_number(double v)
{
    if (std::isfinite(v) && std::floor(v) == v && std::abs(v) < 1e15) {
        char buf[32];
        std::snprintf(buf, sizeof(buf), "%.0f", v);
        return buf;
    }
    char buf[64];
    std::snprintf(buf, sizeof(buf), "%.12g", v);
    return buf;
}

// ---------------------------------------------------------------------------------------------
// The crash result

namespace {

// Set up front and never changed once published; the crash handlers only read them, through
// lock-free atomics, and allocate nothing (sketch-orca, 2026-11-01). A handler may run on any thread
// at any moment, also while another thread notes a stage, so it reads no text that is written in
// place: the stage is an index into fixed names, and the result's parts are one object, published
// whole and taken by the handler with an exchange.
const char *const CRASH_STAGES[] = {"start", "job", "presets", "model", "orient", "arrange", "slice", "gcode", "3mf", "stats", "other"};
const int         CRASH_STAGE_OTHER = int(std::size(CRASH_STAGES)) - 1;
std::atomic<int>  g_crash_stage{0};

struct CrashResult
{
    std::string head, tail;
#ifdef _WIN32
    std::wstring out, tmp;
#else
    std::string out, tmp;
#endif
};
std::atomic<CrashResult *> g_crash_result{nullptr};

// A message built in a fixed buffer, with no allocation or formatting library (snprintf is not
// async-signal-safe).
struct CrashText
{
    char   text[384] = {0};
    size_t size = 0;
    CrashText &add(const char *s)
    {
        for (; s != nullptr && *s != 0 && size + 1 < sizeof(text); ++s)
            text[size++] = *s;
        text[size] = 0;
        return *this;
    }
    // Hexadecimal, at least `digits` digits.
    CrashText &hex(unsigned long long v, int digits, bool upper)
    {
        char buf[17];
        int  n = 0;
        do {
            const int d = int(v & 0xf);
            buf[n++]    = char(d < 10 ? '0' + d : (upper ? 'A' : 'a') + d - 10);
            v >>= 4;
        } while ((v != 0 || n < digits) && n < 16);
        char out[17];
        for (int i = 0; i < n; ++i)
            out[i] = buf[n - 1 - i];
        out[n] = 0;
        return add(out);
    }
};

void write_crash_result(const char *what)
{
    const CrashResult *r = g_crash_result.exchange(nullptr);
    if (r == nullptr)
        return;
    const int stage = g_crash_stage.load();
    CrashText message;
    message.add("the engine crashed (").add(what).add(") during '").add(CRASH_STAGES[stage >= 0 && stage <= CRASH_STAGE_OTHER ? stage : CRASH_STAGE_OTHER]).add("', in Orca's code");
#ifdef _WIN32
    HANDLE h = CreateFileW(r->tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE)
        return;
    DWORD      written = 0;
    const bool ok      = WriteFile(h, r->head.data(), DWORD(r->head.size()), &written, nullptr) &&
                    WriteFile(h, message.text, DWORD(message.size), &written, nullptr) &&
                    WriteFile(h, r->tail.data(), DWORD(r->tail.size()), &written, nullptr);
    CloseHandle(h);
    if (!ok || !MoveFileExW(r->tmp.c_str(), r->out.c_str(), MOVEFILE_REPLACE_EXISTING))
        DeleteFileW(r->tmp.c_str());
#else
    const int fd = ::open(r->tmp.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0)
        return;
    const bool ok = ::write(fd, r->head.data(), r->head.size()) == ssize_t(r->head.size()) &&
                    ::write(fd, message.text, message.size) == ssize_t(message.size) &&
                    ::write(fd, r->tail.data(), r->tail.size()) == ssize_t(r->tail.size());
    ::close(fd);
    if (!ok || ::rename(r->tmp.c_str(), r->out.c_str()) != 0)
        ::unlink(r->tmp.c_str());
#endif
}

[[noreturn]] void crash_exit()
{
#ifdef _WIN32
    TerminateProcess(GetCurrentProcess(), UINT(-1));
#endif
    std::_Exit(-1);
}

#ifdef _WIN32
LONG WINAPI on_unhandled_exception(EXCEPTION_POINTERS *info)
{
    const EXCEPTION_RECORD *record = info != nullptr ? info->ExceptionRecord : nullptr;
    CrashText               what;
    what.add("exception 0x").hex(record != nullptr ? (unsigned long long) record->ExceptionCode : 0ull, 8, true);
    // Where: the module's file name (not its folder) and the offset in it, for a crash report.
    HMODULE module = nullptr;
    if (record != nullptr &&
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, (LPCWSTR) record->ExceptionAddress, &module)) {
        wchar_t path[MAX_PATH];
        const DWORD len = GetModuleFileNameW(module, path, MAX_PATH);
        DWORD       start = 0;
        for (DWORD i = 0; i < len; ++i)
            if (path[i] == L'\\' || path[i] == L'/')
                start = i + 1;
        char name[64];
        size_t k = 0;
        for (DWORD i = start; i < len && k + 1 < sizeof(name); ++i)
            if ((path[i] >= L'a' && path[i] <= L'z') || (path[i] >= L'A' && path[i] <= L'Z') || (path[i] >= L'0' && path[i] <= L'9') || path[i] == L'.' || path[i] == L'-' ||
                path[i] == L'_')
                name[k++] = char(path[i]);
        name[k] = 0;
        what.add(" at ").add(name).add("+0x").hex((unsigned long long) ((const char *) record->ExceptionAddress - (const char *) module), 1, false);
    }
    write_crash_result(what.text);
    crash_exit();
}
#endif

void on_signal(int sig)
{
    const char *what = sig == SIGABRT ? "abort" : sig == SIGSEGV ? "access violation" : sig == SIGFPE ? "arithmetic error" : sig == SIGILL ? "illegal instruction" : "signal";
    write_crash_result(what);
    crash_exit();
}

} // namespace

void install_crash_handler()
{
#ifdef _WIN32
    // No error or crash dialog from Windows or the C runtime: the engine runs hidden.
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
#ifdef _MSC_VER
    _set_abort_behavior(0, _WRITE_ABORT_MSG | _CALL_REPORTFAULT);
#endif
    SetUnhandledExceptionFilter(on_unhandled_exception);
    std::signal(SIGABRT, on_signal);
#else
    for (int sig : {SIGSEGV, SIGBUS, SIGFPE, SIGILL, SIGABRT})
        std::signal(sig, on_signal);
#endif
}

void arm_crash_result(const std::string &out_path, const std::string &head, const std::string &tail)
{
    auto *r = new CrashResult;
    r->head = head;
    r->tail = tail;
#ifdef _WIN32
    r->out = boost::nowide::widen(out_path);
    r->tmp = boost::nowide::widen(out_path + ".crash.tmp");
#else
    r->out = out_path;
    r->tmp = out_path + ".crash.tmp";
#endif
    // The one this replaces is no handler's: a handler that took it left nullptr in its place.
    delete g_crash_result.exchange(r);
}

void disarm_crash_result() { delete g_crash_result.exchange(nullptr); }

void note_crash_stage(const std::string &stage)
{
    int index = CRASH_STAGE_OTHER;
    for (int i = 0; i < CRASH_STAGE_OTHER; ++i)
        if (stage == CRASH_STAGES[i])
            index = i;
    g_crash_stage.store(index);
    // A crash on purpose, for the smoke case that proves the crash result: SKETCH_ORCA_TEST_CRASH
    // names a stage ("slice" for an access violation there, "slice:abort" for an abort).
    static const char *const test_crash = std::getenv("SKETCH_ORCA_TEST_CRASH");
    if (test_crash != nullptr && *test_crash != 0) {
        if (stage == test_crash) {
            volatile int *nowhere = nullptr;
            *nowhere              = 1;
        }
        if (stage + ":abort" == test_crash)
            std::abort();
    }
}

} // namespace sketch_orca
