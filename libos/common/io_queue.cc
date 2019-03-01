#include "io_queue.hh"

#include <cerrno>
#include <fcntl.h>

dmtr::io_queue::task::task(dmtr_opcode_t  opcode, io_queue * const q) :
    opcode(opcode),
    done(false),
    error(0),
    header{},
    sga{},
    queue(q),
    num_bytes(0)
{
    if (q != NULL && opcode != DMTR_OPC_ACCEPT) {
        DMTR_PANIC("a non-NULL queue argument only applies to opcode `DMTR_OPC_ACCEPT`");
    }
}

int dmtr::io_queue::task::to_qresult(dmtr_qresult_t * const qr_out, int qd) const {
    if (NULL != qr_out) {
        *qr_out = {};
        qr_out->qr_opcode = this->opcode;
        qr_out->qr_qd = qd;
        qr_out->qr_tid = DMTR_TID_NIL;
    }

    if (!this->done) {
        return EAGAIN;
    }

    switch (this->opcode) {
        default:
            DMTR_UNREACHABLE();
        case DMTR_OPC_PUSH:
            return this->error;
        case DMTR_OPC_POP:
            if (0 == this->error) {
                DMTR_NOTNULL(EINVAL, qr_out);
                qr_out->qr_tid = DMTR_TID_SGA;
                qr_out->qr_value.sga = this->sga;
            }
            return this->error;
        case DMTR_OPC_ACCEPT:
            DMTR_NOTNULL(EINVAL, qr_out);
            qr_out->qr_tid = DMTR_TID_QD;
            qr_out->qr_value.qd = this->queue->qd();
            return this->error;
    }
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

int dmtr::io_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int new_qd) {
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

int dmtr::io_queue::new_task(task *&t, dmtr_qtoken_t qt, dmtr_opcode_t opcode, io_queue * const q) {
    t = NULL;
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());
    DMTR_TRUE(EINVAL, NULL == q || DMTR_OPC_ACCEPT == opcode);

    my_tasks.insert(std::make_pair(qt, task(opcode, q)));
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

