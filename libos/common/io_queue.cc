#include "io_queue.hh"

#include <cerrno>
#include <dmtr/annot.h>
#include <fcntl.h>

dmtr::io_queue::task::task(dmtr_opcode_t  opcode) :
    opcode(opcode),
    done(false),
    error(0),
    header{},
    sga{},
    num_bytes(0)
{}

int dmtr::io_queue::task::to_qresult(dmtr_qresult_t * const qr_out) const {
    if (NULL != qr_out) {
        *qr_out = {};
        qr_out->qr_tid = DMTR_TID_NIL;
        qr_out->qr_opcode = this->opcode;
    }

    if (!this->done) {
        return EAGAIN;
    }

    if (DMTR_OPC_POP == this->opcode && 0 == this->error) {
        DMTR_NOTNULL(EINVAL, qr_out);
        qr_out->qr_tid = DMTR_TID_SGA;
        qr_out->qr_value.sga = this->sga;
    }

    return this->error;
}

dmtr::io_queue::io_queue(enum category_id cid, int qd) :
    my_cid(cid),
    my_qd(qd)
{}

dmtr::io_queue::~io_queue()
{}

int dmtr::io_queue::socket(int domain, int type, int protocol) {
    return ENOTSUP;
}

int dmtr::io_queue::listen(int backlog) {
    return ENOTSUP;
}

int dmtr::io_queue::bind(const struct sockaddr * const saddr, socklen_t size) {
    return ENOTSUP;
}

int dmtr::io_queue::accept(io_queue *&q_out, struct sockaddr * const saddr_out, socklen_t * const size_out, int new_qd) {
    q_out = NULL;
    return ENOTSUP;
}

int dmtr::io_queue::connect(const struct sockaddr * const saddr, socklen_t size) {
    return ENOTSUP;
}

int dmtr::io_queue::close() {
    return 0;
}

int dmtr::io_queue::set_non_blocking(int fd) {
    int ret = fcntl(fd, F_GETFL);
    if (-1 == ret) {
        return errno;
    }

    int flags = ret;
    if (-1 == fcntl(fd, F_SETFL, flags | O_NONBLOCK)) {
        return errno;
    }

    return 0;
}

int dmtr::io_queue::new_task(task *&t, dmtr_qtoken_t qt, dmtr_opcode_t opcode) {
    t = NULL;
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());

    my_tasks.insert(std::make_pair(qt, task(opcode)));
    DMTR_OK(get_task(t, qt));
    return 0;
}

int dmtr::io_queue::get_task(task *&t, dmtr_qtoken_t qt) {
    t = NULL;
    auto it = my_tasks.find(qt);
    DMTR_TRUE(ENOENT, it != my_tasks.cend());

    t = &it->second;
    DMTR_NOTNULL(EPERM, t);
    return 0;
}

int dmtr::io_queue::drop_task(dmtr_qtoken_t qt) {
    auto it = my_tasks.find(qt);
    DMTR_TRUE(ENOENT, it != my_tasks.cend());
    my_tasks.erase(it);
    return 0;
}
