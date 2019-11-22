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
static void as_sga(T &from, dmtr_sgarray_t &sga) {
    sga.sga_buf = nullptr;
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_buf = &from;
    sga.sga_segs[0].sgaseg_len = sizeof(from);
}

using hr_clock = std::chrono::high_resolution_clock;

struct RequestTimes {
    int req_id;
    hr_clock::time_point entry;
    hr_clock::time_point exit;

    RequestTimes(int req_id, hr_clock::time_point entry, hr_clock::time_point exit) :
        req_id(req_id), entry(entry), exit(exit) {}
};

bool lat_cmp(const RequestTimes &a, const RequestTimes &b) {
    return (a.exit - a.entry) < (b.exit - b.entry);
}

bool time_cmp(const RequestTimes &a, const RequestTimes &b) {
    return (a.entry) < (b.entry);
}

void to_request_times(std::vector<hr_clock::time_point> entries, 
                      std::vector<hr_clock::time_point> exits,
                      std::vector<RequestTimes> out) {
    std::vector<RequestTimes> tmp;
    for (unsigned int i=0; i < entries.size(); i++) {
        tmp.emplace_back(i, entries[i], exits[i]);
    }
    sample_into(tmp, out, lat_cmp, time_cmp, 10000);
}

struct KvRequest {
    int req_qfd;
    dmtr_sgarray_t sga;

    const char *data;
    size_t data_len;
    const char *data_end;
    KvRequest(int qfd, dmtr_sgarray_t &sga) :
            req_qfd(qfd), sga(sga),
            data((char*)sga.sga_segs[0].sgaseg_buf),
            data_len(sga.sga_segs[0].sgaseg_len),
            data_end((char*)data + data_len) {}

    KvRequest(std::string &str) :
        data(str.c_str()),
        data_len(str.size()),
        data_end(str.c_str() + str.size()) {}

    // FIXME: is_reqtype must be called (with the proper reqtype) before the key is used
    // This isn't necessarily bad. Should be made more clear though
    const char *key = NULL;
    const char *key_end = NULL;

    bool is_reqtype(const char *type, size_t qlen) {
        if (strncmp(data, type, qlen) == 0) {
            key = data+qlen;
            key_end = (const char*)memchr(key, ' ', data_len - (qlen));
            if (key_end == NULL) {
                key_end = data + data_len;
            }
            return true;
        }
        return false;
    }


};

class KvResponse {
private:
    void *data;
    size_t data_size;
    bool data_valid = false;

public:
    int req_qfd;
    KvResponse(int req_qfd) :
            data_valid(false), req_qfd(req_qfd)
    {}

    void set_data(const std::string &resp) {
        data_valid = true;
        data = malloc(resp.size());
        data_size = resp.size();
        memcpy(data, resp.c_str(), data_size);
    }

    ~KvResponse() {
        if (data_valid) {
             free(data);
        }
    }

    int move_to_sga(dmtr_sgarray_t &sga) {
        if (!data_valid) {
            return -1;
        }
        sga.sga_buf = data;
        sga.sga_numsegs = 1;
        sga.sga_segs[0].sgaseg_buf = data;
        sga.sga_segs[0].sgaseg_len = data_size;
        data_valid = false;
        return 0;
    }

    // This may be more confusing than using set_data() explicitely,
    // but setting a KvResponse equal to a string just copies the data in
    KvResponse & operator=(const std::string &resp) {
        set_data(resp);
        return *this;
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

    static inline int fast_atoi( const char * str, const char *end)
    {
        int val = 0;
        while( *str != ' ' && str < end) {
            val = val*10 + (*str++ - '0');
        }
        return val;
    }

    int key_based_routing(dmtr_qresult_t &dequeued) {
        void *buf = dequeued.qr_value.sga.sga_segs[0].sgaseg_buf;
        char *req = static_cast<char*>(buf);
        char *req_end = req + dequeued.qr_value.sga.sga_segs[0].sgaseg_len;
        char *space;
        for (space=req; *space != ' ' && space < req_end; ++space) {};
        if (space == req_end) {
            // FIXME: This means all PUT requests go to the same server
            return peer_ids[0];
        }
        int idx = fast_atoi(space + 1, req + dequeued.qr_value.sga.sga_segs[0].sgaseg_len);
        return peer_ids[idx % peer_ids.size()];
    }


    int choose_worker(dmtr_qresult_t &dequeued) {
        switch (choice_fn) {
            case KEY:
                return key_based_routing(dequeued);
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
        std::vector<RequestTimes> req_times;
        to_request_times(entry_times, exit_times, req_times);


        std::ofstream logfile(log_filename);
        if (logfile.is_open()) {
            logfile << "id\tentry\texit" << std::endl;
            for (unsigned int i=0; i < req_times.size(); i++) {
                logfile << req_times[i].req_id << "\t"
                        << ns_since_start(req_times[i].entry) << "\t"
                        << ns_since_start(req_times[i].exit) << std::endl;
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

    ~NetWorker() {
        psu.ioqapi.close(lqd);
    }

    NetWorker(struct sockaddr_in &addr,
              worker_choice choice = RR,
              std::string log_filename = "",
              int argc = 0, char **argv = NULL) :
        PspWorker(0, dmtr::io_queue::NETWORK_Q, argc, argv),
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

    // Pre-calculating the length of the strings for the sake of optimization
    // (I don't think it made a huge difference)
    const char *PUT_STR = "PUT ";
    const size_t PUT_LEN = strlen(PUT_STR);
    const char *GET_STR = "GET ";
    const size_t GET_LEN = strlen(GET_STR);
    const char *SZOF_STR = "SZOF ";
    const size_t SZOF_LEN = strlen(SZOF_STR);
    const char *NNZ_STR = "NNZ ";
    const size_t NNZ_LEN = strlen(NNZ_STR);

    int process_put(KvRequest &input, KvResponse &output) {
        if (!writeable) {
            output = "ERR: Not writeable";
            return -1;
        }
        if (input.key_end == NULL) {
            output = "ERR: No KEY";
            return 0;
        }
        if (input.key_end >= input.data_end) {
            output = "ERR: No value";
            return 0;
        }

        // Make a copy of the key and value to store in the map.
        // Don't think this is avoidable
        std::string key(input.key, input.key_end);
        std::string value(input.key_end+1, input.data_end);
        store[key] = value;
        output = "SUCCESS";
        return 0;
    }

    int process_get(KvRequest &input, KvResponse &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }
        std::string key(input.key, input.key_end);
        auto it = store.find(key);
        if (it == store.end()) {
            output = "ERR: Bad key " + key;
            return -1;
        }
        output = it->second;
        return 0;
    }

    int process_szof(KvRequest &input, KvResponse &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }

        std::string key(input.key, input.key_end);
        auto it = store.find(key);
        if (it == store.end()) {
            output = "ERR: Bad key";
            return -1;
        }
        // Using strlen() rather than str::size so that it requires accessing the full string
        output = std::to_string(strlen(it->second.c_str()));
        return 0;
    }

    // This is really slow. Much slower than strlen 
    // (though in theory both are iterating over the string)
    int process_nnz(KvRequest &input, KvResponse &output) {
        if (!readable) {
            output = "ERR: Not readable";
            return -1;
        }

        std::string key(input.key, input.key_end);
        auto it = store.find(key);
        if (it == store.end()) {
            output = "ERR: Bad key";
            return -1;
        }
        int count = 0;
        for (const char c: it->second) {
            if (c != '0') {
                count++;
            }
        }
        output = std::to_string(count);
        return 0;
    }


public:

    int process_req(KvRequest &req, KvResponse &output) {
        if (req.is_reqtype(PUT_STR, PUT_LEN)) {
            return process_put(req, output);
        } else if (req.is_reqtype(GET_STR, GET_LEN)) {
            return process_get(req, output);
        } else if (req.is_reqtype(SZOF_STR, SZOF_LEN)) {
            return process_szof(req, output);
        } else if (req.is_reqtype(NNZ_STR, NNZ_LEN)) {
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
                KvRequest req(line);
                KvResponse output(0);
                if (process_req(req, output)) {
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

    int n_accesses = 0;

private:
    KvStore &store;
    bool record_lat;
    std::string log_filename_base;
    std::vector<hr_clock::time_point> entry_times;
    std::vector<hr_clock::time_point> exit_times;

    long int ns_since_start(hr_clock::time_point &tp) {
        return std::chrono::duration_cast<std::chrono::nanoseconds>(tp - entry_times[0]).count();
    }

public:

    int dump_times() {
        if (log_filename_base.size() == 0) {
            return 0;
        }
        std::vector<RequestTimes> req_times;
        to_request_times(entry_times, exit_times, req_times);


        std::string log_filename = log_filename_base + "_s" + std::to_string(worker_id);
        std::ofstream logfile(log_filename);
        if (logfile.is_open()) {
            logfile << "id\tentry\texit" << std::endl;
            for (unsigned int i=0; i < req_times.size(); i++) {
                logfile << req_times[i].req_id << "\t"
                        << ns_since_start(req_times[i].entry) << "\t"
                        << ns_since_start(req_times[i].exit) << std::endl;
            }
            logfile.close();
            log_info("Wrote net logs to %s", log_filename.c_str());
            return 0;
        } else {
            log_error("Coult not open logfile %s", log_filename.c_str());
            return -1;
        }
    }

    StoreWorker(int id, KvStore &store, std::string log_filename_base, int argc, char **argv) :
        PspWorker(id, dmtr::io_queue::SHARED_Q, argc, argv),
        store(store), record_lat(log_filename_base.size() > 0),
        log_filename_base(log_filename_base)
    {
        if (id == 0) {
            // RAISE WARNING
        }
        entry_times.reserve(5000000);
        exit_times.reserve(5000000);
    }
    ~StoreWorker() {
        std::cout << "Worker " << worker_id << " called " << n_accesses << " times" << std::endl;
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
        if (record_lat) {
            entry_times.push_back(hr_clock::now());
        }
        n_accesses++;
        assert(dequeued.qr_qd == networker_qd);
        assert(dequeued.qr_opcode == DMTR_OPC_POP);
        dmtr_sgarray_t &sga = dequeued.qr_value.sga;
        assert(sga.sga_numsegs == 1);
        auto kvreq = static_cast<KvRequest*>(sga.sga_segs[0].sgaseg_buf);
        assert(kvreq->sga.sga_numsegs == 1);
        std::string req((char*)kvreq->sga.sga_segs[0].sgaseg_buf,
                        kvreq->sga.sga_segs[0].sgaseg_len);
        log_debug("Received request %s", req.c_str());
        KvResponse *kvresp = new KvResponse(kvreq->req_qfd);
        store.process_req(*kvreq, *kvresp);

        dmtr_sgarray_t sga_resp;
        as_sga(*kvresp, sga_resp);
        DMTR_OK(blocking_push_to_peer(0, sga_resp));
        //free(kvreq->sga.sga_buf);
        dmtr_free_mbuf(&kvreq->sga);
        delete kvreq;

        if (record_lat) {
            exit_times.push_back(hr_clock::now());
        }
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
    bool record_store_latencies;
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
                    ("record-store-lat",
                        bpo::bool_switch(&options.record_store_latencies),
                        "Turn on store latency recording")
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
    std::string store_log_file;
    if (opts.record_latencies)
        log_file = opts.common.log_dir + "/net_traces";
    if (opts.record_store_latencies)
        store_log_file = opts.common.log_dir + "/store_traces";

    NetWorker n = NetWorker(addr, choice_fn, log_file, argc, argv);

    std::vector<StoreWorker*> store_workers;
    KvStore store(opts.cmd_file);
    for (int i=0; i < opts.n_workers; i++) {
        store_workers.push_back(new StoreWorker(i+1, store, store_log_file, argc, argv));
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
        if (opts.record_store_latencies) {
            w->dump_times();
        }
        delete w;
    }

    log_info("Execution complete");
}
