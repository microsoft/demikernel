#ifndef APPS_H_
#define APPS_H_

#include "httpops.hh" // for http_req_type
#include "common.hh"
#include <functional>
#include <vector>
#include <unordered_map>
#include <memory>
#include <fstream>

#include <dmtr/libos/io/persephone.hh>

#define MAX_REQ_STATES 10000000
#define MAX_REQUEST_SIZE 4192

#define DMTR_TRACE

/*****************************************************************
 *********************** REQUEST STRUCTURES **********************
 *****************************************************************/

enum req_type {
    UNKNOWN = 0,
    ALL,
    REGEX,
    PAGE,
    POPULAR_PAGE,
    UNPOPULAR_PAGE,
};

const char *req_type_str[] = {
    "UNKNOWN",
    "ALL",
    "REGEX",
    "PAGE",
    "POPULAR_PAGE",
    "UNPOPULAR_PAGE"
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

class ClientRequest {
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
    public: hr_clock::time_point connecting;     /**< Time that dmtr_connect() started */
    public: hr_clock::time_point connected;   /**< Time that dmrt_connect() completed */
    public: hr_clock::time_point sending;       /**< Time that dmtr_push() started */
    public: hr_clock::time_point reading;        /**< Time that dmtr_pop() started */
    public: hr_clock::time_point completed;   /**< Time that dmtr_pop() completed */
    public: dmtr_qtoken_t push_token; /** The token associated to writing the request */
    public: dmtr_qtoken_t pop_token; /** The token associated with reading the response */
#endif
    public: bool valid; /** Whether the response was valid */
    public: char * req; /** The actual request */
    public: size_t req_size; /** Number of Bytes in the request */
    public: int conn_qd; /** The connection's queue descriptor */
    public: uint32_t id; /** Request id */

    public: ClientRequest(char * const req, size_t req_size, uint32_t id): req(req), req_size(req_size), id(id) {}
    //public: ~ClientRequest() { free(req); }
};

#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
enum ReqStatus {
    CONNECTING,
    CONNECTED,
    SENDING,
    READING,
    COMPLETED,
};

inline void update_request_state(struct ClientRequest &req, enum ReqStatus status, const hr_clock::time_point &op_time) {
    switch (status) {
        case CONNECTING:
            req.connecting = op_time;
            break;
        case CONNECTED:
            req.connected = op_time;
            break;
        case SENDING:
            req.sending = op_time;
            break;
        case READING:
            req.reading = op_time;
            break;
        case COMPLETED:
            req.completed = op_time;
            break;
    }
}
#endif

/*****************************************************************
 *********************** HTTP TOOLS ******************************
 *****************************************************************/
/* Default HTTP GET request */
const char *REQ_STR =
        "GET /%s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\nUser-Agent: %s\r\n\r\n";
/* Start of the string for a valid HTTP response */
const std::string VALID_RESP = "HTTP/1.1 200 OK";
const std::string CONTENT_LEN = "Content-Length: ";
std::string DEFAULT_USER_AGENT = "Perséphone";

/* Validates the given response, checking it against the valid response string */
static inline bool validate_response(std::string &resp_str, bool check_content_len) {
    if ((resp_str.find(VALID_RESP) == 0) & !check_content_len) {
        return true;
    }
    size_t ctlen = resp_str.find(CONTENT_LEN);
    size_t hdr_end = resp_str.find("\r\n\r\n");
    if (ctlen != std::string::npos && hdr_end != std::string::npos) {
        size_t body_len = std::stoi(resp_str.substr(ctlen+CONTENT_LEN.size(), hdr_end - ctlen));
        std::string body = resp_str.substr(hdr_end + 4);
        /* We purposedly ignore the last byte, because we hacked the response to null terminate it */
        if (strlen(body.c_str()) == body_len - 1) {
            return true;
        }
    }
    log_debug("Invalid response received: %s", resp_str.c_str());
    return false;
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

static inline ClientRequest * format_request(uint32_t id, std::string &req_uri, std::string &host) {
    /* Extract request type from request string */
    std::string user_agent;
    std::string uri;
    std::stringstream ss(req_uri);
    getline(ss, user_agent, ',');
    if (user_agent.size() == req_uri.size()) {
        log_error("Request type not present in URI!");
        exit(1);
    }
    getline(ss, uri, ',');

    /* Allocate and format buffer */
    char * const req = static_cast<char *>(malloc(MAX_REQUEST_SIZE));
    memset(req, '\0', MAX_REQUEST_SIZE);
    /* Prepend request ID to payload */
    memcpy(req, (uint32_t *) &id, sizeof(uint32_t));
    size_t req_size = snprintf(
        req + sizeof(uint32_t), MAX_REQUEST_SIZE - sizeof(uint32_t),
        REQ_STR, uri.c_str(), host.c_str(), user_agent.c_str()
    );
    req_size += sizeof(uint32_t);
    return new ClientRequest(req, req_size, id);
}

/**
 * Retrieve the type stored in User-Agent
 */
static inline enum req_type psp_get_req_type(std::string &url) {
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
    std::unordered_map<uint8_t, int> shared_qfds;
};

/* TODO: have child classse for each worker type */
enum worker_type { NET, HTTP };
class Worker {
    public:
        /* I/O variables */
        int shared_qfd;

        struct net_worker_args args;

        PspServiceUnit *psp_su;

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
    public: std::unordered_map<
                enum req_type,
                std::pair<std::vector<std::shared_ptr<Worker> > *, uint16_t>
            > types_map;

    public: std::vector<std::shared_ptr<Worker> > workers; /* All the workers */
    public: std::vector<std::shared_ptr<Worker> > http_workers; /* Just the HTTP workers */
    public: uint16_t n_http_workers;
    public: std::vector<std::shared_ptr<Worker> > regex_workers; /* HTTP workers dedicated to REGEX type requests */
    public: uint16_t n_regex_workers;
    public: std::vector<std::shared_ptr<Worker> > page_workers;
    public: uint16_t n_page_workers;
    public: std::vector<std::shared_ptr<Worker> > popular_page_workers;
    public: uint16_t n_popular_page_workers;
    public: std::vector<std::shared_ptr<Worker> > unpopular_page_workers;
    public: uint16_t n_unpopular_page_workers;

    public: std::vector<std::shared_ptr<Worker> > net_workers;
    public: uint16_t n_net_workers;

    public: dispatch_policy net_dispatch_policy;
    public: std::function<enum req_type(std::string &)> net_dispatch_f;
};

//TODO this should be a class member function of Psp
std::shared_ptr<Worker> create_http_worker(Psp &psp, bool typed, enum req_type type, uint16_t index,
                                           dmtr::shared_item *producer_si, dmtr::shared_item *consumer_si);

/***********************************
 **********  PROFILING  ************
 ***********************************/

#ifdef LEGACY_PROFILING
void inline dump_latencies(Worker &worker, std::string &log_dir, std::string &label) {
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
void inline dump_traces(std::shared_ptr<Worker > w, std::string &log_dir, std::string &label) {
    log_debug("Dumping traces for worker %d on core %d", w->whoami, w->core_id);
    char filename[MAX_FILE_PATH_LEN];
    FILE *f = NULL;

    snprintf(filename, MAX_FILE_PATH_LEN, "%s/%s-traces-%d",
             log_dir.c_str(), label.c_str(), w->core_id);
    f = fopen(filename, "w");
    if (f) {
        fprintf(
            f,
            "REQ_ID\tREQ_TYPE\tNET_RECEIVE\tHTTP_DISPATCH\tSTART_HTTP\tEND_HTTP\tHTTP_DONE\tNET_SEND\tPUSH_TOKEN\tPOP_TOKEN\n"
        );
        for (auto &r: w->req_states) {
            fprintf(
                f, "%d\t%s\t%lu\t%lu\t%lu\t%lu\t%lu\t%lu\t%lu\t%lu\n",
                r->id, req_type_str[r->type],
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

bool req_latency_sorter(const std::unique_ptr<ClientRequest> &a, const std::unique_ptr<ClientRequest> &b) {
    return (a->completed - a->sending) < (b->completed - b->sending);
}

bool req_time_sorter(const std::unique_ptr<ClientRequest> &a, const std::unique_ptr<ClientRequest> &b) {
    return a->sending < b->sending;
}
#endif

#endif // APPS_H_
