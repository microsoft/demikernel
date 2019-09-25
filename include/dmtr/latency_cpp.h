#ifndef DMTR_LATENCY_CPP_H_IS_INCLUDED
#define DMTR_LATENCY_CPP_H_IS_INCLUDED


#include <memory>
#include <functional>
#include <unordered_map>
#include <mutex>
#include <boost/chrono.hpp>

// The maximum lenth for the log URI
#define MAX_LOG_FILENAME_LEN 128

extern std::string dmtr_log_directory;

typedef std::unique_ptr<dmtr_latency_t, std::function<void(dmtr_latency_t *)> > latency_ptr_type;
int dmtr_register_latencies(const char *label,
                            std::unordered_map<pthread_t, latency_ptr_type> &latencies);

#if DMTR_PROFILING
extern std::unordered_map<pthread_t, latency_ptr_type> read_latencies;
extern std::unordered_map<pthread_t, latency_ptr_type> write_latencies;
#endif

int dmtr_record_timed_latency(dmtr_latency_t *latency, uint64_t record_time, uint64_t ns);

long int since_epoch(boost::chrono::steady_clock::time_point &time);
long int ns_diff(boost::chrono::steady_clock::time_point &start,
                 boost::chrono::steady_clock::time_point &end);

int dmtr_dump_latency_to_file(const char *filename, dmtr_latency_t *latency);
#endif
