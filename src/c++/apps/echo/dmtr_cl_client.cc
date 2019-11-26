#include <string.h>
#include <signal.h>
#include <errno.h>
#include <sys/socket.h>
#include <arpa/inet.h>
#include <pthread.h>

#include <dmtr/libos/io_queue.hh>

#include "app.hh"

/*
 * This client sends requests in a closed loop.
 * It takes a list of URI from the command line, and will loop over it until $duration expires
 */

bool terminate = false;
void sig_handler(int signo) {
    log_debug("Setting terminate flag in signal handler");
    terminate = true;
}

void pin_thread(pthread_t thread, u_int16_t cpu) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(cpu, &cpuset);

    int rtn = pthread_setaffinity_np(thread, sizeof(cpu_set_t), &cpuset);
    if (rtn != 0) {
        fprintf(stderr, "could not pin thread: %s\n", strerror(errno));
    }
}

std::unique_ptr<ClientRequest> send_request(PspServiceUnit &su, int qfd, std::string request_str, int id) {
    dmtr_sgarray_t sga;
    std::unique_ptr<ClientRequest> cr(format_request(id, request_str, ip));
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = cr->req_size;
    sga.sga_segs[0].sgaseg_buf = static_cast<void *>(cr->req);
    /* Schedule and send the request */
#ifdef DMTR_TRACE
    cr->sending = take_time();
#endif
    dmtr_qtoken_t token;
    su.ioqapi.push(token, qfd, sga);
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
    free(cr->req);
    return std::move(cr);
}

int recv_request(PspServiceUnit &su, int qfd, ClientRequest *cr,
                 hr_clock::time_point start_time, boost::chrono::seconds exp_t) {
    dmtr_qtoken_t token;
    /* Wait for an answer */
    su.ioqapi.pop(token, qfd);
#ifdef DMTR_TRACE
    cr.pop_token = token;
#endif
    dmtr_qresult_t qr;
    int wait_rtn;
    while ((wait_rtn = su.wait(&qr, token)) == EAGAIN) {
        if (terminate || take_time() - start_time > exp_t) { return ETIME; }
    }
    assert(DMTR_OPC_POP == qr.qr_opcode);
    assert(qr.qr_value.sga.sga_numsegs == 1);
#ifdef DMTR_TRACE
    cr.completed = take_time();
#endif
    dmtr_free_mbuf(&qr.qr_value.sga);
    return wait_rtn;
}

int main (int argc, char *argv[]) {
    /* Parse options */
    int duration;
    uint16_t pipeline;
    std::string uri_list, label, log_dir, remote_host;
    namespace po = boost::program_options;
    po::options_description desc{"Rate client options"};
    desc.add_options()
        ("duration,d", po::value<int>(&duration)->default_value(10), "Runtime duration")
        ("label,l", po::value<std::string>(&label)->default_value("rate_client"), "experiment label")
        ("log-dir,L", po::value<std::string>(&log_dir)->default_value("./"), "Log directory")
        ("uri-list,f", po::value<std::string>(&uri_list)->required(), "List of URIs to request")
        ("pipeline,p", po::value<uint16_t>(&pipeline)->default_value(1), "Number of requests to keep in flight");
    parse_args(argc, argv, false, desc);

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
    read_uris(requests_str, uri_list);

    /* Init Persephone ServiceUnit */
    PspServiceUnit su(0, dmtr::io_queue::category_id::NETWORK_Q, argc, argv);

    pin_thread(pthread_self(), 4);

    /* Configure socket */
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (inet_pton(AF_INET, ip.c_str(), &saddr.sin_addr) != 1) {
        log_error("Unable to parse host address: %s", strerror(errno));
        exit(1);
    }
    saddr.sin_port = htons(port);
    log_info("Closed loop client configured to send requests to %s:%d", inet_ntoa(saddr.sin_addr), port);

    /* Configure IO queue */
    int qfd;
    DMTR_OK(su.ioqapi.socket(qfd, AF_INET, SOCK_STREAM, 0));

    /* "Connect" */
    DMTR_OK(su.ioqapi.connect(qfd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    boost::chrono::seconds duration_tp(duration);
    log_info("Running for %lu", duration_tp.count());

    std::vector<std::unique_ptr<ClientRequest> > requests;
    requests.reserve(1000000); //XXX
    int resp_idx = 0;

    uint32_t sent_requests = 0;
    uint32_t rcv_requests = 0;
    for (int i = 0; i < pipeline - 1; i++) {
        std::string request_str = requests_str[sent_requests % requests_str.size()];
        requests.push_back(std::move(send_request(su, qfd, request_str, sent_requests)));
        sent_requests++;
    }

    //FIXME this assumes that we always receive the request we just sent $pipeline ago
    dmtr_qtoken_t token;
    dmtr_sgarray_t sga;
    dmtr_qresult_t qr;
    hr_clock::time_point start_time = take_time();
    while (take_time() - start_time < duration_tp) {
        /* Pick a request, craft it, prepare the SGA */
        std::string request_str = requests_str[sent_requests % requests_str.size()];
        requests.push_back(std::move(send_request(su, qfd, request_str, sent_requests)));
        sent_requests++;

        int wait_rtn = recv_request(su, qfd, requests[resp_idx++].get(), start_time, duration_tp);
        if (wait_rtn == ECONNABORTED || wait_rtn == ECONNRESET || wait_rtn == ETIME) {
            break;
        }
        assert(wait_rtn == 0);
        rcv_requests++;
    }

    std::cout << "Receving pipelined requests..." << std::endl;
    /* Now take 2 seconds to get pending requests */
    boost::chrono::seconds grace_tp(duration+2);
    for (uint16_t i = 0; (i < pipeline - 1) && (take_time() - start_time <= grace_tp); ++i) {
        int wait_rtn = recv_request(su, qfd, requests[resp_idx++].get(), start_time, grace_tp);
        if (wait_rtn == ECONNABORTED || wait_rtn == ECONNRESET || wait_rtn == ETIME) {
            break;
        }
        assert(wait_rtn == 0);
        rcv_requests++;
    }

    DMTR_OK(su.ioqapi.close(qfd));
    log_info("Sent: %d, Received: %d (missing: %d) ",
             sent_requests, rcv_requests, sent_requests - rcv_requests);

#ifdef DMTR_TRACE
    FILE *f = fopen(generate_log_file_path(log_dir, label, "traces").c_str(), "w");
    if (f) {
        fprintf(f, "REQ_ID\tSENDING\tREADING\tCOMPLETED\tPUSH_TOKEN\tPOP_TOKEN\n");
    } else {
        log_error("Could not open log file!!");
        return 1;
    }

    for (auto &req: requests) {
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
