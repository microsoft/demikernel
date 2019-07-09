// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/libos/io_queue_api.hh>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/libos/memory_queue.hh>
#include <cassert>
#include <cstdlib>
#include <iostream>
#include <unistd.h>

dmtr::io_queue_api::io_queue_api() :
    my_qd_counter(0),
    my_qt_counter(0)
{}

dmtr::io_queue_api::~io_queue_api()
{}

std::string dmtr_log_directory;

int dmtr::io_queue_api::init(io_queue_api *&newobj_out, int argc, char *argv[]) {
    DMTR_NULL(EINVAL, newobj_out);

    newobj_out = new io_queue_api();

    if (argc > 0) {
        std::string log_directory;
        namespace po = boost::program_options;
        po::options_description desc{"IO queue API options"};
        desc.add_options()
            ("log-dir,L", po::value<std::string>(&log_directory)->default_value("./"), "Log directory");
        po::variables_map vm;
        try {
            po::store(po::parse_command_line(argc, argv, desc), vm);
            po::notify(vm);
        } catch (const po::error &e) {
            std::cerr << e.what() << std::endl;
            std::cerr << desc << std::endl;
            exit(0);
        }
        dmtr_log_directory = log_directory;
    }

    return 0;
}

int dmtr::io_queue_api::register_queue_ctor(enum io_queue::category_id cid, io_queue_factory::ctor_type ctor) {
    DMTR_OK(my_queue_factory.register_ctor(cid, ctor));
    return 0;
}

int dmtr::io_queue_api::get_queue(io_queue *&q_out, int qd) const {
    q_out = NULL;

    std::lock_guard<std::mutex> lock(my_queues_mutex);
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
    std::lock_guard<std::mutex> lock(my_queues_mutex);
    if (my_queues.find(qd) != my_queues.cend()) {
        return EEXIST;
    }

    my_queues[qd] = std::move(q);
    return 0;
}

int dmtr::io_queue_api::remove_queue(int qd) {
    DMTR_TRUE(EINVAL, qd != 0);

    std::lock_guard<std::mutex> lock(my_queues_mutex);
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

int dmtr::io_queue_api::open(int &qd_out, const char *pathname, int flags) {
    qd_out = 0;

    io_queue *q = NULL;
    DMTR_OK(new_queue(q, io_queue::FILE_Q));

    int ret = q->open(pathname, flags);
    if (ret != 0) {
        DMTR_OK(remove_queue(q->qd()));
        DMTR_FAIL(ret);
    }

    qd_out = q->qd();
    return 0;
}

int dmtr::io_queue_api::open(int &qd_out, const char *pathname, int flags, mode_t mode) {
    qd_out = 0;

    io_queue *q = NULL;
    DMTR_OK(new_queue(q, io_queue::FILE_Q));

    int ret = q->open(pathname, flags, mode);
    if (ret != 0) {
        DMTR_OK(remove_queue(q->qd()));
        DMTR_FAIL(ret);
    }

    qd_out = q->qd();
    return 0;
}

int dmtr::io_queue_api::creat(int &qd_out, const char *pathname, mode_t mode) {
    qd_out = 0;

    io_queue *q = NULL;
    DMTR_OK(new_queue(q, io_queue::FILE_Q));

    int ret = q->creat(pathname, mode);
    if (ret != 0) {
        DMTR_OK(remove_queue(q->qd()));
        DMTR_FAIL(ret);
    }

    qd_out = q->qd();
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
    qtok_out = 0;
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    dmtr_qtoken_t qt;
    DMTR_OK(new_qtoken(qt, qd));
    DMTR_OK(q->push(qt, sga));

    qtok_out = qt;
    return 0;
}

int dmtr::io_queue_api::pop(dmtr_qtoken_t &qtok_out, int qd) {
    qtok_out = 0;
    DMTR_TRUE(EINVAL, qd != 0);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    dmtr_qtoken_t qt;
    DMTR_OK(new_qtoken(qt, qd));
    DMTR_OK(q->pop(qt));

    qtok_out = qt;
    return 0;
}

int dmtr::io_queue_api::poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, qt != 0);

    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));

    dmtr_qresult_t unused_qr = {};
    if (NULL == qr_out) {
        qr_out = &unused_qr;
    }

    int ret = q->poll(*qr_out, qt);
    switch (ret) {
        default:
            on_poll_failure(qr_out, this);
            DMTR_FAIL(ret);
        case EAGAIN:
        case ECONNABORTED:
        case ECONNRESET:
        // `EBADF` can occur if the queue is closed before completion.
        case EBADF:
            on_poll_failure(qr_out, this);
            return ret;
        case 0:
            return 0;
    }

    return ret;
}

void dmtr::io_queue_api::on_poll_failure(dmtr_qresult_t * const qr_out, io_queue_api *self)  {
    // this is called from a destructor, so we need to be cautious not to
    // trigger an exception in this method.
    if (NULL == qr_out) {
        std::cerr << "Unexpected NULL pointer `q_out` in dmtr::io_queue_api::on_poll_failure()." << std::endl;
        abort();
    }

    if (NULL == self) {
        std::cerr << "Unexpected NULL pointer `self` in dmtr::io_queue_api::on_poll_failure()." << std::endl;
        abort();
    }

    // if there's a failure on an accept token, we remove the queue
    // we created at the beginning of the operation.
    if (DMTR_OPC_ACCEPT == qr_out->qr_opcode) {
        (void)self->remove_queue(qr_out->qr_value.ares.qd);
        qr_out->qr_value.ares.qd = 0;
    }
}

int dmtr::io_queue_api::drop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, qt != 0);

    int qd = qttoqd(qt);

    io_queue *q = NULL;
    DMTR_OK(get_queue(q, qd));
    return q->drop(qt);
}
