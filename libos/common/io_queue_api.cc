#include "io_queue_api.hh"

#include "memory_queue.hh"
#include "latency.h"
#include <dmtr/annot.h>

#include <cassert>
#include <cstdlib>
#include <unistd.h>

// todo: make symbols private
#define BUFFER_SIZE 1024
#define PUSH_MASK 0x1
#define TOKEN_MASK 0x0000FFFF
#define QUEUE_MASK 0xFFFF0000
#define TOKEN(t) t & TOKEN_MASK
#define IS_PUSH(t) t & PUSH_MASK

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

dmtr_qtoken_t dmtr::io_queue_api::new_qtoken(int qd, bool push) {
    // todo: is this means of generating tokens robust?
    if (token_counter == 0) token_counter++;
    dmtr_qtoken_t t = (token_counter << 1 & TOKEN_MASK) | ((dmtr_qtoken_t)qd << 32);
    if (push) t |= PUSH_MASK;
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
        DMTR_OK(ret);
        DMTR_UNREACHABLE();
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

int dmtr::io_queue_api::accept(int &qd_out, struct sockaddr * const saddr_out,socklen_t * const size_out, int sockqd) {
    qd_out = 0;

    io_queue *sockq = NULL;
    DMTR_OK(get_queue(sockq, sockqd));

    io_queue *q = NULL;
    int qd = new_qd();
    int ret = sockq->accept(q, saddr_out, size_out, qd);
    switch (ret) {
        default:
            DMTR_OK(ret);
            DMTR_UNREACHABLE();
        case EAGAIN:
            return EAGAIN;
        case 0:
            break;
    }

    DMTR_OK(insert_queue(q));
    qd_out = qd;
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
            DMTR_OK(ret);
            DMTR_UNREACHABLE();
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
    auto qtok = new_qtoken(qd, true);
    DMTR_OK(q->push(qtok, sga));

    qtok_out = qtok;
    return 0;
}

int dmtr::io_queue_api::pop(dmtr_qtoken_t &qtok_out, int qd) {
    qtok_out = 0;

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    auto qtok = new_qtoken(qd, false);
    DMTR_OK(q->pop(qtok));

    qtok_out = qtok;
    return 0;
}

int dmtr::io_queue_api::poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt) {
    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    return q->poll(sga_out, qt);
}

int dmtr::io_queue_api::drop(dmtr_qtoken_t qt) {
    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    return q->drop(qt);
}
