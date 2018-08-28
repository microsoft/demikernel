/*
 * mtcp_common.cpp
 *
 *  Created on: Aug 16, 2018
 *      Author: amanda
 */
#include <list>
#include <unordered_map>
#include <mtcp_api.h>
#include <mtcp_epoll.h>
#include <fcntl.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/tcp.h>
#include <assert.h>
#include <string.h>
#include <errno.h>
#include <sys/time.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <netinet/in.h>

#include "mtcp_common.h"
#include "../libos/common/latency.h"

#define MTCP_MAX_FLOW_NUM  (10000)
#define MTCP_MAX_EVENTS (MTCP_MAX_FLOW_NUM * 3)
#define MTCP_RCVBUF_SIZE (2*1024)
#define MTCP_SNDBUF_SIZE (8*1024)

uint32_t mtcp_evts;

static mctx_t mctx = NULL;
static int mtcp_ep;

static char mtcp_conf_name[] = "libos.conf";
static bool mtcp_env_initialized = false;

bool listening;
std::list<std::pair<int, struct sockaddr_in>> accepts;

int mtcp_env_init(){
    /* init mtcp environment */
    // NOTE: JL: finally, this init will be called by each thread
    int ret;
    struct mtcp_conf mcfg;
    int core_limit = 1;  // NOTE: JL: no consider of multi-core now
    int core = 0;

    assert(!mtcp_env_initialized);

    mtcp_getconf(&mcfg);
    mcfg.num_cores = core_limit;
    mtcp_setconf(&mcfg);
    // set core limit must be put before mtcp_init()
    ret = mtcp_init(mtcp_conf_name);
    ////mtcp_register_signal(SIGINT, libos_mtcp_signal_handler);
    mtcp_core_affinitize(core);
    mctx = mtcp_create_context(core);
    // init epoll for mtcp
    mtcp_ep = mtcp_epoll_create(mctx, MTCP_MAX_EVENTS);
    if (mtcp_ep < 0) {
        mtcp_destroy_context(mctx);
        return -1;
    }
    // only init mtcp once for each thread (single-thread now)
    mtcp_env_initialized = true;
    return ret;
}

int
MTCP_socket(int domain, int type, int protocol)
{
    if (!mtcp_env_initialized) {
        printf("mtcp-queue.c/queue() will init mtcp env\n");
        mtcp_env_init();
    }
    int mtcp_qd = mtcp_socket(mctx, domain, type, protocol);
    // check qd?, not let the application check qd
    int n = 1;
    mtcp_setsockopt(mctx, mtcp_qd, IPPROTO_TCP, TCP_NODELAY, (char *)&n, sizeof(n));
    return mtcp_qd;
}

int
MTCP_bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    int ret = mtcp_bind(mctx, qd, saddr, sizeof(struct sockaddr_in));
    printf("bind ret: %d\n", ret);
    if (ret == 0){
        return ret;
    }
    return -1;
}

int
MTCP_accept(int qd, struct sockaddr *saddr, socklen_t *size)
{
    assert(listening);

    struct mtcp_epoll_event ev;

    struct sockaddr_in _saddr;
    socklen_t _size = sizeof(saddr);
    int newfd = mtcp_accept(mctx, qd, (struct sockaddr*)&_saddr, &_size);
    if (newfd == -1) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return 0;
        } else {
            return -1;
        }
    } else {
        accepts.push_back(std::make_pair(newfd, _saddr));
    }

    if (accepts.empty()) {
        return 0;
    }

    auto &acceptInfo = accepts.front();
    int newqd = acceptInfo.first;
    struct sockaddr &addr = (struct sockaddr &)acceptInfo.second;
    *saddr = addr;
    *size = sizeof(sockaddr_in);

    if (newqd != -1) {
        int n = 1;
        mtcp_setsockopt(mctx, newqd, IPPROTO_TCP, TCP_NODELAY, (char *)&n, sizeof(n));
        // Always put it in non-blocking mode
        int ret = mtcp_setsock_nonblock(mctx, newqd);
        if (ret < 0) {
            fprintf(stderr, "error accept() cannot set nonblock\n");
        }
        // prepare for read msg
        ev.events = MTCP_EPOLLIN;
        ev.data.sockid = newqd;
        mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_ADD, newqd, &ev);
    }
    accepts.pop_front();
    return newqd;
}

int
MTCP_listen(int qd, int backlog)
{
    int res = mtcp_listen(mctx, qd, backlog);
    if (res == 0) {
        listening = true;
        int ret = mtcp_setsock_nonblock(mctx, qd);
        if(ret < 0) {
            fprintf(stderr, "error set listen socket\n");
        }
        return res;
    } else {
        fprintf(stderr, "error listen %d\n", qd);
        return -errno;
    }
}

int
MTCP_connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    struct mtcp_epoll_event ev;
    int res = mtcp_connect(mctx, qd, saddr, size);
    fprintf(stderr, "connect() res = %i errno= %s\n", res, strerror(errno));
    if (res == 0) {
        // Always put it in non-blocking mode
        int ret = mtcp_setsock_nonblock(mctx, qd);
        if(ret < 0){
            fprintf(stderr, "connect() cannot set the socket to nonblock\n");
        }
        ev.events = MTCP_EPOLLOUT;
        mtcp_evts |= ev.events;
        ev.data.sockid = qd;
        mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_ADD, qd, &ev);
        return res;
    } else {
        return -errno;
    }
}

int
MTCP_close(int qd)
{
    mtcp_evts = 0;
    mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_DEL, qd, NULL);
    return mtcp_close(mctx, qd);
}

ssize_t
MTCP_recv(int qd, struct Pending* req)
{
    Latency_Start(&pop_latency);
    // printf("ProcessIncoming\n");
    // if we don't have a full header in our buffer, then get one
    if (req->num_bytes < sizeof(req->header)) {
        ssize_t count = mtcp_read(mctx, qd, (char*)((uint8_t *)&req->header + req->num_bytes), sizeof(req->header) - req->num_bytes);
        //ssize_t count = mtcp_read(mctx, qd, (char*)((uint8_t *)&req.buf + req.num_bytes), sizeof(req.header) - req.num_bytes);
        // printf("0-mtcp_read() count:%d\n", count);
        // we still don't have a header
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return 0;
            } else {
                fprintf(stderr, "Could not read header: %s\n", strerror(errno));
                return count;
            }
        }
        req->num_bytes += count;
        if (req->num_bytes < sizeof(req->header)) {
            return 0;
        }
    }

    if (req->header[0] != MAGIC) {
        // not a correctly formed packet
        fprintf(stderr, "Could not find magic %llx\n", req->header[0]);
        return -1;
    }
    size_t dataLen = req->header[1];
    // now we'll allocate a buffer
    if (req->buf == NULL) {
        req->buf = malloc(dataLen);
    }

    size_t offset = req->num_bytes - sizeof(req->header);
    // grab the rest of the packet
    if (req->num_bytes < sizeof(req->header) + dataLen) {
        Latency_Start(&dev_read_latency);
        ssize_t count = mtcp_read(mctx, qd, (char*)((int8_t *)req->buf + offset), dataLen - offset);
        Latency_End(&dev_read_latency);
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return 0;
            } else {
                fprintf(stderr, "Could not read data: %s\n", strerror(errno));
                return -1;
            }
        }
        req->num_bytes += count;
        if (req->num_bytes < sizeof(req->header) + dataLen) {
            return 0;
        }
    }

    // now we have the whole buffer, start filling sga
    uint8_t *ptr = (uint8_t *)req->buf;
    // printf("req.buf:%p\n", req.buf);
    req->sga.num_bufs = req->header[2];
    for (int i = 0; i < req->sga.num_bufs; i++) {
        req->sga.bufs[i].len = *(size_t *)ptr;
        ptr += sizeof(uint64_t);
        req->sga.bufs[i].buf = (ioptr)ptr;
        ptr += req->sga.bufs[i].len;
    }

    Latency_End(&pop_latency);
    return dataLen - (req->sga.num_bufs * sizeof(uint64_t));
}

ssize_t
MTCP_send(int qd, struct Pending* req)
{
    Latency_Start(&push_latency);
    struct iovec vsga[2*req->sga.num_bufs + 1];
    uint64_t lens[req->sga.num_bufs];
    size_t dataSize = 0;
    size_t totalLen = 0;

    // calculate size and fill in iov
    for (int i = 0; i < req->sga.num_bufs; i++) {
        lens[i] = req->sga.bufs[i].len;
        vsga[2*i+1].iov_base = &lens[i];
        vsga[2*i+1].iov_len = sizeof(uint64_t);

        vsga[2*i+2].iov_base = (void *)req->sga.bufs[i].buf;
        vsga[2*i+2].iov_len = req->sga.bufs[i].len;

        // add up actual data size
        dataSize += (uint64_t)req->sga.bufs[i].len;

        // add up expected packet size minus header
        totalLen += (uint64_t)req->sga.bufs[i].len;
        totalLen += sizeof(uint64_t);
    }

    // fill in header
    req->header[0] = MAGIC;
    req->header[1] = totalLen;
    req->header[2] = req->sga.num_bufs;

    // set up header at beginning of packet
    vsga[0].iov_base = &req->header;
    vsga[0].iov_len = sizeof(req->header);
    totalLen += sizeof(req->header);

    Latency_Start(&dev_write_latency);
    ssize_t count = mtcp_writev(mctx, qd,  vsga, 2*req->sga.num_bufs +1);
    Latency_End(&dev_write_latency);
    // if error
    if (count < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return 0;
        } else {
            fprintf(stderr, "Could not write packet: %s\n", strerror(errno));
            return count;
        }
    }

    // otherwise
    req->num_bytes += count;
    if (req->num_bytes < totalLen) {
        assert(req->num_bytes == 0);
        return 0;
    }
    Latency_End(&push_latency);
    return dataSize;
}
