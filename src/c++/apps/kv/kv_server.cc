#include <string>
#include <iostream>
#include <fstream>
#include <thread>
#include <csignal>

#include <arpa/inet.h>

#include <boost/program_options.hpp>

#include <dmtr/libos.h>
#include <dmtr/libos/persephone.hh>

#include "logging.h"


class Worker {
private:
    static std::unordered_map<int, Worker*> all_workers;

protected:
    PspServiceUnit psu;
    std::vector<int> peer_ids;

private:
    int id;

    bool terminate = false;

    bool launched = false;
    bool exited = false;
    bool started = false;
    int rtn_code;
    std::thread thread;

    std::unordered_map<int, int> peer_qd_to_id;
    std::unordered_map<int, int> peer_id_to_qd;

    std::deque<dmtr::shared_item> input_channels;

    dmtr::shared_item &generate_channel() {
        return input_channels.emplace_back();
    }


    int register_peer(Worker &peer, dmtr::shared_item &peer_in, dmtr::shared_item &peer_out) {
        int peer_qd;
        DMTR_OK(psu.ioqapi.shared_queue(peer_qd, &peer_out, &peer_in));
        log_debug("Worker %d : peer %d is at qd %d", id, peer.id, peer_qd);
        peer_id_to_qd[peer.id] = peer_qd;
        peer_qd_to_id[peer_qd] = peer.id;
        log_debug("Worker %d Pushing back %d", id, peer.id);
        peer_ids.push_back(peer.id);
        return 0;
    }

    virtual int setup() { return 0; };
    virtual int dequeue(dmtr_qresult_t &dequeued) = 0;
    virtual int work(int status, dmtr_qresult_t &result) = 0;

    int run(void) {
        if (setup()) {
            log_error("Worker thread %d failed to initialize properly", id);
            return -1;
        }
        started = true;
        log_info("Worker thread %d started", id);
        while (!terminate) {
            dmtr_qresult_t dequeued;
            int status = dequeue(dequeued);
            if (status == EAGAIN) {
                continue;
            }
            DMTR_OK(work(status, dequeued));
        }
        return 0;
    }

    void run_wrapper(void) {
        rtn_code = run();
        exited = true;
        log_info("Worker thread %d terminating", id);
    }

protected:
    int get_peer_qd(int peer_id) {
        auto it = peer_id_to_qd.find(peer_id);
        if (it == peer_id_to_qd.end()) {
            return -1;
        }
        log_debug("%d: %d", peer_id, it->second);
        return it->second;
    }


    int get_peer_id(int peer_qd) {
        auto it = peer_qd_to_id.find(peer_qd);
        if (it == peer_qd_to_id.end()) {
            return -1;
        }
        return it->second;
    }

    int push_to_peer(int peer_id, dmtr_sgarray_t &sga) {
        auto it = peer_id_to_qd.find(peer_id);
        if (it == peer_id_to_qd.end()) {
            return -1;
        }
        dmtr_qtoken_t token;
        DMTR_OK(psu.ioqapi.push(token, it->second, sga));
        DMTR_OK(psu.wait(NULL, token));
        log_debug("Pushed from %d to %d", id, peer_id);
        return 0;
    }

    int pop_from_peer(int peer_id, dmtr_qtoken_t &token) {
        auto it = peer_id_to_qd.find(peer_id);
        if (it == peer_id_to_qd.end()) {
            return -1;
        }
        DMTR_OK(psu.ioqapi.pop(token, it->second));
        return 0;
    }

public:
    Worker(int id, dmtr::io_queue::category_id q_type) :
            psu(id, q_type, 0, NULL), id(id)  {
        if (all_workers.find(id) != all_workers.end()) {
            // RAISE WARNING
        }
        all_workers[id] = this;
    }

    virtual ~Worker() {
        if (thread.joinable()) {
            thread.join();
        }
        auto it = all_workers.find(id);
        if (it == all_workers.end()) {
            // RAISE WARNING
        }
        all_workers.erase(it);
    }

    int join() {
        if (thread.joinable()) {
            thread.join();
            return 0;
        }
        return -1;
    }

    int launch() {
        if (launched) {
            log_error("Cannot launch worker a second time");
            return -1;
        }
        launched = true;
        thread = std::thread(&Worker::run_wrapper, this);
        while (!started && !exited) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        log_debug("Thread %d launched", id);
        if (exited && !started) {
            return -1;
        }
        return 0;
    }

    bool has_exited() {
        return exited;
    }

    void stop() {
        log_debug("Terminating worker %d", id);
        terminate = true;
    }

    static int register_peers(Worker &a, Worker &b) {
        auto &a_input = a.generate_channel();
        auto &b_input = b.generate_channel();
        DMTR_OK(a.register_peer(b, b_input, a_input));
        DMTR_OK(b.register_peer(a, a_input, b_input));
        return 0;
    }

    static void stop_all() {
        log_debug("Stopping all workers");
        for (auto w : all_workers) {
            w.second->stop();
        }
    }
};

std::unordered_map<int, Worker*> Worker::all_workers;

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

class NetWorker : public Worker {

    struct sockaddr_in bind_addr;

    int lqd;
    std::vector<dmtr_qtoken_t> tokens;

    int start_offset = 0;

    int worker_offset = 0;

    virtual int choose_worker(dmtr_qresult_t &dequeued) {
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

public:
    NetWorker(struct sockaddr_in &addr) :
            Worker(0, dmtr::io_queue::NETWORK_Q),
            bind_addr(addr)
    {
    }

    int setup() {
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
        if (status == 0 || status == ECONNABORTED) {
            tokens.erase(tokens.begin() + idx);
            if (status == ECONNABORTED) {
                return EAGAIN;
            }
        } else {
            log_warn("wait_any returned non-0 exit code %d", status);
        }

        return status;
    }

    int work(int status, dmtr_qresult_t &dequeued) {
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
            log_debug("Received PUSH code");
            return 0;
        }
        log_debug("Received POP code");
        assert(DMTR_OPC_POP == dequeued.qr_opcode);
        int dequeued_id = get_peer_id(dequeued.qr_qd);
        if (dequeued_id == -1) {
            // New request
            int new_worker_id = choose_worker(dequeued);
            KvRequest *kvr = new KvRequest(dequeued.qr_qd, dequeued.qr_value.sga);
            dmtr_sgarray_t sga_req;
            as_sga(*kvr, sga_req);
            log_debug("NetWorker pushing to peer %d", new_worker_id);
            push_to_peer(new_worker_id, sga_req);

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
        // Making copy so it will take up space;
        std::string val_cp = it->second;
        output = std::to_string(val_cp.size());
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

class StoreWorker : public Worker {

    int networker_qd;
    dmtr_qtoken_t pop_token;

private:
    KvStore &store;

public:
    StoreWorker(int id, KvStore &store) :
            Worker(id, dmtr::io_queue::SHARED_Q),
            store(store) {
        if (id == 0) {
            // RAISE WARNING
        }
    }

    int setup() {
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
        log_debug("Got non-EAGAIN");
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
        DMTR_OK(push_to_peer(0, sga_resp));
        delete kvreq;

        return 0;

    }

};

namespace boost_opts = boost::program_options;

struct ArgumentOpts {
    std::string ip;
    uint16_t port;
    std::string cmd_file;
    std::string log_dir;
    int n_workers;

};

int parse_args(int argc, char **argv, ArgumentOpts &options) {
    boost_opts::options_description opts{"KV Server options"};
    opts.add_options()
                    ("help", "produce help message")
                    ("ip",
                        boost_opts::value<std::string>(&options.ip)->default_value("127.0.0.1"),
                        "Server IP")
                    ("port",
                        boost_opts::value<uint16_t>(&options.port)->default_value(12345),
                        "Server port")
                    ("cmd-file",
                        boost_opts::value<std::string>(&options.cmd_file)->default_value(""),
                        "Initial commands")
                    ("log-dir,L",
                         boost_opts::value<std::string>(&options.log_dir)->default_value("./"),
                        "experiment log directory")
                    ("workers,w", boost_opts::value<int>(&options.n_workers)->default_value(1));

    boost_opts::variables_map vm;
    try {
        boost_opts::parsed_options parsed =
            boost_opts::command_line_parser(argc, argv).options(opts).run();
        boost_opts::store(parsed, vm);
        if (vm.count("help")) {
            std::cout << opts << std::endl;
            exit(0);
        }
        boost_opts::notify(vm);
    } catch (const boost_opts::error &e) {
        std::cerr << e.what() << std::endl;
        std::cerr << opts << std::endl;
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {

    ArgumentOpts opts;
    if (parse_args(argc, argv, opts)) {
        return 1;
    }

    log_info("Launching kv store on %s:%u", opts.ip.c_str(), opts.port);

    struct sockaddr_in addr = {};
    addr.sin_family = AF_INET;
    if (inet_pton(AF_INET, opts.ip.c_str(), &addr.sin_addr) != 1) {
        log_error("Could not convert %s to ip", opts.ip.c_str());
        return -1;
    }
    addr.sin_port = htons(opts.port);

    NetWorker n(addr);
    KvStore store(opts.cmd_file);
    StoreWorker w(1, store);

    std::vector<Worker*> workers = {&n, &w};

    auto sig_handler = [](int signal) {
        Worker::stop_all();
    };

    std::signal(SIGINT, sig_handler);
    std::signal(SIGTERM, sig_handler);

    Worker::register_peers(n, w);

    bool failed_launch = false;
    for (auto w : workers) {
        if (w->launch()) {
            failed_launch = true;
            break;
        }
    }
    if (failed_launch) {
        Worker::stop_all();
    } else {
        bool stopped = false;
        while (!stopped) {
            for (auto w : workers) {
                if (w->has_exited()) {
                    stopped = true;
                    Worker::stop_all();
                    break;
                }
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
        }
    }
    for (auto w : workers) {
        w->join();
    }

    log_info("Execution complete");
}
