#ifndef KV_COMMON_H_
#define KV_COMMON_H_
#include <string>
#include <boost/program_options.hpp>

struct CommonOptions {
    std::string log_dir;
    std::string config_path;
};

int parse_args(int argc, char **argv,
               boost::program_options::options_description &opts,
               CommonOptions &common);

#endif
