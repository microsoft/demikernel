#include <csignal>
#include "psp_http_server.hh"

int main (int argc, char *argv[]) {
#ifdef TRACE
    PSP_INFO("Starting HTTP application with TRACE on");
#endif

    HttpServer server(argc, argv);

    /* Create workers */
    for (auto &su: server.psp->service_units) {
        if (su.second->type.empty()) {
            PSP_WARN("passing service unit with unknown type");
            continue;
        }
        if (su.second->type == "network") {
            NetWorker *w = new NetWorker(su.second->my_id, su.second.get(), server.dispatch_policy);
            server.net_workers.push_back(w);
            if (w->launch() != 0) {
                PspWorker::stop_all();
                break; //TODO cleanup and exit
            }
        } else if  (su.second->type == "http") {
            HttpWorker *w = new HttpWorker(su.second->my_id, su.second.get(), &server.uri_store);
            server.http_workers.push_back(w);
            if (w->launch() != 0) {
                PspWorker::stop_all();
                break; //TODO cleanup and exit
            }
        } else {
            PSP_ERROR("Non supported service unit type: " << su.second->type);
            exit(1);
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
    for (auto &w: server.net_workers) {
        w->join();
    }
    for (auto &w: server.http_workers) {
        w->join();
    }
    return 0;
}
