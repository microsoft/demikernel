#include <string>
#include <iostream>
#include <fstream>
#include <dmtr/libos.h>

#include <dmtr/libos/persephone.hh>
#include <thread>
#include <signal.h>


#include <boost/program_options.hpp>

struct ArgumentOpts {
    std::string ip;
    uint16_t port;
    std::string log_dir;
    int n_workers;
};

class Worker {

protected:
    PspServiceUnit psu;
    std::vector<int> peer_ids;

private:
    int id;

    bool terminate = false;

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


    void register_peer(Worker &peer, dmtr::shared_item &peer_in, dmtr::shared_item &peer_out) {
        int peer_qd;
        psu.shared_queue(peer_qd, &peer_in, &peer_out);
        peer_id_to_qd[peer.id] = peer_qd;
        peer_qd_to_id[peer_qd] = peer.id;
        peer_ids.push_back(peer.id);
    }

    virtual int setup() { return 0;};
    virtual int dequeue(dmtr_qresult_t &dequeued) = 0;
    virtual int work(int status, dmtr_qresult_t &result) = 0;

    int run(void) {
        DMTR_OK(setup());
        started = true;
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
    }

protected:
    int get_peer_qd(int peer_id) {
        auto it = peer_id_to_qd.find(peer_id);
        if (it == peer_id_to_qd.end()) {
            return -1;
        }
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
        return 0;
    }

public:
    Worker(int id, dmtr::io_queue::category_id q_type) :
            psu(id, q_type, 0, NULL), id(id)  {
    }

    virtual ~Worker() {
        if (thread.joinable()) {
            thread.join();
        }
    }

    int launch(void) {
        thread = std::thread(&Worker::run_wrapper, this);
        while (!started && !exited) {}
        if (exited && !started) {
            return -1;
        }
        return 0;
    }

    static void register_peers(Worker &a, Worker &b) {
        auto &a_input = a.generate_channel();
        auto &b_input = b.generate_channel();
        a.register_peer(b, b_input, a_input);
        b.register_peer(a, a_input, b_input);
    }
};

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

class NetWorker : Worker {

    struct sockaddr_in bind_addr;

    int lqd;
    std::vector<dmtr_qtoken_t> tokens;

    int start_offset = 0;

    int worker_offset = 0;

    virtual int choose_worker(dmtr_qresult_t &dequeued) {
        int n_peers = peer_ids.size();
        if (n_peers == 0) {
            return -1;
        }
        if (worker_offset++ >= n_peers) {
            worker_offset = 0;
        }
        return *(peer_ids.begin() + worker_offset);
    }

public:
    NetWorker(struct sockaddr_in &addr) :
            Worker(0, dmtr::io_queue::NETWORK_Q),
            bind_addr(addr)
    {
        psu.ioqapi.socket(lqd, AF_INET, SOCK_STREAM, 0);
    }

    int setup() {
        DMTR_OK(psu.ioqapi.bind(lqd,
                                reinterpret_cast<struct sockaddr*>(&bind_addr),
                                sizeof(bind_addr)));
        dmtr_qtoken_t token;
        DMTR_OK(psu.ioqapi.listen(lqd, 100));
        DMTR_OK(psu.ioqapi.accept(token, lqd));
        tokens.push_back(token);
        return 0;
    }

    int dequeue(dmtr_qresult_t &dequeued) {
        int idx;
        int status = psu.wait_any(&dequeued, &start_offset, &idx, tokens.data(), tokens.size());
        if (status == EAGAIN) {
            return EAGAIN;
        }
        tokens.erase(tokens.begin() + idx);
        return status;
    }

    int work(int status, dmtr_qresult_t &dequeued) {
        if (dequeued.qr_qd == lqd) {
            assert(dequeued.qr_opcode == DMTR_OPC_ACCEPT);
            dmtr_qtoken_t token;
            DMTR_OK(psu.ioqapi.pop(token, dequeued.qr_value.ares.qd));
            tokens.push_back(token);
            return 0;
        }
        if (dequeued.qr_opcode == DMTR_OPC_PUSH) {
            return 0;
        }
        assert(DMTR_OPC_POP == dequeued.qr_opcode);
        int dequeued_id = get_peer_id(dequeued.qr_qd);
        if (dequeued_id == -1) {
            // New request
            int new_worker_id = choose_worker(dequeued);
            KvRequest *kvr = new KvRequest(dequeued_id, dequeued.qr_value.sga);
            dmtr_sgarray_t sga_req;
            as_sga(*kvr, sga_req);
            push_to_peer(new_worker_id, sga_req);
        } else {
            // Returned frozam peer
            dmtr_sgarray_t &sga = dequeued.qr_value.sga;
            assert(sga.sga_numsegs == 1 && sga.sga_segs[0].sgaseg_len == sizeof(KvResponse));

            auto resp = static_cast<KvResponse*>(sga.sga_segs[0].sgaseg_buf);
            dmtr_qtoken_t token;

            dmtr_sgarray_t resp_sga;
            resp->move_to_sga(resp_sga);
            DMTR_OK(psu.ioqapi.push(token, resp->req_qfd, resp_sga));
            if (psu.wait(NULL, token) == EAGAIN) {
                tokens.push_back(token);
            }
            delete resp;
        }
        return -1;
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
        store[req.substr(PUT_STR.size()+1, keylen)] = req.substr(key_end, vallen);
        output = "SUCCESS";
        return 0;
    }

    int process_get(const std::string &req, std::string &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }

        if (req.find_first_of(" ", GET_STR.size() + 1) != std::string::npos) {
            output = "ERR: Key contains space";
            return -1;
        }

        size_t keylen = req.size() - GET_STR.size();
        auto it = store.find(req.substr(GET_STR.size()+1, keylen));
        if (it == store.end()) {
            output = "ERR: Bad key";
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
        auto it = store.find(req.substr(SZOF_STR.size() + 1, keylen));
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
                    // RAISE WARNING
                }
            }
            input_file.close();
        }
        writeable = false;
        readable = true;
    }
};

class StoreWorker : Worker {

    int networker_qd;
    dmtr_qtoken_t pop_token;

private:
    KvStore &store;

public:
    StoreWorker(int id, KvStore &store) :
            Worker(id, dmtr::io_queue::SHARED_Q),
            store(store) {}

    int setup() {
        assert((networker_qd = get_peer_qd(0)) != -1);
        DMTR_OK(psu.ioqapi.pop(pop_token, networker_qd));
        return 0;
    }

    int dequeue(dmtr_qresult_t &dequeued) {
        int status = psu.wait(&dequeued, pop_token);
        if (status == EAGAIN) {
            return EAGAIN;
        }
        DMTR_OK(psu.ioqapi.pop(pop_token, networker_qd));
        return status;
    }

    int work(int status, dmtr_qresult_t &dequeued) {
        assert(dequeued.qr_qd == networker_qd);
        assert(dequeued.qr_opcode == DMTR_OPC_POP);
        dmtr_sgarray_t &sga = dequeued.qr_value.sga;
        assert(sga.sga_numsegs == 1);
        auto kvreq = static_cast<KvRequest*>(sga.sga_segs[0].sgaseg_buf);
        assert(kvreq->sga.sga_numsegs == 1);
        std::string req((char*)kvreq->sga.sga_segs[0].sgaseg_buf,
                        kvreq->sga.sga_segs[0].sgaseg_len);
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

int parse_args(int argc, char **argv, ArgumentOpts &options) {
    boost_opts::options_description opts{"KV Server options"};
    opts.add_options()
                    ("help", "produce help message")
                    ("ip", boost_opts::value<std::string>(&options.ip), "Server IP")
                    ("port", boost_opts::value<uint16_t>(&options.port), "Server port")
                    ("log-dir,L",
                         boost_opts::value<std::string>(&options.log_dir)->default_value("./"),
                        "experiment log directory")
                    ("workers,w", boost_opts::value<int>(&options.n_workers)->default_value(0));

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

    struct sockaddr_in  addr;
    NetWorker n(addr);
    /*
    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";
    if (signal(SIGTERM, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGTERM\n";
        */
}
