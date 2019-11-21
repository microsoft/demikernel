#ifndef KV_LOGGING_HH_
#define KV_LOGGING_HH_

#include <chrono>
#include <iostream>
#include <iomanip>

namespace dmtr_log {

using log_clock = std::chrono::system_clock;
static const auto log_start = log_clock::now();

static double elapsed_time() {
    return std::chrono::duration<double>(log_clock::now() - log_start).count();
}

}

/* For coloring log output  */
#define ANSI_COLOR_RED     "\x1b[31m"
#define ANSI_COLOR_GREEN   "\x1b[32m"
#define ANSI_COLOR_YELLOW  "\x1b[33m"
#define ANSI_COLOR_RESET   "\x1b[0m"
#define ANSI_COLOR_PURPLE   "\x1b[35m"

#define LOG(lbl, color, ...)\
    std::cerr << color \
              << std::fixed << std::setw(7) << std::setprecision(3) << std::setfill('0') \
              << dmtr_log::elapsed_time() \
              << ":" __FILE__ ":" << __LINE__ << ":" << __func__ << ":" \
              << lbl << ": " << __VA_ARGS__ \
              << ANSI_COLOR_RESET << "\n";

#ifdef DO_DEBUG
#define PSP_DEBUG(...) \
    LOG("DEBUG", ANSI_COLOR_RESET, __VA_ARGS__)
#else
#define PSP_DEBUG(...)
#endif

#define PSP_INFO(...) \
    LOG("INFO", ANSI_COLOR_GREEN, __VA_ARGS__)

#define PSP_ERROR(...) \
    LOG("ERROR", ANSI_COLOR_RED, __VA_ARGS__)

#define PSP_WARN(...) \
    LOG("WARN", ANSI_COLOR_YELLOW, __VA_ARGS__)

#endif
