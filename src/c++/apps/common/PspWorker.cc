#include "PspWorker.hh"
#include "common.hh"

std::unordered_map<int, PspWorker*> PspWorker::all_workers;

int PspWorker::register_peers(PspWorker &a, PspWorker &b) {
    auto &a_input = a.generate_channel();
    auto &b_input = b.generate_channel();
    DMTR_OK(a.register_peer(b, b_input, a_input));
    DMTR_OK(b.register_peer(a, a_input, b_input));
    return 0;
}

void PspWorker::stop_all() {
    PSP_INFO("Stopping all");
    for (auto w : all_workers) {
        w.second->stop();
    }
}

int PspWorker::register_peer(PspWorker &peer,
                             dmtr::shared_item &peer_in,
                             dmtr::shared_item &peer_out) {
    int peer_qd;
    DMTR_OK(psu->ioqapi.shared_queue(peer_qd, &peer_out, &peer_in));
    peer_id_to_qd[peer.worker_id] = peer_qd;
    peer_qd_to_id[peer_qd] = peer.worker_id;
    peer_ids.push_back(peer.worker_id);
    return 0;
}

//TODO enable the main_loop to call a clean-up function
int PspWorker::main_loop() {
    if (setup()) {
        PSP_ERROR("Worker thread " << worker_id << " failed to initialize")
        exited = true;
        return -1;
    }
    started = true;
    PSP_INFO("Worker thread " << worker_id << " started");
    while (!terminate) {
        dmtr_qresult_t dequeued;
        int status = dequeue(dequeued);
        if (status == EAGAIN) {
            continue;
        }
        DMTR_OK(work(status, dequeued));
    }
    exited = true;
    PSP_INFO("Worker thread " << worker_id << " terminating")
    return 0;
}

int PspWorker::get_peer_qd(int peer_id) {
    auto it = peer_id_to_qd.find(peer_id);
    if (it == peer_id_to_qd.end()) {
        return -1;
    }
    return it->second;
}

int PspWorker::get_peer_id(int peer_qd) {
    auto it = peer_qd_to_id.find(peer_qd);
    if (it == peer_qd_to_id.end()) {
        return -1;
    }
    return it->second;
}

int PspWorker::blocking_push_to_peer(const dmtr_sgarray_t &sga, int qd) {
    dmtr_qtoken_t token;
    DMTR_OK(psu->ioqapi.push(token, qd, sga));
    while (psu->wait(NULL, token) == EAGAIN && !terminate) {};
    return 0;
}

int PspWorker::blocking_push_to_peer(int peer_id, const dmtr_sgarray_t &sga) {
    int qd = get_peer_qd(peer_id);
    if (qd == -1) {
        return -1;
    }
    dmtr_qtoken_t token;
    DMTR_OK(psu->ioqapi.push(token, qd, sga));
    while (psu->wait(NULL, token) == EAGAIN && !terminate) {};
    return 0;
}

int PspWorker::pop_from_peer(int peer_id, dmtr_qtoken_t &token) {
    int qd = get_peer_qd(peer_id);
    if (qd == -1) {
        return -1;
    }
    DMTR_OK(psu->ioqapi.pop(token, qd));
    return 0;
}

int PspWorker::join() {
    if (worker_thread.joinable()) {
        worker_thread.join();
        return 0;
    }
    return -1;
}

int PspWorker::launch() {
    if (worker_thread.joinable()) {
        PSP_ERROR("Cannot launch thread a second time");
        return -1;
    }
    worker_thread = std::thread(&PspWorker::main_loop, this);
    // Wait for the thread to start
    while (!started && !exited) {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (exited && !started) {
        return -1;
    }
    return 0;
}

PspWorker::PspWorker(int id, PspServiceUnit *psu) : psu(psu), worker_id(id) {
    if (all_workers.find(id) != all_workers.end()) {
        PSP_WARN("Worker with id " << worker_id << "already exists");
    }
    all_workers[id] = this;
}

PspWorker::~PspWorker() {
    if (worker_thread.joinable()) {
        worker_thread.join();
    }
    auto it = all_workers.find(worker_id);
    if (it == all_workers.end()) {
        PSP_WARN("Worker id " << worker_id << "double-registered");
    } else {
        all_workers.erase(worker_id);
    }
}
