#include <string.h>
#include <signal.h>
#include <errno.h>
#include <sys/socket.h>
#include <arpa/inet.h>

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

int main (int argc, char *argv[]) {
    /* Parse options */
    int duration;
    std::string uri_list, label, log_dir, remote_host;
    namespace po = boost::program_options;
    po::options_description desc{"Rate client options"};
    desc.add_options()
        ("duration,d", po::value<int>(&duration)->default_value(10), "Runtime duration")
        ("label,l", po::value<std::string>(&label)->default_value("rate_client"), "experiment label")
        ("log-dir,L", po::value<std::string>(&log_dir)->default_value("./"), "Log directory")
        ("uris-list,f", po::value<std::string>(&uri_list)->required(), "List of URIs to request");
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
    requests.reserve(100000); //XXX
    dmtr_qtoken_t token;
    dmtr_sgarray_t sga;
    dmtr_qresult_t qr;
    uint32_t sent_requests = 0;
    hr_clock::time_point start_time = take_time();
    while (take_time() - start_time < duration_tp) {
        /* Pick a request, craft it, prepare the SGA */
        std::string request_str = requests_str[sent_requests % requests_str.size()];
        std::unique_ptr<ClientRequest> cr(format_request(sent_requests, request_str, ip));

        sga.sga_numsegs = 1;
        sga.sga_segs[0].sgaseg_len = cr->req_size;
        sga.sga_segs[0].sgaseg_buf = static_cast<void *>(cr->req);
        /* Schedule and send the request */
#ifdef DMTR_TRACE
        cr->sending = take_time();
#endif
        DMTR_OK(su.ioqapi.push(token, qfd, sga));
#ifdef DMTR_TRACE
        cr->push_token = token;
#endif
        while (su.wait(&qr, token) == EAGAIN) {
            if (terminate) { break; }
        } //XXX should we check for severed connection here?
        assert(DMTR_OPC_PUSH == qr.qr_opcode);
#ifdef DMTR_TRACE
        cr->reading = take_time();
#endif
        /* Wait for an answer */
        DMTR_OK(su.ioqapi.pop(token, qfd));
#ifdef DMTR_TRACE
        cr->pop_token = token;
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
        cr->completed = take_time();
#endif
        requests.push_back(std::move(cr));
        free(qr.qr_value.sga.sga_buf);
        sent_requests++;
    }

    DMTR_OK(su.ioqapi.close(qfd));

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
