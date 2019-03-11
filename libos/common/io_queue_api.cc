#include "io_queue_api.hh"

#include "memory_queue.hh"
#include <dmtr/annot.h>

#include <cassert>
#include <cstdlib>
#include <unistd.h>

dmtr::io_queue_api::io_queue_api() :
    my_qd_counter(0),
    my_qt_counter(0)
{}

int dmtr::io_queue_api::init(io_queue_api *&newobj_out, int argc, char *argv[]) {
    DMTR_NULL(EINVAL, newobj_out);

    newobj_out = new io_queue_api();
    return 0;
}

int dmtr::io_queue_api::register_queue_ctor(enum io_queue::category_id cid, io_queue_factory::ctor_type ctor) {
    DMTR_OK(my_queue_factory.register_ctor(cid, ctor));
    return 0;
}

int dmtr::io_queue_api::get_queue(io_queue *&q_out, int qd) const {
    q_out = NULL;

    auto it = my_queues.find(qd);
    DMTR_TRUE(EINVAL, my_queues.cend() != it);
    q_out = it->second.get();
    return 0;
}

dmtr_qtoken_t dmtr::io_queue_api::new_qtoken(int qd) {
    uint32_t u = ++my_qt_counter;
    if (0 == u) {
        DMTR_PANIC("Queue token overflow");
    }

    return static_cast<uint64_t>(u) | (static_cast<uint64_t>(qd) << 32);
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
    std::unique_ptr<io_queue> q = NULL;
    DMTR_OK(my_queue_factory.construct(q, cid, qd));

    q_out = q.get();
    int ret = insert_queue(q);
    if (0 != ret) {
        q_out = NULL;
        DMTR_FAIL(ret);
    }

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

int dmtr::io_queue_api::bind(int qd, const struct sockaddr * const saddr, socklen_t size) {
    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->bind(saddr, size));

    return 0;
};

int dmtr::io_queue_api::accept(dmtr_qtoken_t &qtok_out, int sockqd) {
    qtok_out = 0;

    io_queue *sockq = NULL;
    DMTR_OK(get_queue(sockq, sockqd));

    int qd = new_qd();
    auto qtok = new_qtoken(sockqd);
    std::unique_ptr<io_queue> q;
    DMTR_OK(sockq->accept(q, qtok, qd));
    DMTR_OK(insert_queue(q));
    qtok_out = qtok;
    return 0;
}

int dmtr::io_queue_api::listen(int qd, int backlog) {
    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->listen(backlog));

    return 0;
}

int dmtr::io_queue_api::connect(int qd, const struct sockaddr * const saddr, socklen_t size) {
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
    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->close());
    DMTR_OK(remove_queue(qd));

    return 0;
}

int dmtr::io_queue_api::push(dmtr_qtoken_t &qtok_out, int qd, const dmtr_sgarray_t &sga) {
    qtok_out = 0;

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    auto qtok = new_qtoken(qd);
    DMTR_OK(q->push(qtok, sga));

    qtok_out = qtok;
    return 0;
}

int dmtr::io_queue_api::pop(dmtr_qtoken_t &qtok_out, int qd) {
    qtok_out = 0;

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    auto qtok = new_qtoken(qd);
    DMTR_OK(q->pop(qtok));

    qtok_out = qtok;
    return 0;
}

int dmtr::io_queue_api::poll(dmtr_qresult_t * const qr_out, dmtr_qtoken_t qt) {
    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));

    dmtr_qresult_t qr = {};
    int ret = q->poll(&qr, qt);
    switch (ret) {
        default:
            if (DMTR_OPC_ACCEPT == qr.qr_opcode) {
                DMTR_OK(remove_queue(qr.qr_value.qd));
                if (NULL != qr_out) {
                    *qr_out = qr;
                    qr_out->qr_value.qd = 0;
                }
            }
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            DMTR_TRUE(EINVAL, NULL != qr_out || DMTR_OPC_PUSH == qr.qr_opcode);
            if (NULL != qr_out) {
                *qr_out = qr;
            }
            return 0;
    }

    return ret;
}

int dmtr::io_queue_api::drop(dmtr_qtoken_t qt) {
    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    return q->drop(qt);
}
