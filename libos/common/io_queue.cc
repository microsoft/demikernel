#include "io_queue.hh"

#include <cerrno>
#include <fcntl.h>
#include <sstream>

dmtr::io_queue::task::task() :
    my_qr{},
    my_error(-1),
    my_coroutine([=](yield_type &){})
{}

int dmtr::io_queue::task::new_object(std::unique_ptr<task> &task_out, completion_type completion) {
    task_out = NULL;

    auto * const t = new task();
    auto tt = std::unique_ptr<task>(t);
    DMTR_NOTNULL(ENOMEM, t);
    coroutine_type::pull_type cor([=](yield_type &yield) {
        t->my_error = completion(yield, t->my_qr);
    });
    tt->my_coroutine = std::move(cor);
    task_out = std::move(tt);
    return 0;
}

int dmtr::io_queue::task::poll(dmtr_qresult_t &qr_out) {
    // `!my_coroutine` ==> done
    if (my_coroutine) {
        my_coroutine();
    }

    if (my_coroutine) {
        return EAGAIN;
    }

    qr_out = my_qr;
    return my_error;
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
    init_qresult(qr_out, qt);
    DMTR_TRUE(EPERM, qd() != 0);

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
    int qd = this->qd();
    DMTR_TRUE(EPERM, qd != 0);

    std::unique_ptr<task> t;
    DMTR_OK(task::new_object(t, [=](task::yield_type &yield, dmtr_qresult_t &qr) {
        int ret = completion(yield, qr);
        DMTR_OK(init_qresult(qr, qt, qd));
        qr.qr_opcode = opcode;
        return ret;
    }));

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

int dmtr::io_queue::init_qresult(dmtr_qresult_t &qr, dmtr_qtoken_t qt, int qd) const {
    DMTR_TRUE(EINVAL, qd != 0);

    qr.qr_qd = qd;
    qr.qr_opcode = DMTR_OPC_INVALID;
    qr.qr_qt = qt;
    return 0;
}

int dmtr::io_queue::init_qresult(dmtr_qresult_t &qr, dmtr_qtoken_t qt) const {
    int qd = this->qd();
    DMTR_TRUE(EPERM, qd != 0);

    init_qresult(qr, qt, qd);
    return 0;
}


void dmtr::io_queue::set_qresult(dmtr_qresult_t &qr_out, const dmtr_sgarray_t &sga) const {
    qr_out.qr_value.sga = sga;
}

void dmtr::io_queue::set_qresult(dmtr_qresult_t &qr_out, int qd, const sockaddr_in &addr, socklen_t len) const {
    qr_out.qr_value.ares.qd = qd;
    qr_out.qr_value.ares.addr = addr;
    qr_out.qr_value.ares.len = len;
}
