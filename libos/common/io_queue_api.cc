#include "io_queue_api.hh"

#include "memory_queue.hh"
#include "latency.h"
#include <dmtr/annot.h>

#include <cassert>
#include <cstdlib>
#include <unistd.h>

// todo: make symbols private
#define BUFFER_SIZE 1024
#define QUEUE_MASK 0xFFFF0000

// dmtr_qtoken_t format
// | 32 bits = queue id | 31 bits = token | 1 bit = push or pop |

DEFINE_LATENCY(pop_latency);
DEFINE_LATENCY(push_latency);
DEFINE_LATENCY(wait_push);

thread_local static int64_t queue_counter = 10;
thread_local static int64_t token_counter = 10;

dmtr::io_queue_api::io_queue_api()
{}

int dmtr::io_queue_api::init(io_queue_api *&newobj_out, int argc, char *argv[]) {
    DMTR_NULL(EINVAL, newobj_out);

    newobj_out = new io_queue_api();
    // todo: should these really be `thread_local`, or atomic?
    queue_counter = rand();
    token_counter = rand();

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
    q_out = it->second;
    return 0;
}

dmtr_qtoken_t dmtr::io_queue_api::new_qtoken(int qd) {
    // todo: is this means of generating tokens robust?
    if (token_counter == 0) token_counter++;
    dmtr_qtoken_t t = token_counter | ((dmtr_qtoken_t)qd << 32);
    //printf("new_qtoken qd:%lx\n", t);
    token_counter++;
    return t;
}

int dmtr::io_queue_api::new_qd() {
    return queue_counter++ & ~QUEUE_MASK;
}

int dmtr::io_queue_api::new_queue(io_queue *&q_out, enum io_queue::category_id cid) {
    q_out = NULL;

    int qd = new_qd();
    io_queue *q = NULL;
    DMTR_OK(my_queue_factory.construct(q, cid, qd));
    DMTR_OK(insert_queue(q));
    q_out = q;
    return 0;
}

int dmtr::io_queue_api::insert_queue(io_queue * const q) {
    DMTR_NOTNULL(EINVAL, q);

    int qd = q->qd();
    //printf("library.h/InsertQueue() qd: %d\n", qd);
    assert(qd == static_cast<int>(qd & ~QUEUE_MASK));

    if (my_queues.find(qd) != my_queues.cend()) {
        return EEXIST;
    }

    my_queues[qd] = q;
    return 0;
}

int dmtr::io_queue_api::remove_queue(int qd) {
    auto it = my_queues.find(qd);
    if (my_queues.cend() == it) {
        return ENOENT;
    }

    delete it->second;
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
    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    DMTR_OK(q->getsockname(saddr, size));

    return 0;
};

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
    io_queue *q = NULL;
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
                //DMTR_OK(remove_queue(qr.qr_value.qd));
                if (NULL != qr_out) {
                    *qr_out = qr;
                    qr_out->qr_tid = DMTR_TID_NIL;
                    qr_out->qr_value.qd = 0;
                }
            }
            DMTR_FAIL(ret);
        case EAGAIN:
        case ECONNABORTED:
        case ECONNRESET:
            return ret;
        case 0:
            DMTR_TRUE(EINVAL, NULL != qr_out || DMTR_TID_NIL == qr.qr_tid);
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
