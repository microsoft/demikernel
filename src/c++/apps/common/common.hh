// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef ECHO_COMMON_H_
#define ECHO_COMMON_H_

#include <boost/optional.hpp>
#include <boost/program_options.hpp>
#include <iostream>
#include <fstream>
#include <sstream>
#include <chrono>
#include <iomanip>
#include <dmtr/latency.h>
#include <dmtr/time.hh>
#include <dmtr/annot.h>
#include <dmtr/mem.h>
#include <dmtr/libos/persephone.hh>
#include <dmtr/libos.h>
#include <string.h>
#include <stdio.h>
#include <pthread.h>
#include <yaml-cpp/yaml.h>

/*****************************************************************
 *********************** LOGGING MACROS   ************************
 *****************************************************************/
static const auto start_time = take_time();

/* Enable profiling */
#define TRACE
//#define DMTR_PROFILE
//#define OP_DEBUG
//#define LEGACY_PROFILING

/* Enable debug statements  */
//#define LOG_DEBUG

/* Where command-line output gets printed to  */
#define LOG_FD stderr

/* For coloring log output  */
#define ANSI_COLOR_RED     "\x1b[31m"
#define ANSI_COLOR_GREEN   "\x1b[32m"
#define ANSI_COLOR_YELLOW  "\x1b[33m"
#define ANSI_COLOR_RESET   "\x1b[0m"
#define ANSI_COLOR_PURPLE   "\x1b[35m"

/* General logging function which can be filled in with arguments, color, etc. */
#define log_at_level(lvl_label, color, fd, fmt, ...)\
        fprintf(fd, "" color "%07.03f:%s:%d:%s(): " lvl_label ": " fmt ANSI_COLOR_RESET "\n", \
                ((boost::chrono::duration<double>)(hr_clock::now() - start_time)).count(), \
                __FILE__, __LINE__, __func__, ##__VA_ARGS__)

/* Debug statements are replaced with nothing if LOG_DEBUG is false  */
#ifdef LOG_DEBUG
#define log_debug(fmt, ...)\
    log_at_level("DEBUG", ANSI_COLOR_RESET, LOG_FD, fmt, ##__VA_ARGS__)
#else
#define log_debug(...)
#endif

#define log_info(fmt, ...)\
    log_at_level("INFO", ANSI_COLOR_GREEN, LOG_FD, fmt, ##__VA_ARGS__)
#define log_error(fmt, ...)\
    log_at_level("ERROR", ANSI_COLOR_RED, LOG_FD, fmt, ##__VA_ARGS__)
#define log_warn(fmt, ...)\
    log_at_level("WARN", ANSI_COLOR_YELLOW, LOG_FD, fmt, ##__VA_ARGS__)

#ifdef PRINT_REQUEST_ERRORS
#define print_request_error(fmt, ...)\
    log_warn(fmt, ##__VA_ARGS__);
#else
#define print_request_error(...)
#endif

/**
 * Simple macro to replace perror with out log format
 */
#define log_perror(fmt, ...) \
    log_error(fmt ": %s", ##__VA_ARGS__, strerror(errno))

/**
 * Same as above, but to be used only for request-based errors
 */
#define perror_request(fmt, ...) \
    print_request_error(fmt ": %s", ##__VA_ARGS__, strerror(errno))

/* Some c++ macro */
#define LOG(lbl, color, ...)\
    std::cerr << color \
              << std::fixed << std::setw(7) << std::setprecision(3) << std::setfill('0') \
              << boost::chrono::duration_cast<boost::chrono::nanoseconds>(take_time()-start_time).count() \
              << ":" __FILE__ ":" << __LINE__ << ":" << __func__ << ":" \
              << lbl << ": " << __VA_ARGS__ \
              << ANSI_COLOR_RESET << "\n";

#ifdef LOG_DEBUG
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

#define MAX_FILE_PATH_LEN 128

struct log_data {
    dmtr_latency_t *l;
    char const *name;
    FILE *fh;
    char filename[MAX_FILE_PATH_LEN];
};

inline int dump_logs(std::vector<struct log_data> &logs, std::string label) {
    for (auto &log: logs) {
        DMTR_OK(dmtr_dump_latency_to_file(reinterpret_cast<const char *>(log.filename), log.l));
        DMTR_OK(dmtr_delete_latency(&log.l));
    }
    return 0;
}

static std::string generate_log_file_path(std::string exp_label,
                                          char const *log_label) {
    char pathname[MAX_FILE_PATH_LEN];
    snprintf(pathname, MAX_FILE_PATH_LEN, "%s/%s_%s",
             log_dir.c_str(), exp_label.c_str(), log_label);
    std::string str_pathname(pathname);
    return pathname;
}

#define PQL_RESA 1000000
struct poll_q_len {
    std::vector<hr_clock::time_point> times;
    std::vector<size_t> n_tokens;

    poll_q_len() {
        times.reserve(PQL_RESA);
        n_tokens.reserve(PQL_RESA);
    }
};

inline void update_pql(size_t n_tokens, struct poll_q_len *s) {
    hr_clock::time_point t = take_time();
    s->times.push_back(t);
    s->n_tokens.push_back(n_tokens);
}

inline void dump_pql(struct poll_q_len *s, std::string label) {
    char filename[MAX_FILE_PATH_LEN];
    strncpy(filename, generate_log_file_path(label, "pql").c_str(), MAX_FILE_PATH_LEN);
    FILE *f = fopen(filename, "w");
    fprintf(f, "TIME\tVALUE\n");
    size_t n_points = s->n_tokens.size();
    for (unsigned int i = 0; i < n_points; ++i) {
        fprintf(f, "%lu\t%lu\n", since_epoch(s->times[i]), s->n_tokens[i]);
    }
    fclose(f);
}

#define MAX_RID_FIELD_LEN 64

/***************************************************************
 ************************* UTILITY ************************
 ***************************************************************/

/**
* Counts how many digits in the given integer
* @param int
* @return int
*/
static inline int how_many_digits(int num) {
    int length = 1;
    while (num /= 10) {
        length++;
    }
    return length;
}

static inline void pin_thread(pthread_t thread, u_int16_t cpu) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(cpu, &cpuset);

    int rtn = pthread_setaffinity_np(thread, sizeof(cpu_set_t), &cpuset);
    if (rtn != 0) {
        fprintf(stderr, "could not pin thread: %s\n", strerror(errno));
    }
}

static inline void read_uris(std::vector<std::string> &requests_str, std::string &uri_list) {
    if (!uri_list.empty()) {
        /* Loop-over URI file to create requests */
        std::ifstream urifile(uri_list.c_str());
        //FIXME: this does not complain when we give a directory, rather than a file
        if (urifile.bad() || !urifile.is_open()) {
            log_error("Failed to open uri list file");
            exit(1);
        }
        std::string uri;
        while (std::getline(urifile, uri)) {
            requests_str.push_back(uri);
        }
    }
}

#endif
