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
        } else if (su.second->type == "http") {
            HttpWorker *w = new HttpWorker(su.second->my_id, su.second.get(), &server.uri_store);
            server.http_workers.push_back(w);
            if (!server.typed_workers.empty()) {
                if (server.file_workers.size() < server.typed_workers[PAGE]) {
                    w->my_req_type = PAGE;
                    server.file_workers.push_back(w);
                } else if (server.regex_workers.size() < server.typed_workers[REGEX]) {
                    w->my_req_type = REGEX;
                    server.regex_workers.push_back(w);
                }
            }
            if (w->launch() != 0) {
                PspWorker::stop_all();
                break; //TODO cleanup and exit
            }
        } else {
            PSP_ERROR("Non supported service unit type: " << su.second->type);
            exit(1);
        }
    }

    /* Setup shared queues */
    for (auto &n: server.net_workers) {
        for (size_t j = 0; j < server.http_workers.size(); ++j) {
            PspWorker::register_peers(*n, *server.http_workers[j]);
            switch (server.http_workers[j]->my_req_type) {
                case REGEX:
                    n->regex_workers.push_back(server.http_workers[j]);
                    break;
                case PAGE:
                    n->file_workers.push_back(server.http_workers[j]);
                    break;
                default:
                    PSP_WARN("Unsupported type " << server.http_workers[j]->my_req_type);
                    break;
            }
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
