// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/libos/io_queue.hh>

#include <cerrno>
#include <dmtr/annot.h>
#include <fcntl.h>
#include <sstream>



dmtr::io_queue::task::task() :
    valid(false),
    my_qr{},
    my_error(EAGAIN),
    my_sga_arg{},
    my_queue_arg(nullptr)
{}

int dmtr::io_queue::task::initialize(io_queue &q,  dmtr_qtoken_t qt, dmtr_opcode_t opcode) {
    DMTR_NONZERO(EINVAL, qt);
    DMTR_TRUE(EINVAL, DMTR_OPC_INVALID != opcode);
    DMTR_TRUE(EINVAL, valid == false);
    DMTR_OK(initialize_result(my_qr, q.qd(), qt));
    
    my_qr.qr_opcode = opcode;
    my_error = EAGAIN;
    valid = true;

    return 0;
}

int dmtr::io_queue::task::initialize(io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, const dmtr_sgarray_t &arg) {
    DMTR_NONZERO(EINVAL, arg.sga_numsegs);
    DMTR_OK(initialize(q, qt, opcode));
    my_sga_arg = arg;
    return 0;
}

int dmtr::io_queue::task::initialize(io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, io_queue *arg) {
    DMTR_NOTNULL(EINVAL, arg);
    DMTR_OK(initialize(q, qt, opcode));
    my_queue_arg = arg;
    return 0;
}

int dmtr::io_queue::task::initialize_result(dmtr_qresult_t &qr_out, int qd, dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, qd > 0);
    DMTR_TRUE(EINVAL, qt != 0);
    
    qr_out.qr_opcode = DMTR_OPC_INVALID;
    qr_out.qr_qd = qd;
    qr_out.qr_qt = qt;
    return 0;
}

int dmtr::io_queue::task::poll(dmtr_qresult_t &qr_out) const {
    if (!done()) {
        return EAGAIN;
    }

    qr_out = my_qr;
    return my_error;
}

bool dmtr::io_queue::task::arg(const dmtr_sgarray_t *&arg_out) const {
    if (0 == my_sga_arg.sga_numsegs) {
        return false;
    }

    arg_out = &my_sga_arg;
    return true;
}

bool dmtr::io_queue::task::arg(io_queue *&arg_out) const {
    if (NULL == my_queue_arg) {
        return false;
    }

    arg_out = my_queue_arg;
    return true;
}

dmtr::io_queue::io_queue(enum category_id cid, int qd) :
    my_cid(cid),
    my_qd(qd),
    my_qt_counter(0)
{
    //my_tasks.reserve(5);
}

dmtr::io_queue::~io_queue()
{
    int ret = close();
    if (0 != ret) {
        std::ostringstream msg;
        msg << "Failed to close `io_queue` object (error " << ret << ")." << std::endl;
        DMTR_PANIC(msg.str().c_str());
    }
}

int dmtr::io_queue::socket(int domain, int type, int protocol) {
    return ENOTSUP;
}

int dmtr::io_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size) {
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

int dmtr::io_queue::connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size) {
    return ENOTSUP;
}

int dmtr::io_queue::open(const char *pathname, int flags) {
    return ENOTSUP;
}

int dmtr::io_queue::open2(const char *pathname, int flags, mode_t mode) {
    return ENOTSUP;
}

int dmtr::io_queue::creat(const char *pathname, mode_t mode) {
    return ENOTSUP;
}

int dmtr::io_queue::close() {
    return 0;
}

int dmtr::io_queue::drop(dmtr_qtoken_t qt)
{
    DMTR_OK(drop_task(qt));
    return 0;
}

int dmtr::io_queue::set_non_blocking(int fd) {
    ////printf("Set non blocking\n");
    int ret = fcntl(fd, F_GETFL);
    if (-1 == ret) {
        return errno;
    }

    int flags = ret;
    if (-1 == fcntl(fd, F_SETFL, flags | O_NONBLOCK, 1)) {
        return errno;
    }

    return 0;
}


int dmtr::io_queue::new_qtoken(dmtr_qtoken_t &qt_out) {
    DMTR_TRUE(EINVAL, my_qd != 0);

    uint32_t u = ++my_qt_counter;
    if (u == UINT32_MAX) {
        DMTR_PANIC("Queue token overflow");
    }

    while (has_task(u)) {
        //printf("finding another task slot and token\n");
        u = ++my_qt_counter;
        if (u == UINT32_MAX) {
            DMTR_PANIC("Queue token overflow");
        }
    }
    qt_out = static_cast<uint64_t>(u) | (static_cast<uint64_t>(my_qd) << QD_OFFSET);
    //printf("Allocating qtoken: %lu\n", qt_out);
    return 0;
    
}

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode) {
    DMTR_TRUE(EEXIST, !has_task(qt));
    insert_task(qt);
    DMTR_OK(get_task(qt)->initialize(*this, qt, opcode));
    return 0;
}

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, const dmtr_sgarray_t &arg) {
    DMTR_TRUE(EEXIST, !has_task(qt));
    insert_task(qt);
    DMTR_OK(get_task(qt)->initialize(*this, qt, opcode, arg));
    return 0;
}

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, io_queue *arg) {
    DMTR_TRUE(EEXIST, !has_task(qt));
    insert_task(qt);
    DMTR_OK(get_task(qt)->initialize(*this, qt, opcode, arg));
    return 0;
}

void dmtr::io_queue::insert_task(dmtr_qtoken_t qt) {
#ifndef MAX_TASKS
    my_tasks.insert(std::make_pair(qt, task()));
#endif
}

bool dmtr::io_queue::has_task(dmtr_qtoken_t qt) {
#ifdef MAX_TASKS
    return get_task(qt)->is_valid();
#else
    auto it = my_tasks.find(qt);
    return it != my_tasks.end();
#endif
}

int dmtr::io_queue::get_task(task *&t_out, dmtr_qtoken_t qt) {
    DMTR_TRUE(ENOENT, has_task(qt));
    t_out = get_task(qt);
    DMTR_NOTNULL(ENOTSUP, t_out);
    
    return 0;
    // if (my_tasks[qt % MAX_TASKS].valid) {
    //     t_out = &my_tasks[qt % MAX_TASKS];
    //     return 0;
    // } else {
    //     return -1;
    // }
}

dmtr::io_queue::task * dmtr::io_queue::get_task(dmtr_qtoken_t qt) {
#ifdef MAX_TASKS
    return &my_tasks[qt % MAX_TASKS];
#else
    return &my_tasks.find(qt)->second;
#endif
}
int dmtr::io_queue::drop_task(dmtr_qtoken_t qt) {
    //printf("Dropping task %lx\n", qt);
    get_task(qt)->clear();
    return 0;
}

int dmtr::io_queue::task::complete(int error) {
    DMTR_TRUE(EINVAL, error != EAGAIN);
    my_error = error;
    return 0;
}

int dmtr::io_queue::task::complete(int error, const dmtr_sgarray_t &sga) {
    DMTR_OK(complete(error));
    my_qr.qr_value.sga = sga;
    return 0;
}

int dmtr::io_queue::task::complete(int error, int new_qd, const sockaddr_in &addr) {
    DMTR_OK(complete(error));
    my_qr.qr_value.ares.qd = new_qd;
    my_qr.qr_value.ares.addr = addr;
    return 0;
}
