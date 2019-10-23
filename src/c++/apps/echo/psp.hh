#include "httpops.hh" // for http_req_type
#include "common.hh"
#include <functional>
#include <vector>
#include <unordered_map>
#include <memory>

#define MAX_REQ_STATES 10000000

enum req_type {
    UNKNOWN,
    ALL,
    REGEX,
    PAGE,
    POPULAR_PAGE,
    UNPOPULAR_PAGE,
};

enum dispatch_policy { RR, HTTP_REQ_TYPE, ONE_TO_ONE };
class Request {
    /* Request information */
    public: uint32_t net_qd; /** which network queue does the request originates from */
    public: uint32_t id;
    enum req_type type;

    /* Profiling variables */
#ifdef DMTR_TRACE
    public: dmtr_qtoken_t pop_token;
    public: dmtr_qtoken_t push_token;
    public: hr_clock::time_point net_receive;
    public: hr_clock::time_point http_dispatch;
    public: hr_clock::time_point start_http;
    public: hr_clock::time_point end_http;
    public: hr_clock::time_point http_done;
    public: hr_clock::time_point net_send;
#endif

    Request(uint32_t qfd): net_qd(qfd) {}
};

/**
 * Retrieve the type stored in User-Agent
 */
static enum req_type psp_get_req_type(std::string &url) {
    //get a pointer to the User-Agent part of the string
    size_t user_agent_pos = url.find("User-Agent:");
    if (user_agent_pos == std::string::npos) {
        log_error("User-Agent was not present in request headers!");
        return UNKNOWN;
    }
    user_agent_pos += 12; // "User-Agent: "
    size_t body_pos = url.find("\r\n\r\n");
    if (body_pos == std::string::npos) {
        log_error("User-Agent was not present in request headers!");
        return UNKNOWN;
    }
    std::string req_type(url.substr(user_agent_pos, body_pos));
    if (strncmp(req_type.c_str(), "REGEX", 5) == 0) {
        return REGEX;
    } else if (strncmp(req_type.c_str(), "PAGE", 4) == 0) {
        return PAGE;
    } else if (strncmp(req_type.c_str(), "UNPOPULAR", 9) == 0) {
        return UNPOPULAR_PAGE;
    } else if (strncmp(req_type.c_str(), "POPULAR", 7) == 0) {
        return POPULAR_PAGE;
    } else {
        return UNKNOWN;
    }
}

/* TODO: make this an inner struct of the Worker class */
struct net_worker_args {
    dispatch_policy dispatch_p;
    std::function<enum req_type(std::string &)> dispatch_f;
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

        /* Scheduling variables */
        std::unordered_map<enum req_type, uint32_t> type_counts;
        enum req_type handling_type;
        uint32_t num_rcvd;

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
            num_rcvd = 0;
#ifdef LEGACY_PROFILING
            runtimes.reserve(MAX_REQ_STATES);
#endif
#ifdef DMTR_TRACE
            req_states.reserve(MAX_REQ_STATES);
#endif
            type_counts.insert(std::pair<enum req_type, uint32_t>(REGEX, 0));
            type_counts.insert(std::pair<enum req_type, uint32_t>(PAGE, 0));
            type_counts.insert(std::pair<enum req_type, uint32_t>(POPULAR_PAGE, 0));
            type_counts.insert(std::pair<enum req_type, uint32_t>(UNPOPULAR_PAGE, 0));
        }
#endif
};

class Psp {
    public: std::vector<std::shared_ptr<Worker> > workers; /* All the workers */
    public: std::vector<std::shared_ptr<Worker> > http_workers; /* Just the HTTP workers */
    public: std::vector<std::shared_ptr<Worker> > regex_workers; /* HTTP workers dedicated to REGEX type requests */
    public: std::vector<std::shared_ptr<Worker> > page_workers;
    public: std::vector<std::shared_ptr<Worker> > popular_page_workers;
    public: std::vector<std::shared_ptr<Worker> > unpopular_page_workers;

    public: dispatch_policy net_dispatch_policy;
    public: std::function<enum req_type(std::string &)> net_dispatch_f;
};

//TODO this should be a class member function of Psp
std::shared_ptr<Worker> create_http_worker(Psp &psp, bool typed, enum req_type type, uint16_t index);

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

#ifdef DMTR_TRACE
void dump_traces(std::shared_ptr<Worker > w, std::string &log_dir, std::string &label) {
    log_debug("Dumping traces for worker %d on core %d", w->whoami, w->core_id);
    char filename[MAX_FILE_PATH_LEN];
    FILE *f = NULL;

    snprintf(filename, MAX_FILE_PATH_LEN, "%s/%s-traces-%d",
             log_dir.c_str(), label.c_str(), w->core_id);
    f = fopen(filename, "w");
    if (f) {
        fprintf(
            f,
            "REQ_ID\tNET_RECEIVE\tHTTP_DISPATCH\tSTART_HTTP\tEND_HTTP\tHTTP_DONE\tNET_SEND\tPUSH_TOKEN\tPOP_TOKEN\n"
        );
        for (auto &r: w->req_states) {
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
#endif
