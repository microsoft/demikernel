#include "io_queue_api.hh"

#include "memory_queue.hh"
#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <cassert>
#include <cstdlib>
#include <unistd.h>

static dmtr_timer_t *pop_timer = NULL;
static dmtr_timer_t *push_timer = NULL;
static dmtr_timer_t *poll_timer = NULL;

dmtr::io_queue_api::io_queue_api() :
    my_qd_counter(0),
    my_qt_counter(0)
{}

dmtr::io_queue_api::~io_queue_api()
{
    dmtr_dump_timer(stderr, pop_timer);
    dmtr_dump_timer(stderr, push_timer);
    dmtr_dump_timer(stderr, poll_timer);
    dmtr_dump_timer(stderr, read_timer);
    dmtr_dump_timer(stderr, write_timer);
}

int dmtr::io_queue_api::init(io_queue_api *&newobj_out, int argc, char *argv[]) {
    DMTR_NULL(EINVAL, newobj_out);

    newobj_out = new io_queue_api();
    DMTR_OK(dmtr_new_timer(&pop_timer, "pop"));
    DMTR_OK(dmtr_new_timer(&push_timer, "push"));
    DMTR_OK(dmtr_new_timer(&poll_timer, "poll"));
    DMTR_OK(dmtr_new_timer(&read_timer, "read"));
    DMTR_OK(dmtr_new_timer(&write_timer, "write"));
    return 0;
}

int dmtr::io_queue_api::register_queue_ctor(enum io_queue::category_id cid, io_queue_factory::ctor_type ctor) {
    DMTR_OK(my_queue_factory.register_ctor(cid, ctor));
    return 0;
}

int dmtr::io_queue_api::get_queue(io_queue *&q_out, int qd) const {
    q_out = NULL;

    auto it = my_queues.find(qd);
    if (my_queues.cend() == it) {
        return ENOENT;
    }

    q_out = it->second.get();
    return 0;
}

int dmtr::io_queue_api::new_qtoken(dmtr_qtoken_t &qt_out, int qd) {
    DMTR_TRUE(EINVAL, qd != 0);

    uint32_t u = ++my_qt_counter;
    if (0 == u) {
        DMTR_PANIC("Queue token overflow");
    }

    qt_out = static_cast<uint64_t>(u) | (static_cast<uint64_t>(qd) << 32);
    return 0;
}

int dmtr::io_queue_api::new_qd() {
    int qd = ++my_qd_counter;
    if (0 > qd) {
        DMTR_PANIC("Queue descriptor overflow");
    }

    return qd;
}

int dmtr::io_queue_api::new_queue(io_queue *&q_out, enum io_queue::category_id cid) {
    q_out = NULL;

    int qd = new_qd();
    std::unique_ptr<io_queue> qq;
    DMTR_OK(my_queue_factory.construct(qq, cid, qd));
    io_queue * const q = qq.get();
    DMTR_OK(insert_queue(qq));

    q_out = q;
    return 0;
}

int dmtr::io_queue_api::insert_queue(std::unique_ptr<io_queue> &q) {
    DMTR_NOTNULL(EINVAL, q);

    int qd = q->qd();
    if (my_queues.find(qd) != my_queues.cend()) {
        return EEXIST;
    }

    my_queues[qd] = std::move(q);
    return 0;
}

int dmtr::io_queue_api::remove_queue(int qd) {
    DMTR_TRUE(EINVAL, qd != 0);

    auto it = my_queues.find(qd);
    if (my_queues.cend() == it) {
        return ENOENT;
    }

    my_queues.erase(it);
    return 0;
}

int dmtr::io_queue_api::queue(int &qd_out) {
    qd_out = 0;

    io_queue *q = NULL;
    DMTR_OK(new_queue(q, io_queue::MEMORY_Q));

    qd_out = q->qd();
    return 0;
}

int dmtr::io_queue_api::socket(int &qd_out, int domain, int type, int protocol) {
    qd_out = 0;

    io_queue *q = NULL;
    DMTR_OK(new_queue(q, io_queue::NETWORK_Q));

    int ret = q->socket(domain, type, protocol);
    if (ret != 0) {
        DMTR_OK(remove_queue(q->qd()));
        DMTR_FAIL(ret);
    }

    qd_out = q->qd();
    return 0;
}

int dmtr::io_queue_api::getsockname(int qd, struct sockaddr * const saddr, socklen_t * const size) {
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->getsockname(saddr, size));

    return 0;
};

int dmtr::io_queue_api::bind(int qd, const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->bind(saddr, size));

    return 0;
};

int dmtr::io_queue_api::accept(dmtr_qtoken_t &qtok_out, int sockqd) {
    qtok_out = 0;
    DMTR_TRUE(EINVAL, sockqd != 0);

    io_queue *sockq = NULL;
    DMTR_OK(get_queue(sockq, sockqd));

    int qd = new_qd();
    dmtr_qtoken_t qt;
    DMTR_OK(new_qtoken(qt, sockqd));
    std::unique_ptr<io_queue> q;
    DMTR_OK(sockq->accept(q, qt, qd));
    DMTR_OK(insert_queue(q));
    qtok_out = qt;
    return 0;
}

int dmtr::io_queue_api::listen(int qd, int backlog) {
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->listen(backlog));

    return 0;
}

int dmtr::io_queue_api::connect(int qd, const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    int ret = q->connect(saddr, size);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case ECONNREFUSED:
            return ret;
        case 0:
        break;
    }

    return 0;
}

int dmtr::io_queue_api::close(int qd) {
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    int ret = q->close();
    DMTR_OK(remove_queue(qd));

    DMTR_OK(ret);
    return 0;
}

int dmtr::io_queue_api::is_qd_valid(bool &flag, int qd)
{
    flag = 0;

    io_queue *q = NULL;
    int ret = get_queue(q, qd);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case 0:
            flag = 1;
            return 0;
        case ENOENT:
            return 0;
    }
}

int dmtr::io_queue_api::push(dmtr_qtoken_t &qtok_out, int qd, const dmtr_sgarray_t &sga) {
    DMTR_OK(dmtr_start_timer(push_timer));
              
    qtok_out = 0;
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    dmtr_qtoken_t qt;
    DMTR_OK(new_qtoken(qt, qd));
    DMTR_OK(q->push(qt, sga));

    qtok_out = qt;
    DMTR_OK(dmtr_stop_timer(push_timer));
    return 0;
}

int dmtr::io_queue_api::pop(dmtr_qtoken_t &qtok_out, int qd) {
    DMTR_OK(dmtr_start_timer(pop_timer));

    qtok_out = 0;
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    dmtr_qtoken_t qt;
    DMTR_OK(new_qtoken(qt, qd));
    DMTR_OK(q->pop(qt));

    qtok_out = qt;
    DMTR_OK(dmtr_stop_timer(pop_timer));
    return 0;
}

int dmtr::io_queue_api::poll(dmtr_qresult_t * const qr_out, dmtr_qtoken_t qt) {
    DMTR_OK(dmtr_start_timer(poll_timer));
              
    DMTR_TRUE(EINVAL, qt != 0);

    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));

    dmtr_qresult_t qr = {};
    int ret = q->poll(qr, qt);
    switch (ret) {
        default:
            // if there's a failure on an accept token, we remove the queue we created at the beginning of the operation.
            if (DMTR_OPC_ACCEPT == qr.qr_opcode) {
                DMTR_OK(remove_queue(qr.qr_value.ares.qd));
                DMTR_NOTNULL(EINVAL, qr_out);
                *qr_out = qr;
                qr_out->qr_value.ares.qd = 0;
            }
            DMTR_FAIL(ret);
        case EAGAIN:
        case ECONNABORTED:
        case ECONNRESET:
            return ret;
        case 0:
            // callers that know they're waiting on a push token can specify `NULL` for `qr_out` without causing a fuss.
            DMTR_TRUE(EINVAL, NULL != qr_out || DMTR_OPC_PUSH == qr.qr_opcode);
            if (NULL != qr_out) {
                *qr_out = qr;
            }
            DMTR_OK(dmtr_stop_timer(poll_timer));
            return 0;
   
   
    }

    return ret;
}

int dmtr::io_queue_api::drop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, qt != 0);

    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    return q->drop(qt);
}
