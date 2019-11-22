#include <string>
#include <iostream>
#include <fstream>
#include <thread>

#include <signal.h>
#include <errno.h>
#include <arpa/inet.h>

#include <boost/program_options.hpp>

#include <dmtr/time.hh>
#include <dmtr/libos/persephone.hh>
#include <dmtr/libos/io_queue.hh>

#include "common.hh"
#include "logging.h"

#define DMTR_TRACE

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
    public: const char * req; /** The actual request */
    public: size_t req_size; /** Number of Bytes in the request */
    public: int conn_qd; /** The connection's queue descriptor */
    public: uint32_t id; /** Request id */

    public: ClientRequest(const char * req, size_t req_size, uint32_t id): req(req), req_size(req_size), id(id) {}
    public: ~ClientRequest() {}
};

bool req_latency_sorter(const std::unique_ptr<ClientRequest> &a, const std::unique_ptr<ClientRequest> &b) {
    return (a->completed - a->sending) < (b->completed - b->sending);
}
bool req_time_sorter(const std::unique_ptr<ClientRequest> &a, const std::unique_ptr<ClientRequest> &b) {
    return a->sending < b->sending;
}

/*
 * This client sends requests in a closed loop.
 * It takes a list of URI from the command line, and will loop over it until $duration expires
 */

bool terminate = false;
void sig_handler(int signo) {
    log_debug("Setting terminate flag in signal handler");
    terminate = true;
}


namespace bpo = boost::program_options;

struct ClientOpts {
    std::string ip;
    uint16_t port;
    std::string cmd_file;
    int duration;
    int pipeline;
    CommonOptions common;
};

int parse_client_args(int argc, char **argv, ClientOpts &options) {
    bpo::options_description opts{"KV Server options"};
    opts.add_options()
                    ("duration,d", bpo::value<int>(&options.duration)->required(), "Duration")
                    ("ip",
                        bpo::value<std::string>(&options.ip)->default_value("127.0.0.1"),
                        "Server IP")
                    ("port",
                        bpo::value<uint16_t>(&options.port)->default_value(12345),
                        "Server port")
                    ("cmd-file",
                        bpo::value<std::string>(&options.cmd_file)->required(),
                        "Initial commands")
                    ("pipeline,P",
                        bpo::value<int>(&options.pipeline)->default_value(1),
                        "Number to pipeline");

    return parse_args(argc, argv, opts, options.common);
}

std::unique_ptr<ClientRequest> send_request(PspServiceUnit &su, int qfd, std::string request_str, int id) {
    dmtr_sgarray_t sga;
    auto cr = std::make_unique<ClientRequest>(request_str.c_str(), request_str.size(), id);
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = request_str.size();
    sga.sga_segs[0].sgaseg_buf = const_cast<void *>(static_cast<const void *>(request_str.c_str()));
    /* Schedule and send the request */
#ifdef DMTR_TRACE
    cr->sending = take_time();
#endif
    dmtr_qtoken_t token;
    if (su.ioqapi.push(token, qfd, sga)) {
        log_error("Error pushing");
        return nullptr;
    }
#ifdef DMTR_TRACE
    cr->push_token = token;
#endif
    dmtr_qresult_t qr;
    while (su.wait(&qr, token) == EAGAIN) {
        if (terminate) { break; }
    } //XXX should we check for severed connection here?
    assert(DMTR_OPC_PUSH == qr.qr_opcode);
#ifdef DMTR_TRACE
    cr->reading = take_time();
#endif
    return std::move(cr);
}

int main (int argc, char *argv[]) {
    /* Parse options */
    int duration;
    std::string uri_list, label, log_dir, remote_host;
    namespace po = boost::program_options;
    ClientOpts opts;
    if (parse_client_args(argc, argv, opts)) {
        return 1;
    }

    log_info(
        "Running closed loop client for %d seconds, using URIs listed in %s",
        duration, uri_list.c_str()
    );

    /* Configure signal handler */
    if (signal(SIGINT, sig_handler) == SIG_ERR)
        log_error("can't catch SIGINT");
    if (signal(SIGTERM, sig_handler) == SIG_ERR)
        log_error("can't catch SIGTERM");

    /* Extract URIS from list */
    std::vector<std::string> requests_str;
    std::ifstream input_file(opts.cmd_file);
    if (!input_file.is_open()) {
        log_error("Cannot open %s", opts.cmd_file.c_str());
        return -1;
    }
    std::string line;
    while (std::getline(input_file, line)) {
        requests_str.push_back(line);
    }
    input_file.close();

    /* Init Persephone ServiceUnit */
    PspServiceUnit su(0, dmtr::io_queue::category_id::NETWORK_Q, argc, argv);

    pin_thread(pthread_self(), 4);
    /* Configure socket */
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (inet_pton(AF_INET, opts.ip.c_str(), &saddr.sin_addr) != 1) {
        log_error("Unable to parse host address: %s", strerror(errno));
        exit(1);
    }
    saddr.sin_port = htons(opts.port);
    log_info("Closed loop client configured to send requests to %s:%d", inet_ntoa(saddr.sin_addr), opts.port);

    /* Configure IO queue */
    int qfd;
    DMTR_OK(su.ioqapi.socket(qfd, AF_INET, SOCK_STREAM, 0));

    /* "Connect" */
    DMTR_OK(su.ioqapi.connect(qfd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    boost::chrono::seconds duration_tp(opts.duration);
    log_info("Running for %lu", duration_tp.count());

    std::vector<std::unique_ptr<ClientRequest> > requests;
    requests.reserve(10000000); //XXX
    int resp_idx = 0;

    uint32_t sent_requests = 0;
    for (int i=0; i < opts.pipeline - 1; i++) {
        std::string request_str = requests_str[sent_requests % requests_str.size()];
        requests.push_back(std::move(send_request(su, qfd, request_str, sent_requests)));
        sent_requests++;
    }


    dmtr_qtoken_t token;
    dmtr_qresult_t qr;
    hr_clock::time_point start_time = take_time();
    while (take_time() - start_time < duration_tp) {
        /* Pick a request, craft it, prepare the SGA */
        std::string request_str = requests_str[sent_requests % requests_str.size()];

        requests.push_back(std::move(send_request(su, qfd, request_str, sent_requests)));

        ClientRequest &resp_cr = *requests[resp_idx++];

        /* Wait for an answer */
        DMTR_OK(su.ioqapi.pop(token, qfd));
#ifdef DMTR_TRACE
        resp_cr.pop_token = token;
#endif
        int wait_rtn;
        while ((wait_rtn = su.wait(&qr, token)) == EAGAIN) {
            if (terminate) { break; }
        }
        if (wait_rtn == ECONNABORTED || wait_rtn == ECONNRESET) {
            break;
        }
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);
#ifdef DMTR_TRACE
        resp_cr.completed = take_time();
#endif
        //free(qr.qr_value.sga.sga_buf);
        dmtr_free_mbuf(&qr.qr_value.sga);
        sent_requests++;
    }

    // FIXME: CLIENT CRASHES ON CLOSE FOR SOME REASON
    // DMTR_OK(su.ioqapi.close(qfd));

    log_info("Done with all of %d requests", sent_requests);

#ifdef DMTR_TRACE
    std::string trace_file = opts.common.log_dir + "/traces";
    FILE *f = fopen(trace_file.c_str(), "w");
    if (f) {
        fprintf(f, "REQ_ID\tSENDING\tREADING\tCOMPLETED\tPUSH_TOKEN\tPOP_TOKEN\n");
    } else {
        log_error("Could not open log file!!");
        return 1;
    }

    std::vector<std::unique_ptr<ClientRequest>> filtered_reqs;
    sample_into(requests, filtered_reqs, req_latency_sorter, req_time_sorter, 10000);

    for (auto &req: filtered_reqs) {
        fprintf(
            f, "%d\t%lu\t%lu\t%lu\t%lu\t%lu\n",
            req->id,
            since_epoch(req->sending),
            since_epoch(req->reading),
            since_epoch(req->completed),
            req->push_token,
            req->pop_token
        );
    }

    fclose(f);
#endif

    return 0;
}
