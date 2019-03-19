// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/rdma/rdma-queue.h
 *   RDMA implementation of the dmtr io-queue interface
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTRDMAN OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTRDMAN WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef DMTR_LIBOS_RDMA_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_RDMA_QUEUE_HH_IS_INCLUDED

#include <libos/common/io_queue.hh>

#include <memory>
#include <queue>
#include <rdma/rdma_cma.h>
#include <unordered_set>

namespace dmtr {

class rdma_queue : public io_queue {
    private: static const size_t recv_buf_count;
    private: static const size_t recv_buf_size;
    private: static const size_t max_num_sge;

    private: struct metadata {
        dmtr_header_t header;
        uint32_t lengths[]; 
    };

    // queued scatter gather arrays
    private: std::queue<struct rdma_cm_id *> my_pending_accepts;
    private: std::queue<std::pair<void *, size_t>> my_pending_recvs;
    private: std::unordered_set<dmtr_qtoken_t> my_completed_sends;

    // rdma data structures
    // connection manager for this connection queue
    private: static struct ibv_pd *our_pd;
    private: struct rdma_cm_id *my_rdma_id = NULL;
    private: bool my_listening_flag;

    private: int service_event_queue();
    private: int service_completion_queue(struct ibv_cq * const cq, size_t quantity);
    private: int on_work_completed(const struct ibv_wc &wc);
    private: int setup_rdma_qp();

    private: rdma_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~rdma_queue();

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int new_qd);
    public: int connect(const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    private: static int rdma_bind_addr(struct rdma_cm_id * const id, const struct sockaddr * const addr);
    private: static int rdma_create_event_channel(struct rdma_event_channel *&channel_out);
    private: static int rdma_create_id(struct rdma_cm_id *&id_out, struct rdma_event_channel *channel, void *context, enum rdma_port_space ps);
    private: static int rdma_destroy_event_channel(struct rdma_event_channel *&channel);
    private: static int rdma_destroy_id(struct rdma_cm_id *&id);
    private: static int rdma_destroy_qp(struct rdma_cm_id * const id);
    private: static int rdma_listen(struct rdma_cm_id * const id, int backlog);
    private: static int rdma_resolve_addr(struct rdma_cm_id * const id, const struct sockaddr * const src_addr, const struct sockaddr * const dst_addr, int timeout_ms);
    private: static int rdma_get_cm_event(struct rdma_cm_event *&event_out, struct rdma_event_channel *channel);
    private: static int rdma_ack_cm_event(struct rdma_cm_event * const event);
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

    private: static int expect_rdma_cm_event(int err, enum rdma_cm_event_type expected, struct rdma_cm_id * const id);
    private: static int pin(const dmtr_sgarray_t &sga);
    private: static int unpin(const dmtr_sgarray_t &sga);
    private: int get_pd(struct ibv_pd *&pd_out);
    private: int get_rdma_mr(struct ibv_mr *&mr_out, const void * const p);
    private: int new_recv_buf();
    private: int service_recv_queue(void *&buf_out, size_t &len_out);
    private: int setup_recv_queue();
};

} // namespace dmtr

#endif /* DMTR_LIBOS_RDMA_QUEUE_HH_IS_INCLUDED */
