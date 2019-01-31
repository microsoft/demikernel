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

#include <dmtr/mem.h>
#include <libos/common/latency.h>
#include <libos/common/io_queue_api.hh>

#include <arpa/inet.h>
#include <cassert>
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <netinet/tcp.h>
#include <sys/uio.h>
#include <unistd.h>
#include <climits>

DEFINE_LATENCY(dev_read_latency);
DEFINE_LATENCY(dev_write_latency);

dmtr::posix_queue::posix_queue(int qd) :
    io_queue(NETWORK_Q, qd),
    my_fd(-1),
    my_listening_flag(false),
    my_tcp_flag(false),
    my_peer_saddr(NULL)
{}

int dmtr::posix_queue::new_object(io_queue *&q_out, int qd) {
    q_out = new posix_queue(qd);
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
dmtr::posix_queue::bind(const struct sockaddr * const saddr, socklen_t size)
{
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

int
dmtr::posix_queue::accept(io_queue *&q_out, struct sockaddr * const saddr_out, socklen_t * const size_out, int new_qd)
{
    DMTR_TRUE(EPERM, my_listening_flag);

    auto * const q = new posix_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);

    int ret = ::accept4(my_fd, saddr_out, size_out, SOCK_NONBLOCK);
    if (ret == -1) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return EAGAIN;
        }

        DMTR_OK(errno);
        DMTR_UNREACHABLE()
    }

    if (ret < -1) {
        DMTR_UNREACHABLE();
    }

    //fprintf(stderr, "Accepting connection\n");
    int new_fd = ret;
    DMTR_OK(set_tcp_nodelay(new_fd));
    DMTR_OK(set_non_blocking(new_fd));
    q->my_fd = new_fd;
    q->my_tcp_flag = my_tcp_flag;
    q_out = q;
    return 0;
}

int
dmtr::posix_queue::listen(int backlog)
{
    int res = ::listen(my_fd, backlog);
    switch (res) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            my_listening_flag = true;
            // Always put it in non-blocking mode
            if (-1 == fcntl(my_fd, F_SETFL, O_NONBLOCK, 1)) {
                fprintf(stderr,
                    "Failed to set O_NONBLOCK on outgoing dmtr socket");
                return errno;
            }
            return 0;
    }
}

int
dmtr::posix_queue::connect(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_NULL(my_peer_saddr);

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

int
dmtr::posix_queue::close()
{
    int ret = ::close(my_fd);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int
dmtr::posix_queue::process_incoming(pending_request &req)
{
    DMTR_TRUE(EPERM, !my_listening_flag);

    //printf("process_incoming qd:%d\n", qd);
    // if we don't have a full header in our buffer, then get one
    if (my_tcp_flag) {
        if (req.num_bytes < sizeof(req.header)) {
            // note: in TCP mode, we read the header directly into
            // `req.header`. in UDP mode, we read into `req.buf` and
            // then copy the information over to `req.header`.
            uint8_t *p = reinterpret_cast<uint8_t *>(&req.header) + req.num_bytes;
            size_t len = sizeof(req.header) - req.num_bytes;
            size_t count = 0;
            int err = read(count, my_fd, p, len);
            switch (err) {
                default:
                    req.done = true;
                    req.error = err;
                    return 0;
                case EAGAIN:
                    return 0;
                case 0:
                    req.num_bytes += count;
                    break;
            }
        }
    } else {
        DMTR_TRUE(EPERM, req.sga.sga_buf == NULL);
        DMTR_TRUE(EPERM, req.sga.sga_addr == NULL);

        req.sga.sga_addrlen = sizeof(struct sockaddr_in);
        DMTR_OK(dmtr_malloc(&req.sga.sga_buf, 1024));
        void *p = NULL;
        DMTR_OK(dmtr_malloc(&p, req.sga.sga_addrlen));
        req.sga.sga_addr = reinterpret_cast<struct sockaddr *>(p);

        size_t count;
        int err = recvfrom(count, my_fd, req.sga.sga_buf, 1024, 0, req.sga.sga_addr, &req.sga.sga_addrlen);
        switch (err) {
            default:
                req.done = true;
                req.error = err;
                return 0;
            case EAGAIN:
                return 0;
            case 0:
                break;
        }

        req.num_bytes = count;
        memcpy(&req.header, req.sga.sga_buf, sizeof(req.header));
    }

    if (req.num_bytes < sizeof(req.header)) {
        req.done = true;
        // if we haven't read any bytes, it's a sign that the connection
        // was dropped.
        req.error = req.num_bytes == 0 ? ECONNABORTED : EPROTO;
        return 0;
    }

    //fprintf(stderr, "[%x] process_incoming: first read=%ld\n", qd, count);
    if (req.header.h_magic != DMTR_HEADER_MAGIC) {
        // not a correctly formed packet
        //fprintf(stderr, "Could not find magic %lx\n", req.header.h_magic);
        req.done = true;
        req.error = EILSEQ;
        return 0;
    }

    size_t data_len = req.header.h_bytes;
    if (my_tcp_flag) {
        // now we'll allocate a buffer
        if (req.sga.sga_buf == NULL) {
            DMTR_OK(dmtr_malloc(&req.sga.sga_buf, data_len));
        }

        // grab the rest of the packet
        if (req.num_bytes < sizeof(req.header) + data_len) {
            size_t offset = req.num_bytes - sizeof(req.header);
            uint8_t *p = reinterpret_cast<uint8_t *>(req.sga.sga_buf) + offset;
            size_t len = data_len - offset;
            size_t count = 0;
            int err = read(count, my_fd, p, len);
            //fprintf(stderr, "[%x] Next read size=%ld\n", qd, count);
            switch (err) {
                default:
                    req.done = true;
                    req.error = err;
                    return 0;
                case EAGAIN:
                    return 0;
                case 0:
                    req.done = (0 == count);
                    req.num_bytes += count;
                    break;
            }

            if (req.num_bytes < sizeof(req.header) + data_len) {
                if (req.done) {
                    req.error = EPROTO;
                }

                return 0;
            }
        }
        //fprintf(stderr, "[%x] data read length=%ld\n", qd, data_len);
    }

    // now we have the whole buffer, start filling sga
    uint8_t *p = reinterpret_cast<uint8_t *>(req.sga.sga_buf);
    if (!my_tcp_flag) {
        p += sizeof(req.header);
    }
    req.sga.sga_numsegs = req.header.h_sgasegs;
    size_t len = 0;
    for (size_t i = 0; i < req.sga.sga_numsegs; ++i) {
        size_t seglen = *reinterpret_cast<uint32_t *>(p);
        req.sga.sga_segs[i].sgaseg_len = seglen;
        //printf("[%x] sga len= %ld\n", qd, req.sga.bufs[i].len);
        p += sizeof(uint32_t);
        req.sga.sga_segs[i].sgaseg_buf = p;
        p += seglen;
        len += seglen;
    }

    req.done = true;
    //fprintf(stderr, "[%x] message length=%ld\n", qd, req.res);
    return 0;
}

int
dmtr::posix_queue::process_outgoing(pending_request &req)
{
    // todo: need to encode in network byte order.

    auto * const sga = &req.sga;
    //printf("req.num_bytes = %lu req.header[1] = %lu", req.num_bytes, req.header[1]);
    // set up header
    //fprintf(stderr, "[%x] process_outgoing fd:%d num_bufs:%ld\n", qd, fd, sga.num_bufs);

    size_t iov_len = 2 * sga->sga_numsegs + 1;
    struct iovec iov[iov_len];
    size_t data_size = 0;
    size_t total_len = 0;

    // calculate size and fill in iov
    for (size_t i = 0; i < sga->sga_numsegs; i++) {
        auto j = 2 * i + 1;
        iov[j].iov_base = &sga->sga_segs[i].sgaseg_len;
        iov[j].iov_len = sizeof(sga->sga_segs[i].sgaseg_len);
        ++j;
        iov[j].iov_base = sga->sga_segs[i].sgaseg_buf;
        iov[j].iov_len = sga->sga_segs[i].sgaseg_len;

        // add up actual data size
        data_size += sga->sga_segs[i].sgaseg_len;

        // add up expected packet size (not yet including header)
        total_len += sga->sga_segs[i].sgaseg_len;
        total_len += sizeof(sga->sga_segs[i].sgaseg_len);
    }

    // fill in header
    dmtr_header_t header;
    header.h_magic = DMTR_HEADER_MAGIC;
    header.h_bytes = total_len;
    header.h_sgasegs = sga->sga_numsegs;

    // set up header at beginning of packet
    iov[0].iov_base = &header;
    iov[0].iov_len = sizeof(header);
    total_len += sizeof(header);

    size_t count = 0;
    int err = writev(count, my_fd, iov, iov_len);
    switch (err) {
        default:
            req.done = true;
            req.error = err;
            return 0;
        case EAGAIN:
            // we'll try again later.
            return 0;
        case 0:
            break;
    }

    if (count < total_len) {
        req.done = true;
        req.error = ENOTSUP;
        return 0;
    }

    if (count > total_len) {
        DMTR_UNREACHABLE();
    }

    // count == total_len
    //fprintf(stderr, "[%x] Sending message datasize=%ld totalsize=%ld\n", qd, data_size, total_len);
    req.done = true;
    req.num_bytes = count;
    return 0;
}

int
dmtr::posix_queue::process_work_queue(size_t limit)
{
    size_t done = 0;

    while (!my_work_queue.empty() && done < limit) {
        auto qt = my_work_queue.front();
        auto it = my_pending.find(qt);

        DMTR_TRUE(EPERM, it != my_pending.end());
        pending_request * const req = &it->second;
        if (req->push) {
            DMTR_OK(process_outgoing(*req));
        } else {
            DMTR_OK(process_incoming(*req));
        }

        if (req->done) {
            my_work_queue.pop();
        }

        ++done;
    }

    return 0;
}

int
dmtr::posix_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EINVAL, my_pending.find(qt) == my_pending.cend());
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    pending_request req;
    DMTR_ZEROMEM(req);
    req.push = true;
    req.sga = sga;
    my_pending.insert(std::make_pair(qt, req));
    my_work_queue.push(qt);
    return 0;
}

int
dmtr::posix_queue::pop(dmtr_qtoken_t qt)
{
    DMTR_TRUE(EINVAL, my_pending.find(qt) == my_pending.cend());
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    pending_request req;
    DMTR_ZEROMEM(req);
    my_pending.insert(std::make_pair(qt, req));
    my_work_queue.push(qt);
    return 0;
}

int
dmtr::posix_queue::peek(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    auto it = my_pending.find(qt);
    DMTR_TRUE(EINVAL, it != my_pending.cend());

    process_work_queue(1);

    if (it->second.done) {
        pending_request req = it->second;

        if (!req.push && req.error == 0) {
            DMTR_NOTNULL(sga_out);
            *sga_out = req.sga;
        }

        return req.error;
    }

    return EAGAIN;
}

int
dmtr::posix_queue::wait(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = poll(sga_out, qt);
    }

    return ret;
}

int
dmtr::posix_queue::poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    int ret = peek(sga_out, qt);
    switch (ret) {
        default:
            return ret;
        case 0:
            my_pending.erase(qt);
            return 0;
    }
}

int
dmtr::posix_queue::set_tcp_nodelay(int fd)
{
    const int n = 1;
    if (-1 == ::setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &n, sizeof(n))) {
        return errno;
    }

    return 0;
}

int
dmtr::posix_queue::set_non_blocking(int fd) {
    if (-1 == fcntl(fd, F_SETFL, O_NONBLOCK, 1)) {
        return errno;
    }

    return 0;
}

int dmtr::posix_queue::read(size_t &count_out, int fd, void *buf, size_t len) {
    count_out = 0;
    DMTR_NOTNULL(buf);
    DMTR_TRUE(ERANGE, len <= SSIZE_MAX);
    Latency_Start(&dev_read_latency);
    int ret = ::read(fd, buf, len);
    Latency_End(&dev_read_latency);
    if (ret < -1) {
        DMTR_UNREACHABLE();
    } else if (ret == -1) {
        return errno;
    } else {
        count_out = ret;
        return 0;
    }
}

int dmtr::posix_queue::recvfrom(size_t &count_out, int sockfd, void *buf, size_t len, int flags, void *saddr, socklen_t *addrlen) {
    count_out = 0;
    DMTR_TRUE(EINVAL, addrlen == NULL || sizeof(struct sockaddr) <= *addrlen);
    Latency_Start(&dev_read_latency);
    int ret = ::recvfrom(sockfd, buf, len, flags, reinterpret_cast<struct sockaddr *>(saddr), addrlen);
    Latency_End(&dev_read_latency);
    if (ret == -1) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return EAGAIN;
        }
        DMTR_OK(errno);
    } else if (ret == 0) {
        // peer did an "orderly shutdown".
        // note: i think this only applies to connection oriented
        // protocols, which isn't the case here.
        DMTR_OK(ENOTSUP);
    }

    count_out = ret;
    return 0;
}

int dmtr::posix_queue::writev(size_t &count_out, int fd, const struct iovec *iov, int iovcnt) {
    Latency_Start(&dev_write_latency);
    ssize_t ret = ::writev(fd, iov, iovcnt);
    Latency_End(&dev_write_latency);

    if (ret == -1) {
        if (errno == EWOULDBLOCK || errno == EAGAIN) {
            // we'll try again later.
            return EAGAIN;
        }

        DMTR_OK(errno);
        DMTR_UNREACHABLE();
    }

    if (ret < -1) {
        DMTR_UNREACHABLE();
    }

    count_out = ret;
    return 0;
}
