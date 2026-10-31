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
// sketch-orca: a command-line slicer, OrcaSlicer's slicing library without its GUI, that other
// programs run as a separate program. The commands and files are specified in protocol/README.md:
//
//   sketch-orca version
//   sketch-orca config-def --out <file>
//   sketch-orca run --job <file> --out <file> [--progress <file>]
//
// Setup follows the stock command line (OrcaSlicer.cpp, CLI::run and CLI::setup): UTF-8 arguments
// and paths, the logging level, the platform.
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <iostream>
#include <map>
#include <string>
#include <vector>

#include <boost/core/null_deleter.hpp>
#include <boost/log/core.hpp>
#include <boost/log/expressions.hpp>
#include <boost/log/sinks/sync_frontend.hpp>
#include <boost/log/sinks/text_ostream_backend.hpp>
#include <boost/log/trivial.hpp>
#include <boost/make_shared.hpp>
#include <boost/nowide/args.hpp>
#include <boost/nowide/filesystem.hpp>
#include <boost/nowide/iostream.hpp>
#include <boost/shared_ptr.hpp>

#include "libslic3r/Platform.hpp"
#include "libslic3r/Utils.hpp"

#include "common.hpp"
#include "run.hpp"

namespace {

using namespace sketch_orca;

const int EXIT_INVALID_PARAMS = -2;

std::string usage()
{
    std::string text =
        "sketch-orca: a command-line slicer on OrcaSlicer's slicing library.\n"
        "\n"
        "  sketch-orca version\n"
        "      Prints the engine and Orca versions as one line of JSON.\n"
        "  sketch-orca config-def --out <file>\n"
        "      Writes every print, filament and printer setting, with its type, values, default and\n"
        "      limits, and the obsolete keys Orca drops or renames when it reads them, as JSON.\n"
        "  sketch-orca run --job <file> --out <file> [--progress <file>]\n"
        "      Does a job (sketch-orca-job/1: slice, layers or info) and writes its result\n"
        "      (sketch-orca-result/1), also when it fails. With --progress, writes one JSON line\n"
        "      { stage, percent, message } per step.\n"
        "\n"
        "SKETCH_ORCA_LOG_LEVEL sets Orca's log level on stderr (0 fatal ... 5 trace; 1 by default, as in\n"
        "the stock command line). Nothing but the commands' own output goes to stdout. A crash in\n"
        "Orca's code still writes the result, as internal-error.\n"
        "\n"
        "Exit codes: 0 when the result is ok, else the stock Orca command line's code for the failure:\n";
    std::map<int, std::vector<std::string>> by_code;
    for (const auto &e : failure_table())
        if (e.second != 0)
            by_code[e.second].push_back(e.first);
    for (auto it = by_code.rbegin(); it != by_code.rend(); ++it) {
        char code[16];
        std::snprintf(code, sizeof(code), "%6d", it->first);
        text += std::string("  ") + code + "  " + join(it->second, ", ") + "\n";
    }
    text += "A collision printing layer by layer exits with -64 (the stock code) instead of -63.\n";
    text += "\n"
            "sketch-orca is a modified version of OrcaSlicer: its slicing library with a command-line front\n"
            "end (modified 2026-10-30). It is free software under the GNU Affero General Public License,\n"
            "version 3, and comes with NO WARRANTY, to the extent permitted by law. It is a separate\n"
            "program: programs that run it, such as Sketch, talk to it only through this command line and\n"
            "its files. The licences are in the LICENSES folder next to the engine, and its complete source\n"
            "is offered with every release of it (the source bundle).\n";
    return text;
}

int usage_error(const std::string &message, const std::string &out_path = std::string())
{
    boost::nowide::cerr << "sketch-orca: " << message << "\n\n" << usage();
    boost::nowide::cerr.flush();
    if (!out_path.empty())
        write_failure_result(out_path, "invalid-params", message, EXIT_INVALID_PARAMS);
    return EXIT_INVALID_PARAMS;
}

// The value of --name (--name value or --name=value) wherever it is, "" when it is not given. It
// finds --out before the options are checked, so a bad command line still gets its result.
std::string find_option(const std::vector<std::string> &args, size_t first, const std::string &name)
{
    const std::string flag = "--" + name;
    for (size_t i = first; i < args.size(); ++i) {
        if (args[i] == flag)
            return i + 1 < args.size() ? args[i + 1] : std::string();
        if (args[i].compare(0, flag.size() + 1, flag + "=") == 0)
            return args[i].substr(flag.size() + 1);
    }
    return std::string();
}

// --name value or --name=value; every option takes a value.
bool parse_options(const std::vector<std::string> &args, size_t first, const std::vector<std::string> &known, std::map<std::string, std::string> &out,
                   std::string &error)
{
    for (size_t i = first; i < args.size(); ++i) {
        const std::string &a = args[i];
        if (a.size() < 3 || a.compare(0, 2, "--") != 0) {
            error = "unexpected argument " + a;
            return false;
        }
        std::string name = a.substr(2), value;
        const size_t eq  = name.find('=');
        if (eq != std::string::npos) {
            value = name.substr(eq + 1);
            name  = name.substr(0, eq);
        } else {
            if (i + 1 >= args.size()) {
                error = "--" + name + " needs a value";
                return false;
            }
            value = args[++i];
        }
        bool is_known = false;
        for (const std::string &k : known)
            is_known = is_known || k == name;
        if (!is_known) {
            error = "unknown option --" + name;
            return false;
        }
        if (out.count(name)) {
            error = "--" + name + " is given twice";
            return false;
        }
        if (value.empty()) {
            error = "--" + name + " is empty";
            return false;
        }
        out[name] = value;
    }
    return true;
}

// The stock command line's --debug level, 1 (error) by default (PrintConfig.cpp, option "debug").
unsigned int log_level()
{
    const unsigned int stock = 1;
    const char        *env   = std::getenv("SKETCH_ORCA_LOG_LEVEL");
    if (env == nullptr || *env == 0)
        return stock;
    char *end = nullptr;
    const unsigned long v = std::strtoul(env, &end, 10);
    return end != nullptr && *end == 0 && v <= 5 ? unsigned(v) : stock;
}

// Orca logs through Boost.Log. With no sink registered, Boost's default sink prints every record on
// stdout, where version prints its JSON; one sink of ours on stderr replaces it.
void log_to_stderr()
{
    namespace logging = boost::log;
    namespace sinks   = boost::log::sinks;
    namespace expr    = boost::log::expressions;
    auto backend = boost::make_shared<sinks::text_ostream_backend>();
    backend->add_stream(boost::shared_ptr<std::ostream>(&std::clog, boost::null_deleter()));
    backend->auto_flush(true);
    auto sink = boost::make_shared<sinks::synchronous_sink<sinks::text_ostream_backend>>(backend);
    sink->set_formatter(expr::stream << "[" << logging::trivial::severity << "] " << expr::smessage);
    logging::core::get()->add_sink(sink);
}

int command_version()
{
    boost::nowide::cout << version_json().dump(-1, ' ', false, json::error_handler_t::replace) << "\n";
    boost::nowide::cout.flush();
    return 0;
}

int command_config_def(const std::vector<std::string> &args)
{
    std::map<std::string, std::string> opts;
    std::string                        error;
    if (!parse_options(args, 2, {"out"}, opts, error))
        return usage_error(error);
    if (!opts.count("out"))
        return usage_error("config-def needs --out <file>");
    try {
        write_file_atomic(opts["out"], config_def_json().dump(2, ' ', false, json::error_handler_t::replace) + "\n");
    } catch (const std::exception &e) {
        boost::nowide::cerr << "sketch-orca: cannot write " << opts["out"] << ": " << e.what() << "\n";
        return exit_code_for("output-failed");
    }
    return 0;
}

int command_run(const std::vector<std::string> &args)
{
    std::map<std::string, std::string> opts;
    std::string                        error;
    const bool                         parsed = parse_options(args, 2, {"job", "out", "progress"}, opts, error);
    const std::string                  out    = opts.count("out") ? opts["out"] : find_option(args, 2, "out");
    if (!parsed)
        return usage_error(error, out);
    if (out.empty())
        return usage_error("run needs --out <file> for the result");
    if (!opts.count("job"))
        return usage_error("run needs --job <file>", out);
    return run_job(opts["job"], out, opts.count("progress") ? opts["progress"] : std::string());
}

} // namespace

int main(int argc, char **argv)
{
    install_crash_handler();
    // UTF-8 arguments on Windows, from the wide command line.
    boost::nowide::args nowide_args(argc, argv);
    // boost::filesystem paths in UTF-8 (OrcaSlicer.cpp, CLI::run).
    try {
        boost::nowide::nowide_filesystem();
    } catch (const std::runtime_error &e) {
        boost::nowide::cerr << "sketch-orca: boost::nowide::nowide_filesystem failed: " << e.what() << "\n";
        return exit_code_for("environment");
    }
    log_to_stderr();
    Slic3r::set_logging_level(log_level());
    Slic3r::detect_platform();

    std::vector<std::string> args(argv, argv + argc);
    if (args.size() < 2)
        return usage_error("no command");
    const std::string &command = args[1];
    try {
        if (command == "help" || command == "--help" || command == "-h") {
            boost::nowide::cout << usage();
            boost::nowide::cout.flush();
            return 0;
        }
        if (command == "version" || command == "--version") {
            if (args.size() > 2)
                return usage_error("version takes no options");
            return command_version();
        }
        if (command == "config-def")
            return command_config_def(args);
        if (command == "run")
            return command_run(args);
    } catch (const std::exception &e) {
        boost::nowide::cerr << "sketch-orca: " << e.what() << "\n";
        return exit_code_for("internal-error");
    }
    return usage_error("unknown command " + command);
}
