// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "rdma_queue.hh"

#include <arpa/inet.h>
#include <cassert>
#include <cerrno>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/latency.h>
#include <dmtr/sga.h>
#include <hoard/zeusrdma.h>
#include <iostream>
#include <dmtr/mem.h>
#include <dmtr/raii_guard.hh>
#include <netinet/tcp.h>
#include <rdma/rdma_verbs.h>
#include <sys/uio.h>
#include <unistd.h>

//#define DMTR_PIN_MEMORY 1
#define DMTR_PROFILE 1

#if DMTR_PROFILE
typedef std::unique_ptr<dmtr_latency_t, std::function<void(dmtr_latency_t *)>> latency_ptr_type;
static latency_ptr_type read_latency;
static latency_ptr_type write_latency;
#endif

struct ibv_pd *dmtr::rdma_queue::our_pd = NULL;
std::unique_ptr<dmtr::rdmacm_router> dmtr::rdma_queue::our_rdmacm_router;
const size_t dmtr::rdma_queue::recv_buf_count = 32;
const size_t dmtr::rdma_queue::recv_buf_size = 131100;
const size_t dmtr::rdma_queue::max_num_sge = DMTR_SGARRAY_MAXSIZE;
const dmtr::rdma_queue::duration_type dmtr::rdma_queue::event_polling_period = duration_type(1000);

dmtr::rdma_queue::rdma_queue(int qd) :
    io_queue(NETWORK_Q, qd),
    my_last_event_channel_poll(clock_type::now()),
    my_rdma_id(NULL),
    my_listening_flag(false)
{}

int dmtr::rdma_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    if (NULL == our_rdmacm_router) {
        DMTR_OK(rdmacm_router::new_object(our_rdmacm_router));
    }

#if DMTR_PROFILE
    if (NULL == read_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "read"));
        read_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == write_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "write"));
        write_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }
#endif

    q_out = std::unique_ptr<io_queue>(new rdma_queue(qd));
    DMTR_NOTNULL(ENOMEM, q_out);
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
    DMTR_TRUE(ENOTSUP, IBV_WC_SUCCESS == wc.status);

    switch (wc.opcode) {
        default:
            fprintf(stderr, "Unexpected WC opcode: 0x%x\n", wc.opcode);
            return ENOTSUP;
        case IBV_WC_RECV: {
            void *buf = reinterpret_cast<void *>(wc.wr_id);
            size_t byte_len = wc.byte_len;
            my_pending_recvs.push(std::make_pair(buf, byte_len));
            DMTR_OK(new_recv_buf());
            return 0;
        }
        case IBV_WC_SEND: {
            dmtr_qtoken_t qt = wc.wr_id;
            my_completed_sends.insert(qt);
            return 0;
        }
    }
}

int dmtr::rdma_queue::service_completion_queue(struct ibv_cq * const cq, size_t quantity) {
    DMTR_NOTNULL(EINVAL, cq);
    DMTR_TRUE(EINVAL, quantity > 0);

    // check completion queue
    struct ibv_wc wc[quantity];
    size_t count = 0;
    DMTR_OK(ibv_poll_cq(count, cq, quantity, wc));
    //fprintf(stderr, "Found receive work completions: %d\n", num);
    // process messages
    for (size_t i = 0; i < count; ++i) {
        on_work_completed(wc[i]);
    }
    //fprintf(stderr, "Done draining completion queue\n");

    return 0;
}

int dmtr::rdma_queue::service_event_channel() {
    DMTR_NOTNULL(EPERM, my_rdma_id);
    DMTR_NOTNULL(EINVAL, our_rdmacm_router);

    struct rdma_cm_event event;
    int ret = our_rdmacm_router->poll(event, my_rdma_id);
    switch (ret) {
        default:
            DMTR_OK(ret);
        case 0:
            break;
        case EAGAIN:
            return EAGAIN;
    }

    switch(event.event) {
        default:
            fprintf(stderr, "Unrecognized event: 0x%x\n", event.event);
            return ENOTSUP;
        case RDMA_CM_EVENT_TIMEWAIT_EXIT:
            //fprintf(stderr, "An uninteresting event about recycling QP\n");
            return EAGAIN;
        case RDMA_CM_EVENT_CONNECT_REQUEST:
            my_pending_accepts.push(event.id);
            //fprintf(stderr, "Event: RDMA_CM_EVENT_CONNECT_REQUEST\n");
            break;
        case RDMA_CM_EVENT_DISCONNECTED:
            //fprintf(stderr, "Event: RDMA_CM_EVENT_DISCONNECTED\n");
            return ECONNABORTED; // client should call close
        case RDMA_CM_EVENT_ESTABLISHED:
            //fprintf(stderr, "Event: RDMA_CM_EVENT_ESTABLISHED\n");
            break;
    }

    return 0;
}

int dmtr::rdma_queue::socket(int domain, int type, int protocol)
{
    DMTR_NULL(EPERM, my_rdma_id);
    DMTR_NOTNULL(EINVAL, our_rdmacm_router);

    DMTR_OK(our_rdmacm_router->create_id(my_rdma_id, type));
    return 0;
}

int
dmtr::rdma_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size)
{
    // can't run getsockname on rdma socket
    sockaddr *addr = rdma_get_local_addr(my_rdma_id);
    if (addr != NULL) {
	memcpy(saddr, addr, sizeof(sockaddr_in));
	*size = sizeof(sockaddr_in);
	return 0; // eok
    }
    return -1;
}

int dmtr::rdma_queue::bind(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_NOTNULL(EPERM, my_rdma_id);

    DMTR_OK(rdma_bind_addr(my_rdma_id, saddr));
    return 0;

}

int dmtr::rdma_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qt, int new_qd) {
    q_out = NULL;
    DMTR_TRUE(EPERM, my_listening_flag);
    DMTR_NOTNULL(EINVAL, our_rdmacm_router);
    DMTR_NOTNULL(EINVAL, my_accept_thread);

    auto * const q = new rdma_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);
    auto qq = std::unique_ptr<io_queue>(q);

    DMTR_OK(new_task(qt, DMTR_OPC_ACCEPT, q));
    my_accept_thread->enqueue(qt);

    q_out = std::move(qq);
    return 0;
}


int dmtr::rdma_queue::accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    DMTR_NOTNULL(EINVAL, our_rdmacm_router);

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        io_queue *new_q = NULL;
        DMTR_TRUE(EINVAL, t->arg(new_q));
        auto * const new_rq = dynamic_cast<rdma_queue *>(new_q);
        DMTR_NOTNULL(EINVAL, new_rq);

        while (my_pending_accepts.empty()) {
            int ret = service_event_channel();
            switch (ret) {
                default:
                    DMTR_FAIL(ret);
                case EAGAIN:
                    yield();
                    continue;
                case ECONNABORTED:
                    return ret;
                case 0:
                    break;
            }
        }

        auto * const new_rdma_id = my_pending_accepts.front();
        my_pending_accepts.pop();

        new_rq->my_rdma_id = new_rdma_id;
        DMTR_OK(our_rdmacm_router->bind_id(new_rdma_id));
        DMTR_OK(new_rq->setup_rdma_qp());
        DMTR_OK(new_rq->setup_recv_queue());
        new_rq->start_threads();

        // accept the connection
        struct rdma_conn_param params = {};
        params.initiator_depth = 1;
        params.responder_resources = 1;
        params.rnr_retry_count = 7;
        DMTR_OK(rdma_accept(new_rdma_id, &params));

        // get the address
        sockaddr *saddr;
        DMTR_OK(rdma_get_peer_addr(saddr, new_rdma_id));
        DMTR_OK(t->complete(0, new_rq->qd(), *reinterpret_cast<sockaddr_in *>(saddr), sizeof(sockaddr_in)));
    }

    return 0;
}

int dmtr::rdma_queue::listen(int backlog)
{
    DMTR_TRUE(EPERM, !my_listening_flag);
    DMTR_NOTNULL(EPERM, my_rdma_id);

    set_non_blocking(my_rdma_id->channel->fd);
    DMTR_OK(rdma_listen(my_rdma_id, backlog));
    my_listening_flag = true;
    start_threads();
    return 0;
}

int dmtr::rdma_queue::connect(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_NOTNULL(EPERM, my_rdma_id);
    // Spin for 10 seconds before giving up.
    auto timeout = duration_type(1000 * 10);

    int timeout_int = 0;
    DMTR_OK(dmtr_u32toi(&timeout_int, timeout.count()));

    // Convert regular address into an rdma address
    DMTR_OK(rdma_resolve_addr(my_rdma_id, NULL, saddr, timeout_int));
    // Wait for address resolution
    DMTR_OK(expect_rdma_cm_event(EADDRNOTAVAIL, RDMA_CM_EVENT_ADDR_RESOLVED, my_rdma_id, timeout));

    // Find path to rdma address
    DMTR_OK(rdma_resolve_route(my_rdma_id, timeout_int));
    // Wait for path resolution
    DMTR_OK(expect_rdma_cm_event(EPERM, RDMA_CM_EVENT_ROUTE_RESOLVED, my_rdma_id, timeout));

    DMTR_OK(setup_rdma_qp());
    DMTR_OK(setup_recv_queue());

    // Get channel
    struct rdma_conn_param params = {};
    params.initiator_depth = 1;
    params.responder_resources = 1;
    params.rnr_retry_count = 1;
    DMTR_OK(rdma_connect(my_rdma_id, &params));
    int ret = expect_rdma_cm_event(ECONNREFUSED, RDMA_CM_EVENT_ESTABLISHED, my_rdma_id, timeout);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case ECONNREFUSED:
            return ret;
        case 0:
            break;
    }

    start_threads();
    return 0;
}

int dmtr::rdma_queue::close()
{
    DMTR_NOTNULL(EINVAL, our_rdmacm_router);

    if (NULL == my_rdma_id) {
        return 0;
    }

    // In RDMA, both client and server should be calling rdma_disconnect
    if (my_rdma_id->qp != NULL)
    {
        DMTR_OK(rdma_disconnect(my_rdma_id));
        DMTR_OK(rdma_destroy_qp(my_rdma_id));
        my_rdma_id->qp = NULL;
    }

    struct rdma_cm_id *rdma_id = my_rdma_id;
    my_rdma_id = NULL;

    // todo: until we deal with unregistering memory, deallocating the protection domain will fail.
    // Similarly destroying the id and the event channel will fail.
    //DMTR_OK(ibv_dealloc_pd(rdma_id->pd));

    //rdma_event_channel *channel = rdma_id->channel;
    //DMTR_OK(rdma_destroy_event_channel(channel));

    //DMTR_OK(our_rdmacm_router->destroy_id(rdma_id));
    rdma_id->channel = NULL;

    return 0;
}

int dmtr::rdma_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_NOTNULL(EPERM, my_rdma_id);
    DMTR_TRUE(ENOTSUP, !my_listening_flag);
    DMTR_NOTNULL(EINVAL, my_push_thread);

    size_t sgalen = 0;
    DMTR_OK(dmtr_sgalen(&sgalen, &sga));
    if (0 == sgalen) {
        return ENOMSG;
    }

    dmtr_sgarray_t sga_copy = sga;
    size_t num_sge = 2 * sga_copy.sga_numsegs + 1;
    struct ibv_sge sge[num_sge];
    size_t data_size = 0;
    size_t total_len = 0;

    // we need to allocate space to serialize metadata.
    void *buf = NULL;
    DMTR_OK(dmtr_malloc(&buf, sizeof(struct metadata) + sga_copy.sga_numsegs * sizeof(uint32_t)));
    auto md = std::unique_ptr<metadata>(reinterpret_cast<metadata *>(buf));
    sga_copy.sga_buf = buf;
    struct ibv_mr *md_mr = NULL;
    DMTR_OK(get_rdma_mr(md_mr, md.get()));

    // calculate size and fill in iov
    for (size_t i = 0; i < sga_copy.sga_numsegs; ++i) {
        md->lengths[i] = htonl(sga_copy.sga_segs[i].sgaseg_len);

        const auto j = 2 * i + 1;
        sge[j].addr = reinterpret_cast<uintptr_t>(&md->lengths[i]);
        sge[j].length = sizeof(*md->lengths);
        sge[j].lkey = md_mr->lkey;

        const auto k = j + 1;
        void * const p = sga_copy.sga_segs[i].sgaseg_buf;
        struct ibv_mr *mr = NULL;
        DMTR_OK(get_rdma_mr(mr, p));
        sge[k].addr = reinterpret_cast<uintptr_t>(p);
        sge[k].length = sga_copy.sga_segs[i].sgaseg_len;
        sge[k].lkey = mr->lkey;

        // add up actual data size
        data_size += sga_copy.sga_segs[i].sgaseg_len;

        // add up expected packet size minus header
        total_len += sga_copy.sga_segs[i].sgaseg_len;
        total_len += sizeof(sga_copy.sga_segs[i].sgaseg_len);
    }

    // fill in header
    md->header.h_magic = htonl(DMTR_HEADER_MAGIC);
    md->header.h_bytes = htonl(total_len);
    md->header.h_sgasegs = htonl(sga_copy.sga_numsegs);

    // set up header at beginning of packet
    sge[0].addr = reinterpret_cast<uintptr_t>(&md->header);
    sge[0].length = sizeof(md->header);
    sge[0].lkey = md_mr->lkey;

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
    pin(sga_copy);
#if DMTR_PROFILE
    boost::chrono::duration<uint64_t, boost::nano> dt(0);
    auto t0 = boost::chrono::steady_clock::now();
#endif

    DMTR_OK(ibv_post_send(bad_wr, my_rdma_id->qp, &wr));

#if DMTR_PROFILE
    dt += (boost::chrono::steady_clock::now() - t0);
    DMTR_OK(dmtr_record_latency(write_latency.get(), dt.count()));
#endif

    md.release();

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, sga_copy));
    my_push_thread->enqueue(qt);

    return 0;
}

int dmtr::rdma_queue::push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        const dmtr_sgarray_t *sga = NULL;
        DMTR_TRUE(EINVAL, t->arg(sga));
        auto buf = std::unique_ptr<metadata>(reinterpret_cast<metadata *>(sga->sga_buf));

        while (true) {
            DMTR_OK(service_completion_queue(my_rdma_id->send_cq, 1));
            auto it = my_completed_sends.find(qt);
            if (my_completed_sends.cend() != it) {
                my_completed_sends.erase(it);
                break;
            }
            yield();
        }
        DMTR_OK(t->complete(0, *sga));
    }

    return 0;
}

int dmtr::rdma_queue::pop(dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(EPERM, my_rdma_id);
    DMTR_TRUE(ENOTSUP, !my_listening_flag);
    assert(my_rdma_id->verbs != NULL);

    DMTR_OK(new_task(qt, DMTR_OPC_POP));
    my_pop_thread->enqueue(qt);

    return 0;
}

int dmtr::rdma_queue::pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

#if DMTR_PROFILE
        boost::chrono::steady_clock::time_point t0;
        boost::chrono::duration<uint64_t, boost::nano> dt(0);
#endif
        void *buf = NULL;
        size_t sz_buf = 0;
        while (NULL == buf) {
#if DMTR_PROFILE
            t0 = boost::chrono::steady_clock::now();
#endif
            DMTR_OK(service_completion_queue(my_rdma_id->recv_cq, 1));
            int ret = service_recv_queue(buf, sz_buf);
            switch (ret) {
                default:
                    DMTR_FAIL(ret);
                case EAGAIN:
#if DMTR_PROFILE
                    dt += (boost::chrono::steady_clock::now() - t0);
#endif
                    yield();
#if DMTR_PROFILE
                    t0 = boost::chrono::steady_clock::now();
#endif
                    buf = NULL;
                    continue;
                case 0:
                    DMTR_NOTNULL(ENOTSUP, buf);
                    continue;
            }
        }

#if DMTR_PROFILE
        dt += (boost::chrono::steady_clock::now() - t0);
        DMTR_OK(dmtr_record_latency(read_latency.get(), dt.count()));
#endif

#if DMTR_PIN_MEMORY
        raii_guard rg0(std::bind(Zeus::RDMA::Hoard::unpin, buf));
#endif

        const auto key = reinterpret_cast<uintptr_t>(buf);
        auto it = my_recv_bufs.find(key);
        DMTR_TRUE(ENOTSUP, it != my_recv_bufs.cend());
        auto ubuf = std::move(it->second);
        my_recv_bufs.erase(it);

        if (sz_buf < sizeof(dmtr_header_t)) {
            return EPROTO;
        }

        uint8_t *p = reinterpret_cast<uint8_t *>(buf);
        dmtr_header_t * const header = reinterpret_cast<dmtr_header_t *>(p);
        p += sizeof(dmtr_header_t);

        header->h_magic = ntohl(header->h_magic);
        header->h_bytes = ntohl(header->h_bytes);
        header->h_sgasegs = ntohl(header->h_sgasegs);

        if (DMTR_HEADER_MAGIC != header->h_magic) {
            return EILSEQ;
        }

        dmtr_sgarray_t sga = {};
        sga.sga_numsegs = header->h_sgasegs;
        for (size_t i = 0; i < sga.sga_numsegs; ++i) {
            size_t seglen = ntohl(*reinterpret_cast<uint32_t *>(p));
            sga.sga_segs[i].sgaseg_len = seglen;
            //printf("[%x] sga sz_buf= %ld\n", qd, t.sga.bufs[i].sz_buf);
            p += sizeof(uint32_t);
            sga.sga_segs[i].sgaseg_buf = p;
            p += seglen;
        }

        sga.sga_buf = buf;
        DMTR_OK(t->complete(0, sga));
        ubuf.release();
    }

    return 0;
}

int dmtr::rdma_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt) {
    DMTR_OK(task::initialize_result(qr_out, qd(), qt));
    DMTR_TRUE(EINVAL, good());

    const auto now = clock_type::now();
    if (now - my_last_event_channel_poll > event_polling_period) {
        int ret = service_event_channel();
        switch (ret) {
            default:
                DMTR_FAIL(ret);
            case 0:
            case EAGAIN:
                break;
            case ECONNABORTED:
                return ret;
        }

        my_last_event_channel_poll = now;
    }

    task *t;
    DMTR_OK(get_task(t, qt));

    int ret;
    switch (t->opcode()) {
        default:
            return ENOTSUP;
        case DMTR_OPC_ACCEPT:
            ret = my_accept_thread->service();
            break;
        case DMTR_OPC_PUSH:
            ret = my_push_thread->service();
            break;
        case DMTR_OPC_POP:
            ret = my_pop_thread->service();
            break;
    }

    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            break;
        case 0:
            // the threads should only exit if the queue has been closed
            // (`good()` => `false`).
            DMTR_UNREACHABLE();
    }

    return t->poll(qr_out);
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

int dmtr::rdma_queue::rdma_disconnect(struct rdma_cm_id * const id) {
    DMTR_NOTNULL(EINVAL, id);

    int ret = ::rdma_disconnect(id);
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
    DMTR_NOTNULL(EINVAL, id);

    if (NULL == id->qp) {
        return 0;
    }

    ::rdma_destroy_qp(id);
    return 0;
}

int dmtr::rdma_queue::rdma_resolve_addr(struct rdma_cm_id * const id, const struct sockaddr * const src_addr, const struct sockaddr * const dst_addr, int timeout_ms) {
    DMTR_NOTNULL(EINVAL, id);

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

int dmtr::rdma_queue::expect_rdma_cm_event(int err, enum rdma_cm_event_type expected, struct rdma_cm_id * const id, duration_type timeout) {
    DMTR_NOTNULL(EINVAL, id);
    DMTR_NOTNULL(EINVAL, our_rdmacm_router);

    struct rdma_cm_event event = {};
    auto t0 = clock_type::now();
    int ret = EAGAIN;
    while (0 != ret) {
        ret = our_rdmacm_router->poll(event, id);
        switch (ret) {
            default:
                DMTR_FAIL(ret);
            case EAGAIN: {
                auto dt = clock_type::now() - t0;
                if (dt <= timeout) {
                    continue;
                } else {
                    return EAGAIN;
                }
            }
            case 0:
                break;
        }
    }

    if (expected != event.event) {
        std::cerr << "dmtr::rdma_queue::expect_rdma_cm_event(): mismatch; expected " << expected << ", got " << event.event << "." << std::endl;
        return err;
    }

    return 0;
}

int dmtr::rdma_queue::rdma_resolve_route(struct rdma_cm_id * const id, int timeout_ms) {
    DMTR_NOTNULL(EINVAL, id);

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
    DMTR_NOTNULL(EINVAL, id);
    DMTR_NOTNULL(EINVAL, conn_param);

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
    DMTR_NOTNULL(EINVAL, context);

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
            DMTR_FAIL(ret);
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
    DMTR_NOTNULL(EINVAL, id);
    DMTR_NOTNULL(EINVAL, qp_init_attr);

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
    DMTR_NOTNULL(EINVAL, id);
    DMTR_NOTNULL(EINVAL, conn_param);

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
    DMTR_NOTNULL(EINVAL, id);

    saddr_out = ::rdma_get_peer_addr(id);
    DMTR_NOTNULL(ENOTSUP, saddr_out);
    return 0;
}

int dmtr::rdma_queue::ibv_poll_cq(size_t &count_out, struct ibv_cq * const cq, int num_entries, struct ibv_wc * const wc) {
    count_out = 0;
    DMTR_NOTNULL(EINVAL, cq);
    DMTR_NOTNULL(EINVAL, wc);

    int ret = ::ibv_poll_cq(cq, num_entries, wc);
    if (ret < 0) {
        return EPERM;
    }

    DMTR_OK(dmtr_itosz(&count_out, ret));
    return 0;
}

int dmtr::rdma_queue::get_rdma_mr(struct ibv_mr *&mr_out, const void * const p) {
    DMTR_NOTNULL(EINVAL, p);
    DMTR_NOTNULL(EPERM, my_rdma_id);

    struct ibv_pd *pd = NULL;
    DMTR_OK(get_pd(pd));
    // todo: eliminate this `const_cast<>`.
    struct ibv_mr * const mr = Zeus::RDMA::Hoard::getRdmaMr(const_cast<void *>(p), pd);
    DMTR_NOTNULL(ENOTSUP, mr);
    assert(mr->context == my_rdma_id->verbs);
    assert(mr->pd == pd);
    mr_out = mr;
    return 0;
}

int dmtr::rdma_queue::ibv_post_send(struct ibv_send_wr *&bad_wr_out, struct ibv_qp * const qp, struct ibv_send_wr * const wr) {
    DMTR_NOTNULL(EINVAL, qp);
    DMTR_NOTNULL(EINVAL, wr);
    size_t num_sge = wr->num_sge;
    // undocumented: `ibv_post_send()` returns `ENOMEM` if the
    // s/g array is larger than the max specified for the queue
    // in `setup_rdma_qp()`.
    DMTR_TRUE(ERANGE, num_sge <= max_num_sge);

    return ::ibv_post_send(qp, wr, &bad_wr_out);
}

int dmtr::rdma_queue::ibv_post_recv(struct ibv_recv_wr *&bad_wr_out, struct ibv_qp * const qp, struct ibv_recv_wr * const wr) {
    DMTR_NOTNULL(EINVAL, qp);
    DMTR_NOTNULL(EINVAL, wr);

    return ::ibv_post_recv(qp, wr, &bad_wr_out);
}

int dmtr::rdma_queue::new_recv_buf() {
    // todo: it looks like we can't receive anything larger than
    // `recv_buf_size`,
    void *buf = NULL;
    DMTR_OK(dmtr_malloc(&buf, recv_buf_size));
    const auto key = reinterpret_cast<uintptr_t>(buf);
    DMTR_TRUE(ENOTSUP, my_recv_bufs.find(key) == my_recv_bufs.cend());
    my_recv_bufs.insert(std::make_pair(key, std::unique_ptr<uint8_t>(reinterpret_cast<uint8_t *>(buf))));
#if DMTR_PIN_MEMORY
    Zeus::RDMA::Hoard::pin(buf);
#endif

    struct ibv_pd *pd = NULL;
    DMTR_OK(get_pd(pd));
    struct ibv_mr *mr = NULL;
    DMTR_OK(get_rdma_mr(mr, buf));
    struct ibv_sge sge = {};
    sge.addr = reinterpret_cast<uintptr_t>(buf);
    sge.length = recv_buf_size;
    sge.lkey = mr->lkey;
    struct ibv_recv_wr wr = {};
    wr.wr_id = reinterpret_cast<uintptr_t>(buf);
    wr.sg_list = &sge;
    wr.next = NULL;
    wr.num_sge = 1;
    struct ibv_recv_wr *bad_wr = NULL;
    DMTR_OK(ibv_post_recv(bad_wr, my_rdma_id->qp, &wr));
    //fprintf(stderr, "Done posting receive buffer: %lx %d\n", buf, recv_buf_size);

    return 0;
}

int dmtr::rdma_queue::service_recv_queue(void *&buf_out, size_t &len_out) {
    buf_out = NULL;
    if (my_pending_recvs.empty()) {
        return EAGAIN;
    }

    const auto * pair = &my_pending_recvs.front();
    buf_out = pair->first;
    len_out = pair->second;
    my_pending_recvs.pop();
    return 0;
}

int dmtr::rdma_queue::setup_recv_queue() {
    for (size_t i = 0; i < recv_buf_count; ++i) {
        DMTR_OK(new_recv_buf());
    }

    return 0;
}

int dmtr::rdma_queue::pin(const dmtr_sgarray_t &sga) {
#if DMTR_PIN_MEMORY
    for (size_t i = 0; i < sga.sga_numsegs; ++i) {
        void *buf = sga.sga_segs[i].sgaseg_buf;
        DMTR_NOTNULL(EINVAL, buf);
        Zeus::RDMA::Hoard::pin(buf);
    }
#endif

    return 0;
}

int dmtr::rdma_queue::unpin(const dmtr_sgarray_t &sga) {
#if DMTR_PIN_MEMORY
    for (size_t i = 0; i < sga.sga_numsegs; ++i) {
        void *buf = sga.sga_segs[i].sgaseg_buf;
        DMTR_NOTNULL(EINVAL, buf);
        Zeus::RDMA::Hoard::unpin(buf);
    }
#endif

    return 0;
}

int dmtr::rdma_queue::getsockname(int sockfd, struct sockaddr *saddr, socklen_t &addrlen) {
    DMTR_TRUE(EINVAL, saddr != NULL);
    DMTR_TRUE(ERANGE, addrlen > 0);

    int ret = ::getsockname(sockfd, saddr, &addrlen);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case 0:
            return 0;
        case -1:
            return errno;
    }
}

void dmtr::rdma_queue::start_threads() {
    if (my_listening_flag) {
        my_accept_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return accept_thread(yield, tq);
        }));
    } else {
        my_push_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return push_thread(yield, tq);
        }));

        my_pop_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return pop_thread(yield, tq);
        }));
    }
}

int dmtr::rdma_queue::set_my_context(void *context) {
    return ENOTSUP;
}
