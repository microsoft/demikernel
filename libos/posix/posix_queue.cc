// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/posix/posix-queue.cc
 *   POSIX implementation of dmtr queue interface
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#include "posix_queue.hh"

#include <arpa/inet.h>
#include <cassert>
#include <cerrno>
#include <climits>
#include <cstring>
#include <dmtr/sga.h>
#include <fcntl.h>
#include <iostream>
#include <libos/common/io_queue_api.hh>
#include <libos/common/mem.h>
#include <libos/common/raii_guard.hh>
#include <netinet/tcp.h>
#include <sys/uio.h>
#include <unistd.h>

dmtr::posix_queue::posix_queue(int qd) :
    io_queue(NETWORK_Q, qd),
    my_fd(-1),
    my_listening_flag(false),
    my_tcp_flag(false),
    my_peer_saddr(NULL)
{}

int dmtr::posix_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = std::unique_ptr<io_queue>(new posix_queue(qd));
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
    DMTR_TRUE(EPERM, my_listening_flag);
    DMTR_TRUE(EPERM, my_tcp_flag);

    auto * const q = new posix_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);
    auto qq = std::unique_ptr<io_queue>(q);

    DMTR_OK(new_task(qt, DMTR_OPC_ACCEPT, [=](task::yield_type &yield, dmtr_qresult_t &qr_out) {
        int new_fd = -1;
        int ret = EAGAIN;
        sockaddr_in addr;
        socklen_t len = sizeof(addr);
        while (EAGAIN == ret) {
            ret = accept(new_fd, my_fd, reinterpret_cast<sockaddr *>(&addr), &len);
            yield();
        }

        switch (ret) {
            default:
                DMTR_FAIL(ret);
            case EAGAIN:
                DMTR_UNREACHABLE();
            case 0:
                break;
        }

        DMTR_OK(set_tcp_nodelay(new_fd));
        DMTR_OK(set_non_blocking(new_fd));
        q->my_fd = new_fd;
        q->my_tcp_flag = true;
        set_accept_qresult(qr_out, new_qd, addr, len);
        return 0;
    }));

    q_out = std::move(qq);
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

            return 0;
        }
    }
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

int dmtr::posix_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, [=](task::yield_type &yield, dmtr_qresult_t &qr_out) {
        //std::cerr << "push(" << qt << "): preparing message." << std::endl;

        size_t sgalen = 0;
        DMTR_OK(dmtr_sgalen(&sgalen, &sga));
        if (0 == sgalen) {
            return ENOMSG;
        }

        size_t iov_len = 2 * sga.sga_numsegs + 1;
        struct iovec iov[iov_len];
        size_t data_size = 0;
        size_t message_bytes = 0;
        uint32_t seg_lens[sga.sga_numsegs] = {};

        // calculate size and fill in iov
        for (size_t i = 0; i < sga.sga_numsegs; i++) {
            static_assert(sizeof(*seg_lens) == sizeof(sga.sga_segs[i].sgaseg_len), "type mismatch");
            const auto j = 2 * i + 1;
            seg_lens[i] = htonl(sga.sga_segs[i].sgaseg_len);
            iov[j].iov_base = &seg_lens[i];
            iov[j].iov_len = sizeof(sga.sga_segs[i].sgaseg_len);

            const auto k = j + 1;
            iov[k].iov_base = sga.sga_segs[i].sgaseg_buf;
            iov[k].iov_len = sga.sga_segs[i].sgaseg_len;

            // add up actual data size
            data_size += sga.sga_segs[i].sgaseg_len;

            // add up expected packet size (not yet including header)
            message_bytes += sga.sga_segs[i].sgaseg_len;
            message_bytes += sizeof(sga.sga_segs[i].sgaseg_len);
        }

        // fill in header
        dmtr_header_t header = {};
        header.h_magic = htonl(DMTR_HEADER_MAGIC);
        header.h_bytes = htonl(message_bytes);
        header.h_sgasegs = htonl(sga.sga_numsegs);

        // set up header at beginning of packet
        iov[0].iov_base = &header;
        iov[0].iov_len = sizeof(header);
        message_bytes += sizeof(header);

        //std::cerr << "push(" << qt << "): sending message (" << message_bytes << " bytes)." << std::endl;
        size_t bytes_written = 0;
        bool done = false;
        while (!done) {
            int ret = writev(bytes_written, my_fd, iov, iov_len);
            switch (ret) {
                default:
                    DMTR_FAIL(ret);
                case EAGAIN:
                    yield();
                    continue;
                case 0:
                    done = true;
                    break;
            }
        }
        //std::cerr << "push(" << qt << "): sent message (" << bytes_written << " bytes)." << std::endl;

        DMTR_TRUE(ENOTSUP, bytes_written == message_bytes);

        set_push_qresult(qr_out, sga);
        return 0;
    }));

    return 0;
}

int dmtr::posix_queue::pop(dmtr_qtoken_t qt)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    DMTR_OK(new_task(qt, DMTR_OPC_POP, [=](task::yield_type &yield, dmtr_qresult_t &qr_out) {
        while (boost::none != my_active_recv) {
            yield();
        }

        my_active_recv = qt;
        raii_guard rg0([=]() {
            my_active_recv = boost::none;
        });

        // if we don't have a full header yet, get one.
        size_t header_bytes = 0;
        dmtr_header_t header;
        while (header_bytes < sizeof(header)) {
            uint8_t *p = reinterpret_cast<uint8_t *>(&header) + header_bytes;
            size_t remaining_bytes = sizeof(header) - header_bytes;
            size_t bytes_read = 0;
            //std::cerr << "pop(" << qt << "): attempting to read " << remaining_bytes << " bytes..." << std::endl;
            int ret = read(bytes_read, my_fd, p, remaining_bytes);
            switch (ret) {
                default:
                    DMTR_FAIL(ret);
                case EAGAIN:
                    yield();
                    continue;
                case 0:
                    break;
            }

            if (0 == bytes_read) {
                return ECONNABORTED;
            }

            header_bytes += bytes_read;
        }

        //std::cerr << "pop(" << qt << "): read " << header_bytes << " bytes for header." << std::endl;

        header.h_magic = ntohl(header.h_magic);
        header.h_bytes = ntohl(header.h_bytes);
        header.h_sgasegs = ntohl(header.h_sgasegs);

        if (DMTR_HEADER_MAGIC != header.h_magic) {
            return EILSEQ;
        }

        //std::cerr << "pop(" << qt << "): header magic number is correct." << std::endl;

        // grab the rest of the message
        dmtr_sgarray_t sga = {};
        DMTR_OK(dmtr_malloc(&sga.sga_buf, header.h_bytes));
        std::unique_ptr<uint8_t> buf(reinterpret_cast<uint8_t *>(sga.sga_buf));
        size_t data_bytes = 0;
        while (data_bytes < header.h_bytes) {
            uint8_t *p = reinterpret_cast<uint8_t *>(sga.sga_buf) + data_bytes;
            size_t remaining_bytes = header.h_bytes - data_bytes;
            size_t bytes_read = 0;
            //std::cerr << "pop(" << qt << "): attempting to read " << remaining_bytes << " bytes..." << std::endl;
            int ret = read(bytes_read, my_fd, p, remaining_bytes);
            switch (ret) {
                default:
                    DMTR_FAIL(ret);
                case EAGAIN:
                    yield();
                    continue;
                case 0:
                    break;
            }

            if (0 == bytes_read) {
                return ECONNABORTED;
            }

            data_bytes += bytes_read;
        }

        //std::cerr << "pop(" << qt << "): read " << data_bytes << " bytes for content." << std::endl;
        //std::cerr << "pop(" << qt << "): sgarray has " << header.h_sgasegs << " segments." << std::endl;

        // now we have the whole buffer, start filling sga
        uint8_t *p = reinterpret_cast<uint8_t *>(sga.sga_buf);
        sga.sga_numsegs = header.h_sgasegs;
        for (size_t i = 0; i < sga.sga_numsegs; ++i) {
            size_t seglen = ntohl(*reinterpret_cast<uint32_t *>(p));
            sga.sga_segs[i].sgaseg_len = seglen;
            //printf("[%x] sga len= %ld\n", qd, t.sga.bufs[i].len);
            p += sizeof(uint32_t);
            sga.sga_segs[i].sgaseg_buf = p;
            p += seglen;
        }

        //std::cerr << "pop(" << qt << "): sgarray received." << std::endl;
        buf.release();
        set_pop_qresult(qr_out, sga);
        return 0;
    }));
    return 0;
}

int dmtr::posix_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{
    qr_out = {};
    DMTR_TRUE(EINVAL, my_fd != -1);

    return io_queue::poll(qr_out, qt);
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
    count_out = 0;
    ssize_t ret = ::writev(fd, iov, iovcnt);

    if (ret == -1) {
        if (errno == EWOULDBLOCK || errno == EAGAIN) {
            // we'll try again later.
            return EAGAIN;
        }

        DMTR_FAIL(errno);
    }

    if (ret < -1) {
        DMTR_UNREACHABLE();
    }

    count_out = ret;
    return 0;
}
