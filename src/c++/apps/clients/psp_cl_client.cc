#include <string.h>
#include <errno.h>
#include <sys/socket.h>
#include <arpa/inet.h>
#include <pthread.h>
#include <memory>
#include <csignal>

#include <dmtr/libos/io/io_queue.hh>
#include "../common/PspWorker.hh"
#include "../common/Request.hh"
#include "../common/common.hh"
#include "psp_cl_client.hh"

int CLClientWorker::send_request(std::string request_str) {
    dmtr_sgarray_t sga;
    std::unique_ptr<ClientRequest> cr(ClientRequest::format_request(sent_requests, request_str, psu->ip));
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = cr->req_size;
    sga.sga_segs[0].sgaseg_buf = static_cast<void *>(cr->req);
    /* Schedule and send the request */
#ifdef TRACE
    cr->sending = take_time();
#endif
    dmtr_qtoken_t token;
    psu->ioqapi.push(token, connfd, sga);
#ifdef TRACE
    cr->push_token = token;
#endif
    dmtr_qresult_t qr;
    while (psu->wait(&qr, token) == EAGAIN) {
        if (terminate) { break; }
    } //XXX should we check for severed connection here?
    assert(DMTR_OPC_PUSH == qr.qr_opcode);
#ifdef TRACE
    cr->reading = take_time();
#endif
    free(cr->req);
    requests[sent_requests++] = std::move(cr);
    return 0;
}

int CLClientWorker::recv_request() {
    dmtr_qtoken_t token;
    /* Wait for an answer */
    psu->ioqapi.pop(token, connfd);
    dmtr_qresult_t qr;
    int wait_rtn;
    while ((wait_rtn = psu->wait(&qr, token)) == EAGAIN) {
        if (take_time() - start_time > duration_tp) { return ETIME; }
        if (wait_rtn == ECONNABORTED || wait_rtn == ECONNRESET) { return wait_rtn; }
    }
    assert(DMTR_OPC_POP == qr.qr_opcode);
    assert(qr.qr_value.sga.sga_numsegs == 1);
    uint32_t * const ridp = static_cast<uint32_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf);
    auto cr = requests.find(*ridp);
    if (cr == requests.end()) {
        log_error("Received response to unregistered request!!");
    }
#ifdef TRACE
    cr->second->pop_token = token;
    cr->second->completed = take_time();
#endif
    dmtr_free_mbuf(&qr.qr_value.sga);
    recv_requests++;
    return wait_rtn;
}

int main(int argc, char *argv[]) {
#ifdef TRACE
    log_info("Starting Closed Loop client with TRACE on");
#endif
    /* Parse options */
    int duration;
    uint16_t pipeline, remote_port;
    std::string uri_list, label, cfg_file, remote_host;
    namespace po = boost::program_options;
    po::options_description desc{"Closed loop client options"};
    desc.add_options()
        ("help", "produce help message")
        ("ip,I", po::value<std::string>(&remote_host)->required(), "server's IP")
        ("port,P", po::value<uint16_t>(&remote_port)->default_value(12345), "server's port")
        ("duration,d", po::value<int>(&duration)->default_value(10), "running duration")
        ("uri-list,u", po::value<std::string>(&uri_list)->required(), "uri list")
        ("label,l", po::value<std::string>(&label), "experiment label")
        ("pipeline,p", po::value<uint16_t>(&pipeline)->default_value(1), "pipeline width")
        ("config-path,c", po::value<std::string>(&cfg_file)->required(), "path to configuration file")
        ("out,O", po::value<std::string>(&log_dir), "log directory");

    po::variables_map vm;
    try {
        po::parsed_options parsed =
            po::command_line_parser(argc, argv).options(desc).allow_unregistered().run();
        po::store(parsed, vm);
        if (vm.count("help")) {
            std::cout << desc << std::endl;
            exit(0);
        }
        notify(vm);
    } catch (const po::error &e) {
        std::cerr << e.what() << std::endl;
        std::cerr << desc << std::endl;
        exit(0);
    }

    log_info(
        "Running closed loop client for %d seconds, using URIs listed in %s",
        duration, uri_list.c_str()
    );

    pin_thread(pthread_self(), 0);

    /* Extract URIS from list TODO: one set of requests per worker */
    std::vector<std::string> requests_str;
    read_uris(requests_str, uri_list);
    DMTR_TRUE(EINVAL, requests_str.size() > 0);

    /* Init libOS */
    Psp psp(cfg_file);

    /*  Create a worker per service unit */
    std::vector<CLClientWorker *> client_workers;
    for (auto &su: psp.service_units) {
        CLClientWorker *w = new CLClientWorker(
            su.second->my_id, su.second, duration, requests_str, pipeline, remote_host, remote_port
        );
        client_workers.push_back(w);
        if (w->launch() != 0) {
            PspWorker::stop_all();
            break; //XXX cleanup and exit
        }
    }

    auto sig_handler = [](int signal) {
        PspWorker::stop_all();
    };
    if (std::signal(SIGINT, sig_handler) == SIG_ERR)
        log_error("can't catch SIGINT");
    if (std::signal(SIGTERM, sig_handler) == SIG_ERR)
        log_error("can't catch SIGTERM");

    /* Join threads */
    for (auto &w: client_workers) {
        w->join();
#ifdef TRACE
        FILE *f = fopen(generate_log_file_path(label, "traces").c_str(), "w");
        if (f) {
            fprintf(f, "W_ID\tREQ_ID\tSENDING\tREADING\tCOMPLETED\tPUSH_TOKEN\tPOP_TOKEN\n");
        } else {
            log_error("Could not open log file!!");
            return 1;
        }
        for (auto &req: w->requests) {
            fprintf(
                f, "%d\t%d\t%lu\t%lu\t%lu\t%lu\t%lu\n",
                w->worker_id,
                req.second->id,
                since_epoch(req.second->sending),
                since_epoch(req.second->reading),
                since_epoch(req.second->completed),
                req.second->push_token,
                req.second->pop_token
            );
        }
        fclose(f);
#endif
    }
    return 0;
}
