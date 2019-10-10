#include "httpops.hh" // for http_req_type
#include "common.hh"
#include <functional>
#include <vector>
#include <memory>

#define MAX_REQ_STATES 10000000

enum net_filters { RR, HTTP_REQ_TYPE, ONE_TO_ONE };
class Request {
    /* Request information */
    public: uint32_t net_qd; /** which network queue does the request originates from */
    public: uint32_t id;
    enum http_req_type type;

    /* Profiling variables */
    public: dmtr_qtoken_t pop_token;
    public: dmtr_qtoken_t push_token;
    public: hr_clock::time_point net_receive;
    public: hr_clock::time_point http_dispatch;
    public: hr_clock::time_point start_http;
    public: hr_clock::time_point end_http;
    public: hr_clock::time_point http_done;
    public: hr_clock::time_point net_send;

    Request(uint32_t qfd): net_qd(qfd) {}
};

/* TODO: make this an inner struct of the Worker class */
struct net_worker_args {
    net_filters filter;
    std::function<int(dmtr_sgarray_t *)> filter_f;
    struct sockaddr_in saddr;
    bool split;
};

/* TODO: have child classse for each worker type */
enum worker_type { NET, HTTP };
class Worker {
    public:
        /* I/O variables */
        int in_qfd;
        int out_qfd;

        struct net_worker_args args;

        /* Management variables */
        pthread_t me;
        uint8_t whoami;
        uint8_t core_id;
        bool terminate = false;

        /* ID variables */
        enum worker_type type;

        /* Profiling and Co. */
#ifdef LEGACY_PROFILING
        std::vector<std::pair<uint64_t, uint64_t> > runtimes;
#endif

#ifdef OP_DEBUG
        struct poll_q_len pql;
#endif

#ifdef DMTR_TRACE
        std::vector<std::unique_ptr<Request> > req_states; /* Used by network */
#endif

#if defined(LEGACY_PROFILING) || defined(DMTR_TRACE)
        Worker() {
#ifdef LEGACY_PROFILING
            runtimes.reserve(MAX_REQ_STATES);
#endif
#ifdef DMTR_TRACE
            req_states.reserve(MAX_REQ_STATES);
#endif
        }
#endif
};

/***********************************
 **********  PROFILING  ************
 ***********************************/

#ifdef LEGACY_PROFILING
void dump_latencies(Worker &worker, std::string &log_dir, std::string &label) {
    log_debug("Dumping latencies for worker %d on core %d\n", worker.whoami, worker.core_id);
    char filename[MAX_FILE_PATH_LEN];
    FILE *f = NULL;

    std::string wtype;
    if (worker.type == NET) {
        wtype = "network";
    } else if (worker.type == HTTP) {
        wtype = "http";
    }

    snprintf(filename, MAX_FILE_PATH_LEN, "%s/%s_%s-runtime-%d",
             log_dir.c_str(), label.c_str(), wtype.c_str(), worker.core_id);
    f = fopen(filename, "w");
    if (f) {
        fprintf(f, "TIME\tVALUE\n");
        for (auto &l: worker.runtimes) {
            fprintf(f, "%ld\t%ld\n", l.first, l.second);
        }
        fclose(f);
    } else {
        log_error("Failed to open %s for dumping latencies: %s", filename, strerror(errno));                                  }
}
#endif

void dump_traces(Worker &w, std::string log_dir, std::string label) {
    log_debug("Dumping traces for worker %d on core %d\n", w.whoami, w.core_id);
    char filename[MAX_FILE_PATH_LEN];
    FILE *f = NULL;

    snprintf(filename, MAX_FILE_PATH_LEN, "%s/%s-traces-%d",
             log_dir.c_str(), label.c_str(), w.core_id);
    f = fopen(filename, "w");
    if (f) {
        fprintf(
            f,
            "REQ_ID\tNET_RECEIVE\tHTTP_DISPATCH\tSTART_HTTP\tEND_HTTP\tHTTP_DONE\tNET_SEND\tPUSH_TOKEN\tPOP_TOKEN\n"
        );
        for (auto &r: w.req_states) {
            fprintf(
                f, "%d\t%lu\t%lu\t%lu\t%lu\t%lu\t%lu\t%lu\t%lu\n",
                r->id,
                since_epoch(r->net_receive), since_epoch(r->http_dispatch),
                since_epoch(r->start_http), since_epoch(r->end_http),
                since_epoch(r->http_done), since_epoch(r->net_send),
                r->push_token, r->pop_token
            );
            r.reset(); //release ownership + calls destructor
        }
        fclose(f);
    } else {
        log_error("Failed to open %s for dumping traces: %s", filename, strerror(errno));
    }
}

