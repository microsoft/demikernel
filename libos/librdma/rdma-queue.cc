// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/rdma/rdma-queue.cc
 *   RDMA implementation of dmtr queue interface
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
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#include "rdma-queue.h"

#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/mem.h>
#include <hoard/zeusrdma.h>
#include <libos/common/latency.h>

#include <arpa/inet.h>
#include <cassert>
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <netinet/tcp.h>
#include <rdma/rdma_verbs.h>
#include <sys/uio.h>
#include <unistd.h>

DEFINE_LATENCY(post_send);
DEFINE_LATENCY(get_mr);
DEFINE_LATENCY(rdma_wait_latency);
DEFINE_LATENCY(poll_sendcq_latency);
DEFINE_LATENCY(poll_recvcq_latency);
DEFINE_LATENCY(poll_eventcq_latency);
DEFINE_LATENCY(rdma_parse_latency);

struct ibv_pd *dmtr::rdma_queue::our_pd = NULL;
const size_t dmtr::rdma_queue::recv_buf_size = 1024;
const size_t dmtr::rdma_queue::max_num_sge = DMTR_SGARRAY_MAXSIZE;

dmtr::rdma_queue::task::task() :
    pull(false),
    done(false),
    error(0),
    header{},
    sga{}
{}

dmtr::rdma_queue::rdma_queue(int qd) :
    io_queue(NETWORK_Q, qd),
    my_listening_flag(false)
{}

int dmtr::rdma_queue::new_object(io_queue *&q_out, int qd) {
    q_out = new rdma_queue(qd);
    return 0;
}

dmtr::rdma_queue::~rdma_queue()
{}

int dmtr::rdma_queue::setup_rdma_qp()
{
    DMTR_TRUE(EPERM, !my_listening_flag);

    // obtain the protection domain
    struct ibv_pd *pd = NULL;
    DMTR_OK(get_pd(pd));
    my_rdma_id->pd = pd;

    // set up connection queue pairs
    struct ibv_qp_init_attr qp_attr = {};
    qp_attr.qp_type = IBV_QPT_RC;
    qp_attr.cap.max_send_wr = 20;
    qp_attr.cap.max_recv_wr = 20;
    qp_attr.cap.max_send_sge = max_num_sge;
    qp_attr.cap.max_recv_sge = max_num_sge;
    qp_attr.cap.max_inline_data = 64;
    qp_attr.sq_sig_all = 1;
    DMTR_OK(rdma_create_qp(my_rdma_id, pd, &qp_attr));
    DMTR_OK(set_non_blocking(my_rdma_id->send_cq_channel->fd));
    DMTR_OK(set_non_blocking(my_rdma_id->recv_cq_channel->fd));
    return 0;
}

int dmtr::rdma_queue::on_work_completed(const struct ibv_wc &wc)
{
    bool pull = false;
    switch (wc.opcode) {
        default:
            fprintf(stderr, "Unexpected WC opcode: 0x%x\n", wc.opcode);
            return ENOTSUP;
        case IBV_WC_RECV:
            pull = true;
            break;
        case IBV_WC_SEND:
            pull = false;
            break;
    }

    DMTR_TRUE(ENOTSUP, IBV_WC_SUCCESS == wc.status);
    dmtr_qtoken_t qt = wc.wr_id;
    auto it = my_tasks.find(qt);
    DMTR_TRUE(ENOTSUP, it != my_tasks.cend());
    task * const t = it->second;
    t->byte_len = wc.byte_len;
    DMTR_TRUE(ENOTSUP, pull == t->pull);
    if (pull) {
        DMTR_OK(on_recv_completed(*t));
        return 0;
    }

    t->done = true;
    t->error = 0;
    return 0;
}

int dmtr::rdma_queue::service_completion_queue(struct ibv_cq * const cq, size_t quantity) {
    DMTR_NOTNULL(cq);
    DMTR_TRUE(EINVAL, quantity > 0);

    // check completion queue
    struct ibv_wc wc[quantity];
    size_t count = 0;
    Latency_Start(&poll_recvcq_latency);
    DMTR_OK(ibv_poll_cq(count, cq, quantity, wc));
    //fprintf(stderr, "Found receive work completions: %d\n", num);
    // process messages
    for (size_t i = 0; i < count; ++i) {
        on_work_completed(wc[i]);
    }
    Latency_End(&poll_recvcq_latency);
    //fprintf(stderr, "Done draining completion queue\n");

    return 0;
}

int dmtr::rdma_queue::service_event_queue() {
    DMTR_NOTNULL(my_rdma_id);
    DMTR_TRUE(EPERM, fcntl(my_rdma_id->channel->fd, F_GETFL) & O_NONBLOCK);

    Latency_Start(&poll_eventcq_latency);
    struct rdma_cm_event event = {};
    {
        struct rdma_cm_event *e = NULL;
        int ret = rdma_get_cm_event(e, my_rdma_id->channel);
        switch (ret) {
            default:
                DMTR_OK(ret);
            case 0:
                break;
            case EAGAIN:
                return EAGAIN;
        }

        event = *e;
        rdma_ack_cm_event(e);
    }

    switch(event.event) {
        default:
            fprintf(stderr, "Unrecognized event: 0x%x\n", event.event);
            return ENOTSUP;
        case RDMA_CM_EVENT_CONNECT_REQUEST:
            my_accepts.push(event.id);
            fprintf(stderr, "Event: RDMA_CM_EVENT_CONNECT_REQUEST\n");
            break;
        case RDMA_CM_EVENT_DISCONNECTED:
            fprintf(stderr, "Event: RDMA_CM_EVENT_DISCONNECTED\n");
            DMTR_OK(close());
            return ECONNABORTED;
        case RDMA_CM_EVENT_ESTABLISHED:
            fprintf(stderr, "Event: RDMA_CM_EVENT_ESTABLISHED\n");
            break;
    }

    Latency_End(&poll_eventcq_latency);
    return 0;
}

int dmtr::rdma_queue::socket(int domain, int type, int protocol)
{
    DMTR_NULL(my_rdma_id);

    struct rdma_event_channel *channel = NULL;
    DMTR_OK(rdma_create_event_channel(channel));

    switch (type) {
        default:
            return ENOTSUP;
        case SOCK_STREAM:
            DMTR_OK(rdma_create_id(my_rdma_id, channel, NULL, RDMA_PS_TCP));
            return 0;
        case SOCK_DGRAM:
            DMTR_OK(rdma_create_id(my_rdma_id, channel, NULL, RDMA_PS_UDP));
            return 0;
    }
}

int dmtr::rdma_queue::bind(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_NOTNULL(my_rdma_id);

    DMTR_OK(rdma_bind_addr(my_rdma_id, saddr));
    return 0;

}

int dmtr::rdma_queue::accept(io_queue *&q_out, struct sockaddr * const saddr, socklen_t * const addrlen, int new_qd)
{
    int ret = accept2(q_out, saddr, addrlen, new_qd);
    if (0 == ret) {
        return 0;
    }

    if (q_out != NULL) {
        delete q_out;
        q_out = NULL;
    }

    return ret;
}

int dmtr::rdma_queue::accept2(io_queue *&q_out, struct sockaddr * const saddr, socklen_t * const addrlen, int new_qd) {
    DMTR_NOTNULL(my_rdma_id);
    DMTR_TRUE(EPERM, my_listening_flag);

    struct rdma_cm_id *new_rdma_id = NULL;
    int ret = pop_accept(new_rdma_id);
    switch (ret) {
        default:
            DMTR_OK(ret);
        case 0:
            break;
        case EAGAIN:
            return EAGAIN;
    }

    auto * const q = new rdma_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);
    q_out = q;
    q->my_rdma_id = new_rdma_id;
    DMTR_OK(set_non_blocking(new_rdma_id->channel->fd));
    DMTR_OK(q->setup_rdma_qp());

    // accept the connection
    struct rdma_conn_param params = {};
    memset(&params, 0, sizeof(params));
    params.initiator_depth = 1;
    params.responder_resources = 1;
    params.rnr_retry_count = 7;
    DMTR_OK(rdma_accept(new_rdma_id, &params));

    // todo: is the assumption that this is always a `sockaddr_in`
    // correct? the interface always specifies `sockaddr *`.
    struct sockaddr *paddr = NULL;
    DMTR_OK(rdma_get_peer_addr(paddr, new_rdma_id));
    size_t n = *addrlen;
    memcpy(saddr, paddr, std::min(n, sizeof(sockaddr_in)));
    *addrlen = sizeof(sockaddr_in);

    return 0;
}

int dmtr::rdma_queue::listen(int backlog)
{
    DMTR_TRUE(EPERM, !my_listening_flag);
    DMTR_NOTNULL(my_rdma_id);

    set_non_blocking(my_rdma_id->channel->fd);
    DMTR_OK(rdma_listen(my_rdma_id, backlog));
    my_listening_flag = true;
    return 0;
}

int dmtr::rdma_queue::connect(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_NOTNULL(my_rdma_id);

    // Convert regular address into an rdma address
    DMTR_OK(rdma_resolve_addr(my_rdma_id, NULL, saddr, 1));
    // Wait for address resolution
    DMTR_OK(expect_rdma_cm_event(EADDRNOTAVAIL, RDMA_CM_EVENT_ADDR_RESOLVED, my_rdma_id));

    // Find path to rdma address
    DMTR_OK(rdma_resolve_route(my_rdma_id, 1));
    // Wait for path resolution
    DMTR_OK(expect_rdma_cm_event(EPERM, RDMA_CM_EVENT_ROUTE_RESOLVED, my_rdma_id));

    DMTR_OK(setup_rdma_qp());

    // Get channel
    struct rdma_conn_param params = {};
    params.initiator_depth = 1;
    params.responder_resources = 1;
    params.rnr_retry_count = 1;
    DMTR_OK(rdma_connect(my_rdma_id, &params));
    int ret = expect_rdma_cm_event(ECONNREFUSED, RDMA_CM_EVENT_ESTABLISHED, my_rdma_id);
    switch (ret) {
        default:
            DMTR_OK(ret);
            DMTR_UNREACHABLE();
        case ECONNREFUSED:
            return ret;
        case 0:
        break;
    }

    DMTR_OK(set_non_blocking(my_rdma_id->channel->fd));
    return 0;
}

int dmtr::rdma_queue::close()
{
    DMTR_NOTNULL(my_rdma_id);

    DMTR_OK(rdma_destroy_qp(my_rdma_id));
    DMTR_OK(ibv_dealloc_pd(my_rdma_id->pd));
    rdma_event_channel *channel = my_rdma_id->channel;
    DMTR_OK(rdma_destroy_id(my_rdma_id));
    DMTR_OK(rdma_destroy_event_channel(channel));

    my_rdma_id = NULL;
    return 0;
}

int dmtr::rdma_queue::on_recv_completed(task &t) {
    if (t.byte_len < sizeof(dmtr_header_t)) {
        t.done = true;
        t.error = EPROTO;
        return 0;
    }

    uint8_t *p = reinterpret_cast<uint8_t *>(t.sga.sga_buf);
    dmtr_header_t * const header = reinterpret_cast<dmtr_header_t *>(p);
    t.header = *header;
    p += sizeof(dmtr_header_t);

    t.sga.sga_numsegs = t.header.h_sgasegs;
    size_t len = 0;
    for (size_t i = 0; i < t.sga.sga_numsegs; ++i) {
        size_t seglen = *reinterpret_cast<uint32_t *>(p);
        t.sga.sga_segs[i].sgaseg_len = seglen;
        //printf("[%x] sga len= %ld\n", qd, t.sga.bufs[i].len);
        p += sizeof(uint32_t);
        t.sga.sga_segs[i].sgaseg_buf = p;
        p += seglen;
        len += seglen;
    }

    t.done = true;
    t.error = 0;
    return 0;
}

int dmtr::rdma_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_NOTNULL(my_rdma_id);
    DMTR_TRUE(EINVAL, my_tasks.find(qt) == my_tasks.cend());
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    task * const t = new task();
    t->sga = sga;
    my_tasks.insert(std::make_pair(qt, t));

    size_t num_sge = 2 * sga.sga_numsegs + 1;
    struct ibv_sge sge[num_sge];
    size_t data_size = 0;
    size_t total_len = 0;

    //printf("ProcessOutgoing qd:%d num_bufs:%d\n", qd, sga.num_bufs);

    // we need to allocate space to serialize the segment lengths.
    DMTR_OK(dmtr_malloc(&t->sga.sga_buf, sga.sga_numsegs * sizeof(uint32_t)));
    uint32_t * const lengths = reinterpret_cast<uint32_t *>(t->sga.sga_buf);
    struct ibv_mr *lengths_mr = NULL;
    DMTR_OK(get_rdma_mr(lengths_mr, lengths));

    // calculate size and fill in iov
    for (size_t i = 0; i < sga.sga_numsegs; ++i) {
        // todo: we need to use network byte ordering.
        lengths[i] = sga.sga_segs[i].sgaseg_len;

        const auto j = 2 * i + 1;
        sge[j].addr = reinterpret_cast<uintptr_t>(&lengths[i]);
        sge[j].length = sizeof(*lengths);
        sge[j].lkey = lengths_mr->lkey;

        const auto k = j + 1;
        void * const p = sga.sga_segs[i].sgaseg_buf;
        struct ibv_mr *mr = NULL;
        DMTR_OK(get_rdma_mr(mr, p));
        sge[k].addr = reinterpret_cast<uintptr_t>(p);
        sge[k].length = sga.sga_segs[i].sgaseg_len;
        sge[k].lkey = mr->lkey;

        // add up actual data size
        data_size += sga.sga_segs[i].sgaseg_len;

        // add up expected packet size minus header
        total_len += sga.sga_segs[i].sgaseg_len;
        total_len += sizeof(sga.sga_segs[i].sgaseg_len);
    }

    // fill in header
    t->header.h_magic = DMTR_HEADER_MAGIC;
    t->header.h_bytes = total_len;
    t->header.h_sgasegs = sga.sga_numsegs;

    // set up header at beginning of packet
    sge[0].addr = reinterpret_cast<uintptr_t>(&t->header);
    sge[0].length = sizeof(t->header);
    struct ibv_mr *mr = NULL;
    DMTR_OK(get_rdma_mr(mr, t));
    sge[0].lkey = mr->lkey;

    // set up RDMA work request.
    struct ibv_send_wr wr = {};
    wr.opcode = IBV_WR_SEND;
    // warning: if you don't set the send flag, it will not
    // give a meaningful error.
    wr.send_flags = IBV_SEND_SIGNALED;
    wr.wr_id = qt;
    wr.sg_list = sge;
    wr.num_sge = num_sge;

    struct ibv_send_wr *bad_wr = NULL;
    DMTR_OK(ibv_post_send(bad_wr, my_rdma_id->qp, &wr));
    return 0;
}

int dmtr::rdma_queue::pop(dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(my_rdma_id);
    DMTR_TRUE(EINVAL, my_tasks.find(qt) == my_tasks.cend());
    DMTR_TRUE(ENOTSUP, !my_listening_flag);
    assert(my_rdma_id->verbs != NULL);

    // todo: it looks like we can't receive anything larger than
    // `recv_buf_size`,
    void *buf = NULL;
    DMTR_OK(dmtr_malloc(&buf, recv_buf_size));
    auto t = new task();
    t->pull = true;
    t->sga.sga_buf = buf;
    my_tasks.insert(std::make_pair(qt, t));

    struct ibv_pd *pd = NULL;
    DMTR_OK(get_pd(pd));
    struct ibv_mr *mr = NULL;
    DMTR_OK(get_rdma_mr(mr, buf));
    struct ibv_sge sge = {};
    sge.addr = reinterpret_cast<uintptr_t>(buf);
    sge.length = recv_buf_size;
    sge.lkey = mr->lkey;
    struct ibv_recv_wr wr = {};
    wr.wr_id = qt;
    wr.sg_list = &sge;
    wr.next = NULL;
    wr.num_sge = 1;
    struct ibv_recv_wr *bad_wr = NULL;
    DMTR_OK(ibv_post_recv(bad_wr, my_rdma_id->qp, &wr));
    //fprintf(stderr, "Done posting receive buffer: %lx %d\n", buf, recv_buf_size);

    return 0;
}

int dmtr::rdma_queue::peek(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(my_rdma_id);
    auto it = my_tasks.find(qt);
    DMTR_TRUE(EINVAL, it != my_tasks.cend());
    task const * t = it->second;

    int ret = service_event_queue();
    switch (ret) {
        default:
            DMTR_OK(ret);
            DMTR_UNREACHABLE();
        case 0:
        case EAGAIN:
            break;
        case ECONNABORTED:
            return ret;
    }

    if (t->pull) {
        DMTR_OK(service_completion_queue(my_rdma_id->recv_cq, 1));
    } else {
        DMTR_OK(service_completion_queue(my_rdma_id->send_cq, 1));
    }

    if (t->done) {
        if (t->pull && 0 == t->error) {
            DMTR_NOTNULL(sga_out);
            *sga_out = t->sga;
        }

        return t->error;
    }

    return EAGAIN;
}

int dmtr::rdma_queue::wait(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(my_rdma_id);

    Latency_Start(&rdma_wait_latency);
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = poll(sga_out, qt);
    }
    Latency_End(&rdma_wait_latency);

    return ret;
}

int dmtr::rdma_queue::poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(my_rdma_id);

    int ret = peek(sga_out, qt);
    switch (ret) {
        default:
            return ret;
        case 0:
            auto it = my_tasks.find(qt);
            DMTR_TRUE(ENOTSUP, it != my_tasks.cend());
            task * const t = it->second;
            if (!t->pull && t->sga.sga_buf != NULL) {
                // free the buffer used to store segment lengths.
                free(t->sga.sga_buf);
            }
            delete it->second;
            my_tasks.erase(it);
            return 0;
    }
}

int dmtr::rdma_queue::pop_accept(struct rdma_cm_id *&id_out) {
    id_out = NULL;
    DMTR_TRUE(EPERM, my_listening_flag);

    int ret = service_event_queue();
    switch (ret) {
        default:
            DMTR_OK(ret);
        case 0:
            break;
        case EAGAIN:
            break;
    }

    if (my_accepts.empty()) {
        return EAGAIN;
    }

    id_out = my_accepts.front();
    my_accepts.pop();
    return 0;
}

int dmtr::rdma_queue::rdma_create_event_channel(struct rdma_event_channel *&channel_out) {
    channel_out = ::rdma_create_event_channel();
    if (NULL == channel_out) {
        return errno;
    }

    return 0;
}

int dmtr::rdma_queue::rdma_create_id(struct rdma_cm_id *&id_out, struct rdma_event_channel *channel, void *context, enum rdma_port_space ps) {
    id_out = NULL;

    int ret = ::rdma_create_id(channel, &id_out, context, ps);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case 0:
            return 0;
        case -1:
            return errno;
    }
}

int dmtr::rdma_queue::rdma_bind_addr(struct rdma_cm_id * const id, const struct sockaddr * const addr)
{
    int ret = ::rdma_bind_addr(id, const_cast<struct sockaddr *>(addr));
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_listen(struct rdma_cm_id * const id, int backlog) {
    int ret = ::rdma_listen(id, backlog);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_destroy_qp(struct rdma_cm_id * const id) {
    DMTR_NOTNULL(id);

    if (NULL == id->qp) {
        return 0;
    }

    ::rdma_destroy_qp(id);
    return 0;
}

int dmtr::rdma_queue::rdma_destroy_id(struct rdma_cm_id *&id) {
    DMTR_NOTNULL(id);

    int ret = ::rdma_destroy_id(id);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            id = NULL;
            return 0;
    }
}

int dmtr::rdma_queue::rdma_destroy_event_channel(struct rdma_event_channel *&channel) {
    DMTR_NOTNULL(channel);

    ::rdma_destroy_event_channel(channel);
    channel = NULL;
    return 0;
}

int dmtr::rdma_queue::rdma_resolve_addr(struct rdma_cm_id * const id, const struct sockaddr * const src_addr, const struct sockaddr * const dst_addr, int timeout_ms) {
    DMTR_NOTNULL(id);

    int ret = ::rdma_resolve_addr(id, const_cast<struct sockaddr *>(src_addr), const_cast<struct sockaddr *>(dst_addr), timeout_ms);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_get_cm_event(struct rdma_cm_event *&event_out, struct rdma_event_channel *channel) {
    DMTR_NOTNULL(channel);

    int ret = ::rdma_get_cm_event(channel, &event_out);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            event_out = NULL;
            if (EAGAIN == ret || EWOULDBLOCK == ret) {
                return EAGAIN;
            } else {
                return errno;
            }
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_ack_cm_event(struct rdma_cm_event * const event) {
    DMTR_NOTNULL(event);

    int ret = ::rdma_ack_cm_event(event);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::expect_rdma_cm_event(int err, enum rdma_cm_event_type expected, struct rdma_cm_id * const id) {
    DMTR_NOTNULL(id);

    struct rdma_cm_event *event = NULL;
    DMTR_OK(::rdma_get_cm_event(id->channel, &event));
    if (expected != event->event) {
        DMTR_OK(rdma_ack_cm_event(event));
        return err;
    }

    DMTR_OK(rdma_ack_cm_event(event));
    return 0;
}

int dmtr::rdma_queue::rdma_resolve_route(struct rdma_cm_id * const id, int timeout_ms) {
    DMTR_NOTNULL(id);

    int ret = ::rdma_resolve_route(id, timeout_ms);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_connect(struct rdma_cm_id * const id, struct rdma_conn_param * const conn_param) {
    DMTR_NOTNULL(id);
    DMTR_NOTNULL(conn_param);

    int ret = ::rdma_connect(id, conn_param);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::ibv_alloc_pd(struct ibv_pd *&pd_out, struct ibv_context *context) {
    DMTR_NOTNULL(context);

    pd_out = ::ibv_alloc_pd(context);
    if (NULL == pd_out) {
        return EPERM;
    }

    return 0;
}

int dmtr::rdma_queue::ibv_dealloc_pd(struct ibv_pd *&pd) {
    if (NULL == pd) {
        return 0;
    }

    int ret = ::ibv_dealloc_pd(pd);
    switch (ret) {
        default:
            DMTR_OK(ret);
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            pd = NULL;
            return 0;
    }
}

int dmtr::rdma_queue::get_pd(struct ibv_pd *&pd_out) {
    // todo: verify that we intend to use one single protection domain.
    if (NULL == our_pd) {
        DMTR_OK(ibv_alloc_pd(our_pd, my_rdma_id->verbs));
    }

    pd_out = our_pd;
    return 0;
}

int dmtr::rdma_queue::rdma_create_qp(struct rdma_cm_id * const id, struct ibv_pd * const pd, struct ibv_qp_init_attr * const qp_init_attr) {
    DMTR_NOTNULL(id);
    DMTR_NOTNULL(qp_init_attr);

    int ret = ::rdma_create_qp(id, pd, qp_init_attr);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_accept(struct rdma_cm_id * const id, struct rdma_conn_param * const conn_param) {
    DMTR_NOTNULL(id);
    DMTR_NOTNULL(conn_param);

    int ret = ::rdma_accept(id, conn_param);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdma_queue::rdma_get_peer_addr(struct sockaddr *&saddr_out, struct rdma_cm_id * const id) {
    DMTR_NOTNULL(id);

    saddr_out = ::rdma_get_peer_addr(id);
    DMTR_NOTNULL(saddr_out);
    return 0;
}

int dmtr::rdma_queue::ibv_poll_cq(size_t &count_out, struct ibv_cq * const cq, int num_entries, struct ibv_wc * const wc) {
    count_out = 0;
    DMTR_NOTNULL(cq);
    DMTR_NOTNULL(wc);

    int ret = ::ibv_poll_cq(cq, num_entries, wc);
    if (ret < 0) {
        return EPERM;
    }

    DMTR_OK(dmtr_itosz(&count_out, ret));
    return 0;
}

int dmtr::rdma_queue::get_rdma_mr(struct ibv_mr *&mr_out, const void * const p) {
    DMTR_NOTNULL(p);
    DMTR_NOTNULL(my_rdma_id);

    Latency_Start(&get_mr);
    struct ibv_pd *pd = NULL;
    DMTR_OK(get_pd(pd));
    // todo: eliminate this `const_cast<>`.
    struct ibv_mr * const mr = Zeus::RDMA::Hoard::getRdmaMr(const_cast<void *>(p), pd);
    Latency_End(&get_mr);
    DMTR_NOTNULL(mr);
    assert(mr->context == my_rdma_id->verbs);
    assert(mr->pd == pd);
    mr_out = mr;
    return 0;
}

int dmtr::rdma_queue::ibv_post_send(struct ibv_send_wr *&bad_wr_out, struct ibv_qp * const qp, struct ibv_send_wr * const wr) {
    DMTR_NOTNULL(qp);
    DMTR_NOTNULL(wr);
    size_t num_sge = wr->num_sge;
    // undocumented: `ibv_post_send()` returns `ENOMEM` if the
    // s/g array is larger than the max specified for the queue
    // in `setup_rdma_qp()`.
    DMTR_TRUE(ERANGE, num_sge <= max_num_sge);

    Latency_Start(&post_send);
    int ret = ::ibv_post_send(qp, wr, &bad_wr_out);
    Latency_End(&post_send);
    return ret;
}

int dmtr::rdma_queue::ibv_post_recv(struct ibv_recv_wr *&bad_wr_out, struct ibv_qp * const qp, struct ibv_recv_wr * const wr) {
    DMTR_NOTNULL(qp);
    DMTR_NOTNULL(wr);

    return ::ibv_post_recv(qp, wr, &bad_wr_out);
}

