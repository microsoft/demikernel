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

int dmtr::posix_queue::accept(io_queue *&q_out, struct sockaddr * const saddr, socklen_t * const addrlen, int new_qd)
{
    int ret = accept2(q_out, saddr, addrlen, new_qd);
    if (0 == ret) {
        return 0;
    }

    if (q_out != NULL) {
        delete q_out;
        q_out = NULL;
    }
    return ret;
}

int dmtr::posix_queue::accept2(io_queue *&q_out, struct sockaddr * const saddr, socklen_t * const addrlen, int new_qd)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(EPERM, my_listening_flag);

    auto * const q = new posix_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);

    int ret = ::accept4(my_fd, saddr, addrlen, SOCK_NONBLOCK);
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
    DMTR_TRUE(EINVAL, my_fd != -1);

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

int dmtr::posix_queue::connect(const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
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

int dmtr::posix_queue::close()
{
    DMTR_TRUE(EINVAL, my_fd != -1);

    int ret = ::close(my_fd);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            my_fd = -1;
            return 0;
    }
}

int dmtr::posix_queue::on_recv(task &t)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(EPERM, !my_listening_flag);

    //printf("on_recv qd:%d\n", qd);
    // if we don't have a full header in our buffer, then get one
    if (my_tcp_flag) {
        if (t.num_bytes < sizeof(t.header)) {
            // note: in TCP mode, we read the header directly into
            // `t.header`. in UDP mode, we read into `t.buf` and
            // then copy the information over to `t.header`.
            uint8_t *p = reinterpret_cast<uint8_t *>(&t.header) + t.num_bytes;
            size_t len = sizeof(t.header) - t.num_bytes;
            size_t count = 0;
            int err = read(count, my_fd, p, len);
            switch (err) {
                default:
                    t.done = true;
                    t.error = err;
                    return 0;
                case EAGAIN:
                    return 0;
                case 0:
                    t.num_bytes += count;
                    break;
            }
        }
    } else {
        DMTR_TRUE(EPERM, t.sga.sga_buf == NULL);
        DMTR_TRUE(EPERM, t.sga.sga_addr == NULL);

        t.sga.sga_addrlen = sizeof(struct sockaddr_in);
        DMTR_OK(dmtr_malloc(&t.sga.sga_buf, 1024));
        void *p = NULL;
        DMTR_OK(dmtr_malloc(&p, t.sga.sga_addrlen));
        t.sga.sga_addr = reinterpret_cast<struct sockaddr *>(p);

        size_t count;
        int err = recvfrom(count, my_fd, t.sga.sga_buf, 1024, 0, t.sga.sga_addr, &t.sga.sga_addrlen);
        switch (err) {
            default:
                t.done = true;
                t.error = err;
                return 0;
            case EAGAIN:
                return 0;
            case 0:
                break;
        }

        t.num_bytes = count;
        memcpy(&t.header, t.sga.sga_buf, sizeof(t.header));
    }

    if (t.num_bytes < sizeof(t.header)) {
        t.done = true;
        // if we haven't read any bytes, it's a sign that the connection
        // was dropped.
        t.error = t.num_bytes == 0 ? ECONNABORTED : EPROTO;
        return 0;
    }

    //fprintf(stderr, "[%x] on_recv: first read=%ld\n", qd, count);
    if (t.header.h_magic != DMTR_HEADER_MAGIC) {
        // not a correctly formed packet
        //fprintf(stderr, "Could not find magic %lx\n", t.header.h_magic);
        t.done = true;
        t.error = EILSEQ;
        return 0;
    }

    size_t data_len = t.header.h_bytes;
    if (my_tcp_flag) {
        // now we'll allocate a buffer
        if (t.sga.sga_buf == NULL) {
            DMTR_OK(dmtr_malloc(&t.sga.sga_buf, data_len));
        }

        // grab the rest of the packet
        if (t.num_bytes < sizeof(t.header) + data_len) {
            size_t offset = t.num_bytes - sizeof(t.header);
            uint8_t *p = reinterpret_cast<uint8_t *>(t.sga.sga_buf) + offset;
            size_t len = data_len - offset;
            size_t count = 0;
            int err = read(count, my_fd, p, len);
            //fprintf(stderr, "[%x] Next read size=%ld\n", qd, count);
            switch (err) {
                default:
                    t.done = true;
                    t.error = err;
                    return 0;
                case EAGAIN:
                    return 0;
                case 0:
                    t.done = (0 == count);
                    t.num_bytes += count;
                    break;
            }

            if (t.num_bytes < sizeof(t.header) + data_len) {
                if (t.done) {
                    t.error = EPROTO;
                }

                return 0;
            }
        }
        //fprintf(stderr, "[%x] data read length=%ld\n", qd, data_len);
    }

    // now we have the whole buffer, start filling sga
    uint8_t *p = reinterpret_cast<uint8_t *>(t.sga.sga_buf);
    if (!my_tcp_flag) {
        p += sizeof(t.header);
    }
    t.sga.sga_numsegs = t.header.h_sgasegs;
    size_t len = 0;
    for (size_t i = 0; i < t.sga.sga_numsegs; ++i) {
        size_t seglen = *reinterpret_cast<uint32_t *>(p);
        t.sga.sga_segs[i].sgaseg_len = seglen;
        //printf("[%x] sga len= %ld\n", qd, t.sga.bufs[i].len);
        p += sizeof(uint32_t);
        t.sga.sga_segs[i].sgaseg_buf = p;
        p += seglen;
        len += seglen;
    }

    t.done = true;
    t.error = 0;
    //fprintf(stderr, "[%x] message length=%ld\n", qd, t.res);
    return 0;
}

int dmtr::posix_queue::on_send(task &t)
{
    // todo: need to encode in network byte order.
    DMTR_TRUE(EINVAL, my_fd != -1);

    auto * const sga = &t.sga;
    //printf("t.num_bytes = %lu t.header[1] = %lu", t.num_bytes, t.header[1]);
    // set up header
    //fprintf(stderr, "[%x] on_send fd:%d num_bufs:%ld\n", qd, fd, sga.num_bufs);

    size_t iov_len = 2 * sga->sga_numsegs + 1;
    struct iovec iov[iov_len];
    size_t data_size = 0;
    size_t total_len = 0;

    // calculate size and fill in iov
    for (size_t i = 0; i < sga->sga_numsegs; i++) {
        const auto j = 2 * i + 1;
        iov[j].iov_base = &sga->sga_segs[i].sgaseg_len;
        iov[j].iov_len = sizeof(sga->sga_segs[i].sgaseg_len);

        const auto k = j + 1;
        iov[k].iov_base = sga->sga_segs[i].sgaseg_buf;
        iov[k].iov_len = sga->sga_segs[i].sgaseg_len;

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
            t.done = true;
            t.error = err;
            return 0;
        case EAGAIN:
            // we'll try again later.
            return 0;
        case 0:
            break;
    }

    if (count < total_len) {
        t.done = true;
        t.error = ENOTSUP;
        return 0;
    }

    if (count > total_len) {
        DMTR_UNREACHABLE();
    }

    // count == total_len
    //fprintf(stderr, "[%x] Sending message datasize=%ld totalsize=%ld\n", qd, data_size, total_len);
    t.done = true;
    t.num_bytes = count;
    return 0;
}

int dmtr::posix_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(EINVAL, my_tasks.find(qt) == my_tasks.cend());
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    task t = {};
    t.sga = sga;
    my_tasks.insert(std::make_pair(qt, t));
    return 0;
}

int dmtr::posix_queue::pop(dmtr_qtoken_t qt)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    DMTR_TRUE(EINVAL, my_tasks.find(qt) == my_tasks.cend());
    DMTR_TRUE(ENOTSUP, !my_listening_flag);

    task t = {};
    t.pull = true;
    my_tasks.insert(std::make_pair(qt, t));
    return 0;
}

int dmtr::posix_queue::peek(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    DMTR_TRUE(EINVAL, my_fd != -1);
    auto it = my_tasks.find(qt);
    DMTR_TRUE(EINVAL, it != my_tasks.cend());

    if (it->second.pull) {
        if (my_active_recv != boost::none && boost::get(my_active_recv) != qt) {
            return EAGAIN;
        }

        my_active_recv = qt;
        on_recv(it->second);
    } else {
        on_send(it->second);
    }

    if (it->second.done) {
        task t = it->second;

        if (t.pull && t.error == 0) {
            DMTR_NOTNULL(sga_out);
            *sga_out = t.sga;
        }

        return t.error;
    }

    return EAGAIN;
}

int dmtr::posix_queue::wait(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    DMTR_TRUE(EINVAL, my_fd != -1);

    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = poll(sga_out, qt);
    }

    return ret;
}

int dmtr::posix_queue::poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt)
{
    DMTR_TRUE(EINVAL, my_fd != -1);

    int ret = peek(sga_out, qt);
    switch (ret) {
        default:
            return ret;
        case 0:
            my_tasks.erase(qt);
            if (my_active_recv != boost::none && boost::get(my_active_recv) == qt) {
                my_active_recv = boost::none;
            }

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
    DMTR_TRUE(EINVAL, addrlen == NULL || sizeof(struct sockaddr) <= *addrlen);
    count_out = 0;

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
