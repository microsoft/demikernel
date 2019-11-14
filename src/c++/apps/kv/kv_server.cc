#include <string>
#include <iostream>
#include <fstream>
#include <thread>
#include <csignal>

#include <arpa/inet.h>

#include <boost/program_options.hpp>

#include <dmtr/libos.h>
#include <dmtr/libos/persephone.hh>

#include "PspWorker.hh"
#include "common.hh"
#include "logging.h"

template <typename T>
void as_sga(T &from, dmtr_sgarray_t &sga) {
    sga.sga_buf = nullptr;
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_buf = &from;
    sga.sga_segs[0].sgaseg_len = sizeof(from);
}
struct KvRequest {
    int req_qfd;
    dmtr_sgarray_t sga;

    KvRequest(int qfd, dmtr_sgarray_t &sga) : req_qfd(qfd), sga(sga) {}
};

struct KvResponse {
    int req_qfd;
    void *data;
    size_t data_size;
    bool moved;

    KvResponse(int req_qfd, std::string &resp) :
            req_qfd(req_qfd), moved(false){
        data = malloc(resp.size());
        data_size = resp.size();
        memcpy(data, resp.c_str(), data_size);
    }

    ~KvResponse() {
        if (!moved) {
             free(data);
        }
    }

    int move_to_sga(dmtr_sgarray_t &sga) {
        if (moved) {
            return -1;
        }
        sga.sga_buf = nullptr;
        sga.sga_numsegs = 1;
        sga.sga_segs[0].sgaseg_buf = data;
        sga.sga_segs[0].sgaseg_len = data_size;
        moved = true;
        return 0;
    }
};

class NetWorker : public PspWorker {
public:

    enum worker_choice {
        RR, KEY
    };

private:


    struct sockaddr_in bind_addr;

    worker_choice choice_fn;

    using hr_clock = std::chrono::high_resolution_clock;
    std::vector<hr_clock::time_point> entry_times;
    std::vector<hr_clock::time_point> exit_times;
    std::string log_filename;

    bool record_lat;
    int lqd;
    std::vector<dmtr_qtoken_t> tokens;


    int start_offset = 0;

    int worker_offset = 0;

    int round_robin_choice(dmtr_qresult_t &dequeued) {
        int n_peers = peer_ids.size();
        log_debug("Choosing from %d peers", n_peers);
        if (n_peers == 0) {
            return -1;
        }
        if (++worker_offset >= n_peers) {
            worker_offset = 0;
        }

        return peer_ids[worker_offset];
    }

    int first_key_digit_choice(dmtr_qresult_t &dequeued) {
        void *buf = dequeued.qr_value.sga.sga_segs[0].sgaseg_buf;
        char *req = static_cast<char*>(buf);
        char *space = strstr(req, " ");
        char dig;
        if (space == NULL) {
            dig = '0';
        } else {
            dig = *(space + 1);
        }
        int idx = ((int)dig - (int)'0');
        int n_peers = peer_ids.size();
        return peer_ids[idx % n_peers];
    }


    int choose_worker(dmtr_qresult_t &dequeued) {
        switch (choice_fn) {
            case KEY:
                return first_key_digit_choice(dequeued);
            case RR:
            default:
                return round_robin_choice(dequeued);
        }
    }

    long int ns_since_start(hr_clock::time_point &tp) {
        return std::chrono::duration_cast<std::chrono::nanoseconds>(tp - entry_times[0]).count();
    }

public:

    int dump_times() {
        if (log_filename.size() == 0) {
            return 0;
        }
        std::ofstream logfile(log_filename);
        if (logfile.is_open()) {
            logfile << "entry\texit" << std::endl;
            for (unsigned int i=0; i < exit_times.size(); i++) {
                logfile << ns_since_start(entry_times[i]) << "\t"
                        << ns_since_start(exit_times[i]) << std::endl;
            }
            logfile.close();
            log_info("Wrote net logs to %s", log_filename.c_str());
            return 0;
        } else {
            log_error("Coult not open logfile %s", log_filename.c_str());
            return -1;
        }
    }

public:

    NetWorker(struct sockaddr_in &addr,
              worker_choice choice = RR,
              std::string log_filename = "") :
            PspWorker(0, dmtr::io_queue::NETWORK_Q),
            bind_addr(addr), choice_fn(choice),
            log_filename(log_filename), record_lat(log_filename.size() > 0)
    {
        entry_times.reserve(10000000);
        exit_times.reserve(10000000);
    }

    int setup() {
        pin_thread(pthread_self(), 4);
        DMTR_OK(psu.ioqapi.socket(lqd, AF_INET, SOCK_STREAM, 0));
        DMTR_OK(psu.ioqapi.bind(lqd,
                                reinterpret_cast<struct sockaddr*>(&bind_addr),
                                sizeof(bind_addr)));
        dmtr_qtoken_t token;
        DMTR_OK(psu.ioqapi.listen(lqd, 100));
        DMTR_OK(psu.ioqapi.accept(token, lqd));
        tokens.push_back(token);

        for (int peer_id : peer_ids) {
            DMTR_OK(pop_from_peer(peer_id, token));
            tokens.push_back(token);
        }
        return 0;
    }

    int dequeue(dmtr_qresult_t &dequeued) {
        int idx;
        int status = psu.wait_any(&dequeued, &start_offset, &idx, tokens.data(), tokens.size());
        if (status == EAGAIN) {
            return EAGAIN;
        }
        tokens.erase(tokens.begin() + idx);
        log_debug("wait_any returned %d", status);
        if (status == ECONNABORTED) {
            return EAGAIN;
        }
        return status;
    }

    int work(int status, dmtr_qresult_t &dequeued) {
        hr_clock::time_point entry_time = hr_clock::now();
        if (status != 0) {
            log_error("NetWorker work() received non-0 status %d", status);
            return -1;
        }
        if (dequeued.qr_qd == lqd) {
            assert(dequeued.qr_opcode == DMTR_OPC_ACCEPT);
            dmtr_qtoken_t token;
            DMTR_OK(psu.ioqapi.pop(token, dequeued.qr_value.ares.qd));
            tokens.push_back(token);

            DMTR_OK(psu.ioqapi.accept(token, lqd));
            tokens.push_back(token);
            log_debug("Accepted a new connection");
            return 0;
        }
        if (dequeued.qr_opcode == DMTR_OPC_PUSH) {
            // sga segment must be freed after pushing to the client
            free(dequeued.qr_value.sga.sga_segs[0].sgaseg_buf);
            return 0;
        }
        log_debug("Received POP code");
        assert(DMTR_OPC_POP == dequeued.qr_opcode);
        int dequeued_id = get_peer_id(dequeued.qr_qd);
        if (dequeued_id == -1) {
            if (record_lat) {
                entry_times.push_back(entry_time);
            }
            // New request
            int new_worker_id = choose_worker(dequeued);
            KvRequest *kvr = new KvRequest(dequeued.qr_qd, dequeued.qr_value.sga);
            dmtr_sgarray_t sga_req;
            as_sga(*kvr, sga_req);
            if (blocking_push_to_peer(new_worker_id, sga_req) == -1) {
                log_warn("Could not push to worker %d", new_worker_id);
            } else {
                log_debug("NetWorker pushed to peer %d", new_worker_id);
            }

            dmtr_qtoken_t token;
            DMTR_OK(psu.ioqapi.pop(token, dequeued.qr_qd));
            tokens.push_back(token);
        } else {
            // Returned from peer
            dmtr_sgarray_t &sga = dequeued.qr_value.sga;
            assert(sga.sga_numsegs == 1 && sga.sga_segs[0].sgaseg_len == sizeof(KvResponse));

            auto resp = static_cast<KvResponse*>(sga.sga_segs[0].sgaseg_buf);
            dmtr_qtoken_t token;

            dmtr_sgarray_t resp_sga;
            resp->move_to_sga(resp_sga);
            DMTR_OK(psu.ioqapi.push(token, resp->req_qfd, resp_sga));
            int status = psu.wait(NULL, token);
            if (status == EAGAIN) {
                tokens.push_back(token);
            }
            if (record_lat) {
                exit_times.push_back(hr_clock::now());
            }
            DMTR_OK(status);

            DMTR_OK(psu.ioqapi.pop(token, dequeued.qr_qd));
            tokens.push_back(token);
            delete resp;
        }
        return 0;
    }
};

class KvStore {
private:
    bool writeable;
    bool readable;
    std::unordered_map<std::string, std::string> store;

    const std::string PUT_STR = "PUT ";
    const std::string GET_STR = "GET ";
    const std::string SZOF_STR = "SZOF ";
    const std::string NNZ_STR = "NNZ ";

    static bool startswith(const std::string a, const std::string b) {
        if (a.compare(0, b.size(), b)) {
            return false;
        }
        return true;
    }

    int process_put(const std::string &req, std::string &output) {
        if (!writeable) {
            output = "ERR: Not writeable";
            return -1;
        }

        size_t key_end = req.find_first_of(" ", PUT_STR.size()+1);
        if (key_end == std::string::npos) {
            output = "ERR: No key";
            return -1;
        }
        size_t keylen = key_end - PUT_STR.size();
        size_t vallen = req.size() - key_end;
        store[req.substr(PUT_STR.size(), keylen)] = req.substr(key_end+1, vallen);
        output = "SUCCESS";
        return 0;
    }

    int process_get(const std::string &req, std::string &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }

        if (req.find_first_of(" ", GET_STR.size()+1) != std::string::npos) {
            output = "ERR: Key contains space";
            return -1;
        }

        size_t keylen = req.size() - GET_STR.size();
        auto it = store.find(req.substr(GET_STR.size(), keylen));
        if (it == store.end()) {
            output = "ERR: Bad key " + req.substr(GET_STR.size(), keylen);
            return -1;
        }
        output = it->second;
        return 0;
    }

    int process_szof(const std::string &req, std::string &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }

        if (req.find_first_of(" ", SZOF_STR.size() + 1) != std::string::npos) {
            output = "ERR: Key contains space";
            return -1;
        }

        size_t keylen = req.size() - SZOF_STR.size();
        auto it = store.find(req.substr(SZOF_STR.size(), keylen));
        if (it == store.end()) {
            output = "ERR: Bad key";
            return -1;
        }
        // Using strlen() rather than str::size so that it requires accessing the full string
        output = std::to_string(strlen(it->second.c_str()));
        return 0;
    }

    int process_nnz(const std::string &req, std::string &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }

        if (req.find_first_of(" ", NNZ_STR.size() + 1) != std::string::npos) {
            output = "ERR: Key contains space";
            return -1;
        }

        size_t keylen = req.size() - NNZ_STR.size();
        auto it = store.find(req.substr(NNZ_STR.size(), keylen));
        if (it == store.end()) {
            output = "ERR: Bad key";
            return -1;
        }
        int count = 0;
        for (const char &c: it->second) {
            if (c != '0') {
                count++;
            }
        }
        output = std::to_string(count);
        return 0;
    }


public:

    int process_req(const std::string &req, std::string &output) {
        if (startswith(req, PUT_STR)) {
            return process_put(req, output);
        } else if (startswith(req, GET_STR)) {
            return process_get(req, output);
        } else if (startswith(req, SZOF_STR)) {
            return process_szof(req, output);
        } else if (startswith(req, NNZ_STR)) {
            return process_nnz(req, output);
        }
        output = "ERR: Unknown reqtype";
        return -1;
    }

    KvStore(const std::string filename) : writeable(true), readable(false) {
        std::ifstream input_file(filename);
        if (input_file.is_open()) {
            std::string line;
            while (std::getline(input_file, line)) {
                std::string output;
                if (process_req(line, output)) {
                    log_warn("Could not process line %s", line.c_str());
                }
            }
            input_file.close();
        } else {
            log_warn("Could not open input file %s", filename.c_str());
            log_warn("KV store will be writeable! May have concurrency issues");
            writeable = true;
            readable = true;
            return;
        }
        writeable = false;
        readable = true;
    }
};

class StoreWorker : public PspWorker {

    int networker_qd;
    dmtr_qtoken_t pop_token;

private:
    KvStore &store;

public:
    StoreWorker(int id, KvStore &store) :
            PspWorker(id, dmtr::io_queue::SHARED_Q),
            store(store) {
        if (id == 0) {
            // RAISE WARNING
        }
    }

    int setup() {
        pin_thread(pthread_self(), 4+worker_id);
        networker_qd = get_peer_qd(0);
        if (networker_qd == -1) {
            log_error("Must register networker before starting StoreWorker");
            return -1;
        }
        DMTR_OK(psu.ioqapi.pop(pop_token, networker_qd));
        return 0;
    }

    int dequeue(dmtr_qresult_t &dequeued) {
        int status = psu.wait(&dequeued, pop_token);
        if (status == EAGAIN) {
            return EAGAIN;
        }
        log_debug("StoreWorker Got non-EAGAIN");
        DMTR_OK(status);
        DMTR_OK(psu.ioqapi.pop(pop_token, networker_qd));
        return status;
    }

    int work(int status, dmtr_qresult_t &dequeued) {
        if (status) {
            log_error("StoreWorker work() received non-0 status %d", status);
            return -1;
        }
        assert(dequeued.qr_qd == networker_qd);
        assert(dequeued.qr_opcode == DMTR_OPC_POP);
        dmtr_sgarray_t &sga = dequeued.qr_value.sga;
        assert(sga.sga_numsegs == 1);
        auto kvreq = static_cast<KvRequest*>(sga.sga_segs[0].sgaseg_buf);
        assert(kvreq->sga.sga_numsegs == 1);
        std::string req((char*)kvreq->sga.sga_segs[0].sgaseg_buf,
                        kvreq->sga.sga_segs[0].sgaseg_len);
        log_debug("Received request %s", req.c_str());
        std::string resp;
        store.process_req(req, resp);

        KvResponse *kvr = new KvResponse(kvreq->req_qfd, resp);
        dmtr_sgarray_t sga_resp;
        as_sga(*kvr, sga_resp);
        DMTR_OK(blocking_push_to_peer(0, sga_resp));
        delete kvreq;
        free(sga.sga_buf);

        return 0;

    }

};


struct ServerOpts {
    std::string ip;
    uint16_t port;
    std::string cmd_file;
    int n_workers;
    std::string choice_fn;
    bool record_latencies;
    CommonOptions common;
};

namespace bpo = boost::program_options;

int parse_server_args(int argc, char **argv, ServerOpts &options) {
    bpo::options_description opts{"KV Server options"};
    opts.add_options()
                    ("ip",
                        bpo::value<std::string>(&options.ip)->default_value("127.0.0.1"),
                        "Server IP")
                    ("port",
                        bpo::value<uint16_t>(&options.port)->default_value(12345),
                        "Server port")
                    ("cmd-file",
                        bpo::value<std::string>(&options.cmd_file)->default_value(""),
                        "Initial commands")
                    ("workers,w",
                        bpo::value<int>(&options.n_workers)->default_value(1),
                        "Number of 'store' workers")
                    ("record-lat,r",
                        bpo::bool_switch(&options.record_latencies),
                        "Turn on latency recording")
                    ("strategy,s",
                        bpo::value<std::string>(&options.choice_fn)->default_value("RR"),
                        "Worker delegation strategy (RR or KEY)");
    return parse_args(argc, argv, opts, options.common);
}

int main(int argc, char **argv) {

    ServerOpts opts;
    if (parse_server_args(argc, argv, opts)) {
        return 1;
    }

    NetWorker::worker_choice choice_fn;
    if (opts.choice_fn == "RR") {
        choice_fn = NetWorker::RR;
    } else if (opts.choice_fn == "KEY") {
        choice_fn = NetWorker::KEY;
    } else {
        log_error("Unknown choice function '%s'", opts.choice_fn.c_str());
    }

    log_info("Launching kv store on %s:%u", opts.ip.c_str(), opts.port);

    struct sockaddr_in addr = {};
    addr.sin_family = AF_INET;
    if (inet_pton(AF_INET, opts.ip.c_str(), &addr.sin_addr) != 1) {
        log_error("Could not convert %s to ip", opts.ip.c_str());
        return -1;
    }
    addr.sin_port = htons(opts.port);

    std::string log_file;
    if (opts.record_latencies)
        log_file = opts.common.log_dir + "/net_traces";

    NetWorker n = NetWorker(addr, choice_fn, log_file);

    std::vector<PspWorker*> store_workers;
    KvStore store(opts.cmd_file);
    for (int i=0; i < opts.n_workers; i++) {
        store_workers.push_back(new StoreWorker(i+1, store));
        PspWorker::register_peers(n, *store_workers[i]);
    }

    auto sig_handler = [](int signal) {
        PspWorker::stop_all();
    };

    std::signal(SIGINT, sig_handler);
    std::signal(SIGTERM, sig_handler);

    bool failed_launch = (n.launch() != 0);
    for (auto w : store_workers) {
        if (w->launch()) {
            failed_launch = true;
            break;
        }
    }
    if (failed_launch) {
        PspWorker::stop_all();
    } else {
        bool stopped = false;
        while (!stopped) {
            if (n.has_exited()) {
                stopped = true;
                PspWorker::stop_all();
                break;
            }
            for (auto w : store_workers) {
                if (w->has_exited()) {
                    stopped = true;
                    PspWorker::stop_all();
                    break;
                }
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
        }
    }
    n.join();
    if (opts.record_latencies)
        n.dump_times();
    for (auto w : store_workers) {
        w->join();
        delete w;
    }

    log_info("Execution complete");
}
