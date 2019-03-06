#include "io_queue.hh"

#include <cerrno>
#include <fcntl.h>
#include <sstream>

dmtr::io_queue::task::task() :
    my_qr{},
    my_error(-1)
{}

int dmtr::io_queue::task::new_object(std::unique_ptr<task> &task_out, completion_type completion) {
    task_out = NULL;

    auto * const t = new task();
    auto tt = std::unique_ptr<task>(t);
    DMTR_NOTNULL(ENOMEM, t);
    coroutine_type::pull_type cor([=](coroutine_type::push_type &yield) {
        dmtr_qresult_t qr = {};
        t->my_error = completion(yield, qr);
        if (0 == t->my_error) {
            t->my_qr = qr;
        }
    });
    tt->my_coroutine = std::move(cor);
    task_out = std::move(tt);
    return 0;
}

int dmtr::io_queue::task::poll(dmtr_qresult_t &qr_out) {
    qr_out = {};
    DMTR_TRUE(EINVAL, boost::none != my_coroutine);

    coroutine_type::pull_type * const cor = boost::get_pointer(my_coroutine);

    // `!cor` ==> done
    if (*cor) {
        (*cor)();
    }

    if (*cor) {
        return EAGAIN;
    }

    if (0 == my_error) {
        qr_out = my_qr;
    }

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

int dmtr::io_queue::poll(dmtr_qresult * const qr_out, dmtr_qtoken_t qt) {
    if (qr_out != NULL) {
        *qr_out = {};
    }

    task *t = NULL;
    DMTR_OK(get_task(t, qt));

    dmtr_qresult_t qr = {};
    int ret = t->poll(qr);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            break;
    }

    if (NULL == qr_out) {
        if (DMTR_OPC_PUSH == qr.qr_opcode) {
            return 0;
        }

        return EINVAL;
    }

    *qr_out = qr;
    return 0;
}

int dmtr::io_queue::drop(dmtr_qtoken_t qt)
{
    dmtr_qresult_t qr = {};
    int ret = poll(&qr, qt);
    switch (ret) {
        default:
            return ret;
        case 0:
            DMTR_OK(drop_task(qt));
            return 0;
    }
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

int dmtr::io_queue::new_task(dmtr_qtoken_t qt, task::completion_type completion) {
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());

    std::unique_ptr<task> t;
    DMTR_OK(task::new_object(t, completion));
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

int dmtr::io_queue::init_qresult(dmtr_qresult_t &qr_out) const {
    qr_out = {};
    qr_out.qr_qd = qd();
    return 0;
}

int dmtr::io_queue::init_push_qresult(dmtr_qresult_t &qr_out) const {
    DMTR_OK(init_qresult(qr_out));
    qr_out.qr_opcode = DMTR_OPC_PUSH;
    return 0;
}

int dmtr::io_queue::init_pop_qresult(dmtr_qresult_t &qr_out, const dmtr_sgarray_t &sga) const {
    DMTR_OK(init_qresult(qr_out));
    qr_out.qr_opcode = DMTR_OPC_POP;
    qr_out.qr_value.sga = sga;
    return 0;
}

int dmtr::io_queue::init_accept_qresult(dmtr_qresult_t &qr_out, int qd) const {
    DMTR_OK(init_qresult(qr_out));
    qr_out.qr_opcode = DMTR_OPC_ACCEPT;
    qr_out.qr_value.qd = qd;
    return 0;
}
