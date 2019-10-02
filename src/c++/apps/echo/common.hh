// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef ECHO_COMMON_H_
#define ECHO_COMMON_H_

#include <boost/optional.hpp>
#include <boost/program_options.hpp>
#include <iostream>
#include <chrono>
#include <dmtr/latency.h>
#include <dmtr/time.hh>
#include <dmtr/annot.h>
#include <dmtr/libos/mem.h>
#include <string.h>
#include <stdio.h>
#include <yaml-cpp/yaml.h>

/*****************************************************************
 *********************** LOGGING MACROS   ************************
 *****************************************************************/
static const auto start_time = boost::chrono::steady_clock::now();

#define MAX_FNAME_PATH_LEN 128

/* Enable profiling */
#define DMTR_PROFILE
#define DMTR_APP_PROFILE
#define OP_DEBUG
#define LEGACY_PROFILING

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

#ifdef PRINT_RESPONSES
#define print_response(fmt, ...)\
    fprintf(LOG_FD, fmt "\n", ##__VA_ARGS__);
#else
#define print_response(...)
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

#define MAX_FILE_PATH_LEN 128

struct log_data {
    dmtr_latency_t *l;
    char const *name;
    FILE *fh;
    char filename[MAX_FNAME_PATH_LEN];
};

inline int dump_logs(std::vector<struct log_data> &logs, std::string log_dir, std::string label) {
    for (auto &log: logs) {
        DMTR_OK(dmtr_dump_latency_to_file(reinterpret_cast<const char *>(log.filename), log.l));
        DMTR_OK(dmtr_delete_latency(&log.l));
    }
    return 0;
}

static std::string generate_log_file_path(std::string log_dir,
                                          std::string exp_label,
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

inline void dump_pql(struct poll_q_len *s, std::string log_dir, std::string label) {
    char filename[MAX_FNAME_PATH_LEN];
    strncpy(filename, generate_log_file_path(log_dir, label, "pql").c_str(), MAX_FNAME_PATH_LEN);
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
 ************************* ARGS PARSING ************************
 ***************************************************************/

uint16_t port;
boost::optional<std::string> server_ip_addr;
uint32_t packet_size;
uint32_t iterations;
int dmtr_argc = 0;
char **dmtr_argv = NULL;
const char FILL_CHAR = 'a';
boost::optional<std::string> file;
std::string config_path;

using namespace boost::program_options;

void parse_args(int argc, char **argv, bool server, const options_description &d = {})
{
    options_description desc{"echo experiment options"};
    if (d.get_option_column_width() > 0) {
        const options_description &add_desc = d;
        desc.add(add_desc);
    }
    desc.add_options()
        ("help", "produce help message")
        ("ip", value<std::string>(), "server ip address")
        ("port", value<uint16_t>(&port)->default_value(12345), "server port")
        ("size,s", value<uint32_t>(&packet_size)->default_value(64), "packet payload size")
        ("iterations,i", value<uint32_t>(&iterations)->default_value(10), "test iterations")
        ("config-path,c", value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file")
        ("file", value<std::string>(), "log file");

    variables_map vm;
    try {
        parsed_options parsed =
            command_line_parser(argc, argv).options(desc).allow_unregistered().run();
        store(parsed, vm);
        if (vm.count("help")) {
            std::cout << desc << std::endl;
            exit(0);
        }
        notify(vm);
    } catch (const error &e) {
        std::cerr << e.what() << std::endl;
        std::cerr << desc << std::endl;
        exit(0);
    }

    if (!server) {
        server_ip_addr = "127.0.0.1";
    }

    if (access(config_path.c_str(), R_OK) == -1) {
        std::cerr << "Unable to find config file at `" << config_path << "`." << std::endl;
    } else {
        YAML::Node config = YAML::LoadFile(config_path);
        if (server) {
            YAML::Node node = config["server"]["bind"]["host"];
            if (YAML::NodeType::Scalar == node.Type()) {
                server_ip_addr = node.as<std::string>();
            }

            node = config["server"]["bind"]["port"];
            if (YAML::NodeType::Scalar == node.Type()) {
                port = node.as<uint16_t>();
            }
        } else {
            YAML::Node node = config["client"]["connect_to"]["host"];
            if (YAML::NodeType::Scalar == node.Type()) {
                server_ip_addr = node.as<std::string>();
            }

            node = config["client"]["connect_to"]["port"];
            if (YAML::NodeType::Scalar == node.Type()) {
                port = node.as<uint16_t>();
            }
        }
    }

    if (vm.count("ip")) {
        server_ip_addr = vm["ip"].as<std::string>();
        //std::cout << "Setting server IP to: " << ip << std::endl;
    }

    if (vm.count("port")) {
        port = vm["port"].as<uint16_t>();
        //std::cout << "Setting server port to: " << port << std::endl;
    }

    if (vm.count("iterations")) {
        iterations = vm["iterations"].as<uint32_t>();
        //std::cout << "Setting iterations to: " << iterations << std::endl;
    }

    if (vm.count("size")) {
        packet_size = vm["size"].as<uint32_t>();
        //std::cout << "Setting packet size to: " << packet_size << " bytes." << std::endl;
    }

    if (vm.count("file")) {
        file = vm["file"].as<std::string>();
    }
};

void* generate_packet()
{
    void *p = NULL;
    dmtr_malloc(&p, packet_size);
    char *s = reinterpret_cast<char *>(p);
    memset(s, FILL_CHAR, packet_size);
    s[packet_size - 1] = '\0';
    return p;
};

/**
* Counts how many digits in the given integer
* @param int
* @return int
*/
int how_many_digits(int num) {
    int length = 1;
    while (num /= 10) {
        length++;
    }
    return length;
}

#endif
