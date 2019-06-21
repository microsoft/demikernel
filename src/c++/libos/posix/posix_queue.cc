// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "posix_queue.hh"

#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <cassert>
#include <cerrno>
#include <climits>
#include <cstring>
#include <dmtr/latency.h>
#include <dmtr/sga.h>
#include <fcntl.h>
#include <iostream>
#include <dmtr/libos/io_queue_api.hh>
#include <dmtr/libos/mem.h>
#include <dmtr/libos/raii_guard.hh>
#include <netinet/tcp.h>
#include <sys/uio.h>
#include <unistd.h>

//#define DMTR_DEBUG 1
#define DMTR_PROFILE 1

#if DMTR_PROFILE
typedef std::unique_ptr<dmtr_latency_t, std::function<void(dmtr_latency_t *)>> latency_ptr_type;
static latency_ptr_type read_latency;
static latency_ptr_type write_latency;
#endif

dmtr::posix_queue::posix_queue(int qd, io_queue::category_id cid) :
    io_queue(cid, qd),
    my_fd(-1),
    my_listening_flag(false),
    my_tcp_flag(false),
    my_peer_saddr(NULL)
{}

int dmtr::posix_queue::alloc_latency()
{
    if (NULL == read_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "read"));
        read_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == write_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "write"));
        write_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }
    return 0;
}

int dmtr::posix_queue::new_net_object(std::unique_ptr<io_queue> &q_out, int qd) {
#if DMTR_PROFILE
    DMTR_OK(alloc_latency());        
#endif

    q_out = std::unique_ptr<io_queue>(new posix_queue(qd, NETWORK_Q));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

int dmtr::posix_queue::new_file_object(std::unique_ptr<io_queue> &q_out, int qd) {
#if DMTR_PROFILE
    alloc_latency();
#endif

    q_out = std::unique_ptr<io_queue>(new posix_queue(qd, FILE_Q));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

int
dmtr::posix_queue::socket(int domain, int type, int protocol)
{
    int fd = ::socket(domain, type, protocol);
    if (fd == -1) {
        return errno;
    }

    //fprintf(stderr, "Allocating socket: %d\n", fd);
    switch (type) {
        default:
            return ENOTSUP;
        case SOCK_STREAM:
            DMTR_OK(set_tcp_nodelay(fd));
            my_tcp_flag = true;
            break;
        case SOCK_DGRAM:
            DMTR_OK(set_non_blocking(fd));
            my_tcp_flag = false;
            break;
    }

    my_fd = fd;
    return 0;
}

int
dmtr::posix_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size)
{
    DMTR_NOTNULL(EINVAL, saddr);
    DMTR_NOTNULL(EINVAL, size);
    DMTR_TRUE(ERANGE, *size > 0);

    int ret = ::getsockname(my_fd, saddr, size);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case 0:
            return 0;
        case -1:
            return errno;
    }
}

int
dmtr::posix_queue::bind(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_TRUE(EINVAL, my_fd != -1);

    // Set SO_REUSEADDR
    const int n = 1;
    int ret = ::setsockopt(my_fd, SOL_SOCKET, SO_REUSEADDR, &n, sizeof(n));
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case 0:
            break;
        case -1:
            fprintf(stderr,
                "Failed to set SO_REUSEADDR on TCP listening socket");
            return errno;
    }

    ret = ::bind(my_fd, saddr, size);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case 0:
            return 0;
        case -1:
            return errno;
    }
}

int dmtr::posix_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qt, int new_qd) {
    q_out = NULL;
    DMTR_TRUE(EINVAL, my_listening_flag);
    DMTR_TRUE(EINVAL, my_tcp_flag);
    DMTR_NOTNULL(EINVAL, my_accept_thread);

    auto * const q = new posix_queue(new_qd, NETWORK_Q);
    DMTR_TRUE(ENOMEM, q != NULL);
    auto qq = std::unique_ptr<io_queue>(q);

    DMTR_OK(new_task(qt, DMTR_OPC_ACCEPT, q));
    my_accept_thread->enqueue(qt);

    q_out = std::move(qq);
    return 0;
}

int dmtr::posix_queue::accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    DMTR_TRUE(EINVAL, good());
    DMTR_TRUE(EINVAL, my_listening_flag);
    DMTR_TRUE(EINVAL, my_tcp_flag);

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        io_queue *new_q = NULL;
        DMTR_TRUE(EINVAL, t->arg(new_q));
        auto * const new_pq = dynamic_cast<posix_queue *>(new_q);
        DMTR_NOTNULL(EINVAL, new_pq);

        int new_fd = -1;
        sockaddr_in addr;
        socklen_t len = sizeof(addr);
        int ret = accept(new_fd, my_fd, reinterpret_cast<sockaddr *>(&addr), &len);
        while (EAGAIN == ret) {
            yield();
            ret = accept(new_fd, my_fd, reinterpret_cast<sockaddr *>(&addr), &len);
        }

        if (0 != ret) {
            DMTR_OK(t->complete(ret));
            // move onto the next task.
            continue;
        }

        DMTR_OK(set_tcp_nodelay(new_fd));
        DMTR_OK(set_non_blocking(new_fd));
        new_pq->my_fd = new_fd;
        new_pq->my_tcp_flag = true;
        new_pq->my_listening_flag = false;
        new_pq->start_threads();
        DMTR_OK(t->complete(0, new_pq->qd(), addr, len));
    }

    return 0;
}

int dmtr::posix_queue::accept(int &newfd_out, int fd, struct sockaddr * const saddr, socklen_t * const addrlen)
{
    DMTR_TRUE(EINVAL, NULL == saddr || (addrlen != NULL && 0 < *addrlen));

    int ret = ::accept4(fd, saddr, addrlen, SOCK_NONBLOCK);
    if (ret == -1) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return EAGAIN;
        }

        DMTR_FAIL(errno);
    }

    if (ret < -1) {
        DMTR_UNREACHABLE();
    }

    //fprintf(stderr, "Accepting connection\n");
    newfd_out = ret;
    return 0;
}

int
dmtr::posix_queue::listen(int backlog)
{
    DMTR_TRUE(EINVAL, my_fd != -1);

    int res = ::listen(my_fd, backlog);
    switch (res) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            my_listening_flag = true;
            DMTR_OK(set_non_blocking(my_fd));
            start_threads();
            return 0;
    }
}

int dmtr::posix_queue::connect(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_NULL(EPERM, my_peer_saddr);

    int res = ::connect(my_fd, saddr, size);
    switch (res) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0: {
            DMTR_OK(set_non_blocking(my_fd));

            void *p = malloc(size);
            DMTR_TRUE(ENOMEM, p != NULL);
            memcpy(p, saddr, size);
            my_peer_saddr = reinterpret_cast<struct sockaddr *>(p);

            start_threads();
            return 0;
        }
    }
}

int dmtr::posix_queue::open(const char *pathname, int flags)
{
    int fd = ::open(pathname, flags);
    if (fd == -1) {
        return errno;
    }

    my_fd = fd;
    start_threads();
    return 0;
}

int dmtr::posix_queue::open(const char *pathname, int flags, mode_t mode)
{
    int fd = ::open(pathname, flags, mode);
    if (fd == -1) {
        return errno;
    }

    my_fd = fd;
    start_threads();
    return 0;
}

int dmtr::posix_queue::creat(const char *pathname, mode_t mode)
{
    int fd = ::creat(pathname, mode);
    if (fd == -1) {
        return errno;
    }

    my_fd = fd;
    start_threads();
    return 0;
}



int dmtr::posix_queue::close()
{
    if (-1 == my_fd) {
        return 0;
    }

    int fd = my_fd;
    my_fd = -1;

    int ret = ::close(fd);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return ret;
    }
}

int dmtr::posix_queue::net_push(const dmtr_sgarray_t *sga, task::thread_type::yield_type &yield)
{
    size_t iov_len = 2 * sga->sga_numsegs + 1;
    struct iovec iov[iov_len];
    size_t data_size = 0;
    size_t message_bytes = 0;
    uint32_t seg_lens[sga->sga_numsegs];

    // calculate size and fill in iov
    for (size_t i = 0; i < sga->sga_numsegs; i++) {
        static_assert(sizeof(*seg_lens) == sizeof(sga->sga_segs[i].sgaseg_len), "type mismatch");
        const auto j = 2 * i + 1;
        seg_lens[i] = htonl(sga->sga_segs[i].sgaseg_len);
        iov[j].iov_base = &seg_lens[i];
        iov[j].iov_len = sizeof(sga->sga_segs[i].sgaseg_len);

        const auto k = j + 1;
        iov[k].iov_base = sga->sga_segs[i].sgaseg_buf;
        iov[k].iov_len = sga->sga_segs[i].sgaseg_len;

        // add up actual data size
        data_size += sga->sga_segs[i].sgaseg_len;

        // add up expected packet size (not yet including header)
        message_bytes += sga->sga_segs[i].sgaseg_len;
        message_bytes += sizeof(sga->sga_segs[i].sgaseg_len);
    }

    // fill in header
    dmtr_header_t header = {};
    header.h_magic = htonl(DMTR_HEADER_MAGIC);
    header.h_bytes = htonl(message_bytes);
    header.h_sgasegs = htonl(sga->sga_numsegs);

    // set up header at beginning of packet
    iov[0].iov_base = &header;
    iov[0].iov_len = sizeof(header);
    message_bytes += sizeof(header);

#if DMTR_DEBUG
    std::cerr << "push(" << qt << "): sending message (" << message_bytes << " bytes)." << std::endl;
#endif

    size_t bytes_written = 0;
#if DMTR_PROFILE
    auto t0 = boost::chrono::steady_clock::now();
    boost::chrono::duration<uint64_t, boost::nano> dt(0);
#endif
    while (EAGAIN == ret || bytes_written < message_bytes) {
#if DMTR_PROFILE
        dt += boost::chrono::steady_clock::now() - t0;
#endif
        yield();
#if DMTR_PROFILE
        t0 = boost::chrono::steady_clock::now();
#endif
        //Only handle partial write if it actually happened
        if (bytes_written > 0 && ret != EAGAIN) {
#if DMTR_DEBUG
            std::cout << "Handling partial write (" << bytes_written << "/" << message_bytes << ")";
            std::cout << " ret was " << strerror(ret) <<  std::endl;
#endif
            size_t nr = bytes_written;
            while (nr >= iov->iov_len) {
                nr -= iov->iov_len;
                --iov_len;
                ++iov;
            }
            assert(nr > 0); //Otherwise we would not been handling a partial write
            iov->iov_len -= nr;
            iov->iov_base = (char *) iov->iov_base + nr;
        }
        ret = writev(bytes_written, my_fd, iov, iov_len);
    }

#if DMTR_PROFILE
    dt += (boost::chrono::steady_clock::now() - t0);
    DMTR_OK(dmtr_record_latency(write_latency.get(), dt.count()));
#endif

#if DMTR_DEBUG
    std::cerr << "push(" << qt << "): sent message (" << bytes_written << " bytes)." << std::endl;
#endif

    DMTR_TRUE(ENOTSUP, bytes_written == message_bytes);

    return ret;
}

int dmtr::posix_queue::file_push(const dmtr_sgarray_t *sga, task::thread_type::yield_type &yield)
{
    size_t iov_len = sga->sga_numsegs;
    struct iovec iov[iov_len];
    size_t data_size = 0;

    for (size_t i = 0; i < sga->sga_numsegs; i++) {
        iov[i].iov_base = sga->sga_segs[i].sgaseg_buf;
        iov[i].iov_len = sga->sga_segs[i].sgaseg_len;
        data_size += sga->sga_segs[i].sgaseg_len;
#if DMTR_DEBUG
        std::cerr << "push: segs[" << i << "] addr=" << iov[i].iov_base << std::endl;
#endif
    }
#if DMTR_DEBUG
    std::cerr << "push: segs=" << iov_len << " totallen=" << data_size << std::endl;
#endif
    size_t bytes_written = 0;
    int ret = writev(bytes_written, my_fd, iov, iov_len);
    while (EAGAIN == ret) {
        yield();
        ret = writev(bytes_written, my_fd, iov, iov_len);
    }
    return ret;
}

int dmtr::posix_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(ENOTSUP, !my_listening_flag);
    DMTR_NOTNULL(EINVAL, my_push_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, sga));
    my_push_thread->enqueue(qt);

    return 0;
}

int dmtr::posix_queue::push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        const dmtr_sgarray_t *sga = NULL;
        DMTR_TRUE(EINVAL, t->arg(sga));

#if DMTR_DEBUG
        std::cerr << "push(" << qt << "): preparing message." << std::endl;
#endif

        size_t sgalen = 0;
        DMTR_OK(dmtr_sgalen(&sgalen, sga));
        if (0 == sgalen) {
            DMTR_OK(t->complete(ENOMSG));
            // move onto the next task.
            continue;
        }

        int ret = 0;
        switch (my_cid) {
        case NETWORK_Q:
            ret = net_push(sga, yield);
            break;
        case FILE_Q:
            ret = file_push(sga, yield);
            break;
        default:
            ret = ENOTSUP;
            break;
        }

        if (0 != ret) {
            DMTR_OK(t->complete(ret));
            // move onto the next task.
            continue;
        }
        DMTR_OK(t->complete(0, *sga));
    }

    return 0;
}

int dmtr::posix_queue::net_pop(dmtr_sgarray_t *sga, task::thread_type::yield_type &yield)
{

    // if we don't have a full header yet, get one.
    size_t header_bytes = 0;
    dmtr_header_t header;
    int ret = -1;
#if DMTR_PROFILE
    boost::chrono::steady_clock::time_point t0;
    boost::chrono::duration<uint64_t, boost::nano> dt(0);
#endif
    while (header_bytes < sizeof(header)) {
        uint8_t *p = reinterpret_cast<uint8_t *>(&header) + header_bytes;
        size_t remaining_bytes = sizeof(header) - header_bytes;
        size_t bytes_read = 0;
#if DMTR_DEBUG
        std::cerr << "pop(" << qt << "): attempting to read " << remaining_bytes << " bytes..." << std::endl;
#endif
#if DMTR_PROFILE
        t0 = boost::chrono::steady_clock::now();
#endif
        ret = read(bytes_read, my_fd, p, remaining_bytes);
        if (EAGAIN == ret) {
#if DMTR_PROFILE
            dt += (boost::chrono::steady_clock::now() - t0);
#endif
            yield();
            continue;

        }

        if (0 == bytes_read) {
            return ECONNABORTED;
        }
        free(iovbase);

        header_bytes += bytes_read;

#if DMTR_PROFILE
	    dt += (boost::chrono::steady_clock::now() - t0);
#endif
	}


#if DMTR_DEBUG
    std::cerr << "pop(" << qt << "): read " << header_bytes << " bytes for header." << std::endl;
#endif

    header.h_magic = ntohl(header.h_magic);
    header.h_bytes = ntohl(header.h_bytes);
    header.h_sgasegs = ntohl(header.h_sgasegs);

    if (DMTR_HEADER_MAGIC != header.h_magic) {
        return EILSEQ;
    }

#if DMTR_DEBUG
    std::cerr << "pop(" << qt << "): header magic number is correct." << std::endl;
#endif

    // grab the rest of the message
    DMTR_OK(dmtr_malloc(&sga->sga_buf, header.h_bytes));
   std::unique_ptr<uint8_t> buf(reinterpret_cast<uint8_t *>(sga->sga_buf));
    size_t data_bytes = 0;
    ret = -1;
    while (data_bytes < header.h_bytes) {
        uint8_t *p = reinterpret_cast<uint8_t *>(sga->sga_buf) + data_bytes;
        size_t remaining_bytes = header.h_bytes - data_bytes;
        size_t bytes_read = 0;
#if DMTR_DEBUG
        std::cerr << "pop(" << qt << "): attempting to read " << remaining_bytes << " bytes..." << std::endl;
#endif
#if DMTR_PROFILE
        t0 = boost::chrono::steady_clock::now();
#endif
        ret = read(bytes_read, my_fd, p, remaining_bytes);
        if (EAGAIN == ret) {
#if DMTR_PROFILE
            dt += (boost::chrono::steady_clock::now() - t0);
#endif
            yield();
            continue;
        }

        if (0 == bytes_read) {
            return ECONNABORTED;
        }

        data_bytes += bytes_read;

#if DMTR_PROFILE
        dt += (boost::chrono::steady_clock::now() - t0);
#endif
        
    }
#if DMTR_PROFILE
    DMTR_OK(dmtr_record_latency(read_latency.get(), dt.count()));
#endif

    if (0 != ret) return ret;
    
#if DMTR_DEBUG
    std::cerr << "pop(" << qt << "): read " << data_bytes << " bytes for content." << std::endl;
    std::cerr << "pop(" << qt << "): sgarray has " << header.h_sgasegs << " segments." << std::endl;
#endif

    // now we have the whole buffer, start filling sga
    uint8_t *p = reinterpret_cast<uint8_t *>(sga->sga_buf);
    sga->sga_numsegs = header.h_sgasegs;
    for (size_t i = 0; i < sga->sga_numsegs; ++i) {
        size_t seglen = ntohl(*reinterpret_cast<uint32_t *>(p));
        sga->sga_segs[i].sgaseg_len = seglen;
        //printf("[%x] sga len= %ld\n", qd, t.sga.bufs[i].len);
        p += sizeof(uint32_t);
        sga->sga_segs[i].sgaseg_buf = p;
        p += seglen;
    }

    buf.release();
    return 0;
}

int dmtr::posix_queue::file_pop(dmtr_sgarray_t *sga, task::thread_type::yield_type &yield)
{
    
    return 0;
}

int dmtr::posix_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(ENOTSUP, !my_listening_flag);
    DMTR_NOTNULL(EINVAL, my_pop_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_POP));
    my_pop_thread->enqueue(qt);

    return 0;
}

int dmtr::posix_queue::pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
#if DMTR_DEBUG
    std::cerr << "[" << qd() << "] pop thread started." << std::endl;
#endif

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        dmtr_sgarray_t sga = {};
        int ret = 0;
        switch(my_cid) {
        case NETWORK_Q:
            ret = net_pop(&sga, yield);
            break;
        case FILE_Q:
            ret = file_pop(&sga, yield);
            break;
        default:
            ret = ENOTSUP;
            break;
        }

        if (EAGAIN == ret) {
            yield();
            continue;
        }
        
        if (0 != ret) {
            DMTR_OK(t->complete(ret));
            // move onto the next task.
            continue;
        }

        //std::cerr << "pop(" << qt << "): sgarray received." << std::endl;
        DMTR_OK(t->complete(0, sga));
    }

    return 0;
}

int dmtr::posix_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{
    DMTR_OK(task::initialize_result(qr_out, qd(), qt));
    DMTR_TRUE(EINVAL, good());

    task *t;
    DMTR_OK(get_task(t, qt));

    int ret;
    switch (t->opcode()) {
        default:
            return ENOTSUP;
        case DMTR_OPC_ACCEPT:
            ret = my_accept_thread->service();
            break;
        case DMTR_OPC_PUSH:
            ret = my_push_thread->service();
            break;
        case DMTR_OPC_POP:
            ret = my_pop_thread->service();
            break;
    }

    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            break;
        case 0:
            // the threads should only exit if the queue has been closed
            // (`good()` => `false`).
            DMTR_UNREACHABLE();
    }

    return t->poll(qr_out);
}

int
dmtr::posix_queue::set_tcp_nodelay(int fd)
{
    const int n = 1;
    //printf("Setting the nagle algorithm off\n");
    if (-1 == ::setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &n, sizeof(n))) {
        return errno;
    }

    return 0;
}

int dmtr::posix_queue::read(size_t &count_out, int fd, void *buf, size_t len) {
    count_out = 0;
    DMTR_NOTNULL(EINVAL, buf);
    DMTR_TRUE(ERANGE, len <= SSIZE_MAX);

    int ret = ::read(fd, buf, len);
    if (ret < -1) {
        DMTR_UNREACHABLE();
    } else if (ret == -1) {
        return errno;
    } else {
        count_out = ret;
        return 0;
    }
}

int dmtr::posix_queue::writev(size_t &count_out, int fd, const struct iovec *iov, int iovcnt) {
    ssize_t ret = ::writev(fd, iov, iovcnt);

    if (ret == -1) {
        if (errno == EWOULDBLOCK || errno == EAGAIN) {
            // we'll try again later.
            return EAGAIN;
        }

        return errno;
    }

    if (ret < -1) {
        DMTR_UNREACHABLE();
    }

    count_out += ret;
    return 0;
}

void dmtr::posix_queue::start_threads() {
    if (my_listening_flag) {
        my_accept_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return accept_thread(yield, tq);
        }));
    } else {
        my_push_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return push_thread(yield, tq);
        }));

        my_pop_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return pop_thread(yield, tq);
        }));
    }
}
