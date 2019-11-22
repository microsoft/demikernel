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

int pin_thread(pthread_t thread, int cpu);

template <typename T>
void sample_into(std::vector<T> &from, std::vector<T>&to,
                 bool(*lat_compare)(const T&, const T&),
                 bool(*time_compare)(const T&, const T&),
                 int n) {
    std::sort(from.begin(), from.end(), lat_compare);
    int spacing = from.size() / n;
    if (spacing == 0) {
        spacing = 1;
    }
    to.push_back(std::move(from[0]));
    for (unsigned int i = spacing; i < from.size() - 1; i+=spacing) {
        to.push_back(std::move(from[i]));
    }
    to.push_back(std::move(from[from.size() - 1]));
    std::sort(to.begin(), to.end(), time_compare);
}

#endif
