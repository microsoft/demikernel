#ifndef PSP_WORKER_H_
#define PSP_WORKER_H_

#include <dmtr/libos/io/persephone.hh>
#include <thread>

class PspWorker {
private:
    // Used primarily in Worker::stop_all()
    static std::unordered_map<int, PspWorker*> all_workers;

public:
    static int register_peers(PspWorker &a, PspWorker &b);
    static void stop_all();

private:
    bool started = false, exited = false, terminate = false;

    std::thread worker_thread;

    std::unordered_map<int, int> peer_qd_to_id, peer_id_to_qd;

    // Workers own their own input channels
    // A deque is necessary instead of a vector since shared_item has no copy ctor
    std::deque<dmtr::shared_item> input_channels;

    // Creates (and stores) a new input channel
    // Returns a reference to the stored channel
    dmtr::shared_item &generate_channel() { return input_channels.emplace_back(); }

    // Registers a channel from `peer` to this worker
    // (must be called in the other direction as well)
    int register_peer(PspWorker &peer,
                      dmtr::shared_item &peer_input,
                      dmtr::shared_item &peer_output);

    int main_loop();

    // The three functions to overload in child classes
    virtual int setup() { return 0; }
    virtual int dequeue(dmtr_qresult_t &dequeued) = 0;
    virtual int work(int status, dmtr_qresult_t &result) = 0;

protected:
    PspServiceUnit psu;
    int worker_id;
    std::vector<int> peer_ids;

    int get_peer_qd(int peer_id);
    int get_peer_id(int peer_qd);

    int blocking_push_to_peer(int peer_id, const dmtr_sgarray_t &sga);
    int pop_from_peer(int peer_id, dmtr_qtoken_t &token);

public:
    int join();
    int launch();
    bool has_exited() { return exited; }
    void stop() { terminate = true; }

    PspWorker(int id, dmtr::io_queue::category_id q_type, int argc=0, char **argv=NULL);
    virtual ~PspWorker();

};

#endif // PSP_WORKER_H_
