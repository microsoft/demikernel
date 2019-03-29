#include "io_queue.hh"

#include <cerrno>
#include <fcntl.h>
#include <sstream>

dmtr::io_queue::task::completion_adaptor::completion_adaptor(task &t, int &error, io_queue &q, completion_type completion) :
    my_task(&t),
    my_error(&error),
    my_queue(&q),
    my_completion(completion)
{}

dmtr::io_queue::task::task() :
    my_coroutine(coroutine_nop)
{}

void dmtr::io_queue::task::start_coroutine(completion_type completion, io_queue &q) {
    coroutine_type::pull_type cor{completion_adaptor(*this, my_error, q, completion)};
    my_coroutine = std::move(cor);
}

int dmtr::io_queue::task::new_object(std::unique_ptr<task> &task_out, io_queue &q,  dmtr_qtoken_t qt, dmtr_opcode_t opcode) {
    task_out = NULL;
    DMTR_NONZERO(EINVAL, qt);
    DMTR_TRUE(EINVAL, DMTR_OPC_INVALID != opcode);

    auto * const t = new task();
    DMTR_NOTNULL(ENOMEM, t);
    auto tt = std::unique_ptr<task>(t);

    DMTR_OK(initialize_result(t->my_qr, q.qd(), qt));
    t->my_qr.qr_opcode = opcode;
    t->my_error = -1;

    task_out = std::move(tt);
    return 0;
}

int dmtr::io_queue::task::new_object(std::unique_ptr<task> &task_out, io_queue &q,  dmtr_qtoken_t qt, dmtr_opcode_t opcode, completion_type completion) {
    DMTR_NOTNULL(EINVAL, completion);

    DMTR_OK(new_object(task_out, q, qt, opcode));
    task_out->start_coroutine(completion, q);
    return 0;
}

int dmtr::io_queue::task::new_object(std::unique_ptr<task> &task_out, io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, completion_type completion, const dmtr_sgarray_t &arg) {
    DMTR_NOTNULL(EINVAL, completion);
    DMTR_NONZERO(EINVAL, arg.sga_numsegs);

    DMTR_OK(new_object(task_out, q, qt, opcode));
    task_out->my_sga_arg = arg;
    task_out->start_coroutine(completion, q);
    return 0;
}

int dmtr::io_queue::task::new_object(std::unique_ptr<task> &task_out, io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, completion_type completion, io_queue *arg) {
    DMTR_NOTNULL(EINVAL, arg);

    DMTR_OK(new_object(task_out, q, qt, opcode));
    task_out->my_queue_arg = arg;
    task_out->start_coroutine(completion, q);
    return 0;
}

int dmtr::io_queue::task::initialize_result(dmtr_qresult_t &qr, int qd, dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, qd > 0);
    DMTR_TRUE(EINVAL, qt != 0);

    qr.qr_qd = qd;
    qr.qr_qt = qt;
    return 0;
}

void dmtr::io_queue::task::coroutine_nop(yield_type &)
{}

int dmtr::io_queue::task::poll(dmtr_qresult_t &qr_out) {
    if (!done()) {
        my_coroutine();
    }

    if (my_coroutine) {
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
    my_qd(qd)
{}

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

int dmtr::io_queue::connect(const struct sockaddr * const saddr, socklen_t size) {
    return ENOTSUP;
}

int dmtr::io_queue::close() {
    return 0;
}

int dmtr::io_queue::poll(dmtr_qresult &qr_out, dmtr_qtoken_t qt) {
    qr_out = {};
    // some systems rely upon the queue descriptor and queue token being
    // valid, even after a failure. we provide this information as early as
    // possible.
    DMTR_OK(task::initialize_result(qr_out, qd(), qt));

    task *t = NULL;
    DMTR_OK(get_task(t, qt));

    return t->poll(qr_out);
}

int dmtr::io_queue::drop(dmtr_qtoken_t qt)
{
    dmtr_qresult_t qr;
    int ret = poll(qr, qt);
    switch (ret) {
        default:
            return ret;
        case 0:
            DMTR_OK(drop_task(qt));
            return 0;
    }
}

int dmtr::io_queue::set_non_blocking(int fd) {
    //printf("Set non blocking\n");
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

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion) {
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());

    std::unique_ptr<task> t;
    DMTR_OK(task::new_object(t, *this, qt, opcode, completion));
    my_tasks.insert(std::make_pair(qt, std::move(t)));
    return 0;
}

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion, const dmtr_sgarray_t &arg) {
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());

    std::unique_ptr<task> t;
    DMTR_OK(task::new_object(t, *this, qt, opcode, completion, arg));
    my_tasks.insert(std::make_pair(qt, std::move(t)));
    return 0;
}

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, task::completion_type completion, io_queue *arg) {
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());

    std::unique_ptr<task> t;
    DMTR_OK(task::new_object(t, *this, qt, opcode, completion, arg));
    my_tasks.insert(std::make_pair(qt, std::move(t)));
    return 0;
}

int dmtr::io_queue::get_task(task *&t, dmtr_qtoken_t qt) {
    t = NULL;
    auto it = my_tasks.find(qt);
    DMTR_TRUE(ENOENT, it != my_tasks.cend());

    t = it->second.get();
    DMTR_NOTNULL(EPERM, t);
    return 0;
}

int dmtr::io_queue::drop_task(dmtr_qtoken_t qt) {
    auto it = my_tasks.find(qt);
    DMTR_TRUE(ENOENT, it != my_tasks.cend());
    my_tasks.erase(it);
    return 0;
}

void dmtr::io_queue::task::complete(const dmtr_sgarray_t &sga) {
    my_qr.qr_value.sga = sga;
}

void dmtr::io_queue::task::complete(int qd, const sockaddr_in &addr, socklen_t len) {
    my_qr.qr_value.ares.qd = qd;
    my_qr.qr_value.ares.addr = addr;
    my_qr.qr_value.ares.len = len;
}
