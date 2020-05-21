// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_RDMA_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_RDMA_QUEUE_HH_IS_INCLUDED

#include <boost/chrono.hpp>
#include <dmtr/libos/io_queue.hh>
#include <dmtr/libos/rdma/rdmacm_router.hh>
#include <memory>
#include <queue>
#include <rdma/rdma_cma.h>
#include <unordered_set>

namespace dmtr {

// We don't support dynamically allocated packet sizes yet, so this is
// our max packet size
#define RECV_BUF_SIZE 524288
//we allocate receive windows in 2 batches so we always have some to
// use while we re-allocate
#define RECV_WINDOW 1024
#define RECV_WINDOW_BATCHES 2
// how many entries to pull when polling the single shared completion queue
#define CQ_BATCH 5
// how often we are going to send a send completion signal. Make sure
// this is bigger than the receive window because we're going to use
// it to set the send wr queue size!
#define SEND_SIGNAL_FREQ 4096

class rdma_queue : public io_queue {
    public: typedef boost::chrono::steady_clock clock_type;
    public: typedef boost::chrono::duration<int32_t, boost::milli> duration_type;
    private: static const size_t max_num_sge;

    // my local receive buffer count and size
    private: size_t my_recv_window = 0;
    // how much can I send to the other side?
    private: size_t my_send_window = 0;
    private: size_t my_send_window_unused = 0;
    // how to reach the window on the other end of the connection
    private: uint64_t their_send_window_addr;
    private: uint32_t their_send_window_rkey;

    private: struct connection_data {
        uint64_t send_window_addr;
        uint32_t send_window_rkey;
    };
    
    private: struct metadata {
        dmtr_header_t header;
        uint32_t lengths[];
    };
    
    private: uint64_t in_packets = 0;
    private: uint64_t out_packets = 0;

    // queued scatter gather arrays
    private: std::queue<struct rdma_cm_event *> my_pending_accepts;
    private: std::queue<std::pair<void *, size_t>> my_pending_recvs;
    private: std::unordered_set<dmtr_qtoken_t> my_completed_sends;
    private: clock_type::time_point my_last_event_channel_poll;

    // rdma data structures
    // connection manager for this connection queue
    private: static struct ibv_pd *our_pd;
    private: static std::unique_ptr<rdmacm_router> our_rdmacm_router;
    private: struct rdma_cm_id *my_rdma_id;
    private: bool my_listening_flag;
    private: struct ibv_cq * my_cq = NULL;
    private: std::unique_ptr<task::thread_type> my_accept_thread;
    private: std::unique_ptr<task::thread_type> my_push_thread;
    private: std::unique_ptr<task::thread_type> my_pop_thread;
    private: std::unique_ptr<task::thread_type> my_connect_thread;

    private: int service_event_channel();
    private: int service_completion_queue(struct ibv_cq * const cq, size_t quantity);
    private: int on_work_completed(const struct ibv_wc &wc);
    private: int setup_rdma_qp();
    private: int submit_io(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);

    public: rdma_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~rdma_queue();
    public: static int init_rdma();
    // network functions
    public: virtual int socket(int domain, int type, int protocol);
    public: virtual int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: virtual int listen(int backlog);
    public: virtual int bind(const struct sockaddr * const saddr, socklen_t size);
    public: virtual int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int new_qd);
    public: virtual int connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size);
    public: virtual int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    private: static int rdma_bind_addr(struct rdma_cm_id * const id, const struct sockaddr * const addr);
    private: static int rdma_destroy_qp(struct rdma_cm_id * const id);
    private: static int rdma_disconnect(struct rdma_cm_id * const id);
    private: static int rdma_listen(struct rdma_cm_id * const id, int backlog);
    private: static int rdma_resolve_addr(struct rdma_cm_id * const id, const struct sockaddr * const src_addr, const struct sockaddr * const dst_addr, int timeout_ms);
    private: static int rdma_resolve_route(struct rdma_cm_id * const id, int timeout_ms);
    private: static int rdma_connect(struct rdma_cm_id * const id, struct rdma_conn_param * const conn_param);
    private: static int rdma_create_qp(struct rdma_cm_id * const id, struct ibv_pd * const pd, struct ibv_qp_init_attr * const qp_init_attr);
    private: static int rdma_accept(struct rdma_cm_id * const id, struct rdma_conn_param * const conn_param);
    private: static int rdma_get_peer_addr(struct sockaddr *&saddr_out, struct rdma_cm_id * const id);

    private: static int ibv_alloc_pd(struct ibv_pd *&pd_out, struct ibv_context *context);
    private: static int ibv_dealloc_pd(struct ibv_pd *&pd);
    private: static int ibv_poll_cq(size_t &count_out, struct ibv_cq * const cq, int num_entries, struct ibv_wc * const wc);
    private: static int ibv_post_send(struct ibv_send_wr *&bad_wr_out, struct ibv_qp * const qp, struct ibv_send_wr * const wr);
    private: static int ibv_post_recv(struct ibv_recv_wr *&bad_wr_out, struct ibv_qp * const qp, struct ibv_recv_wr * const wr);

    private: static int getsockname(int sockfd, struct sockaddr *saddr, socklen_t &addrlen);

private: static int expect_rdma_cm_event(int err, enum rdma_cm_event_type expected, struct rdma_cm_id * const id, duration_type timeout, struct rdma_cm_event **e = NULL);
    private: static int pin(const dmtr_sgarray_t &sga);
    private: static int unpin(const dmtr_sgarray_t &sga);
    private: int get_pd(struct ibv_pd *&pd_out);
    private: static void close_pd();
    private: int get_rdma_mr(struct ibv_mr *&mr_out, const void * const p);
    private: int new_recv_bufs(size_t n);
private: int update_remote_window(size_t n);
    private: int service_recv_queue(void *&buf_out, size_t &len_out);
    private: int setup_recv_queue();
    private: int setup_recv_window(struct connection_data &cd);
    private: void start_threads();
    private: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int connect_thread(task::thread_type::yield_type &yield, dmtr_qtoken_t qt, duration_type timeout);
    private: int connect(task::thread_type::yield_type &yield, dmtr_qtoken_t qt, duration_type timeout);

    private: bool good() const {
        return my_rdma_id != NULL;
    }
};

} // namespace dmtr

#endif /* DMTR_LIBOS_RDMA_QUEUE_HH_IS_INCLUDED */
