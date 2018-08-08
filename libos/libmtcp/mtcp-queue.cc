// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/posix/posix-queue.cc
 *   POSIX implementation of Zeus queue interface
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
#include "mtcp_measure.h"
#include "mtcp-queue.h"
#include "common/library.h"
#include "include/measure.h"
// hoard include
#include "libzeus.h"
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

/*********************************************************************/
/* measurement variables */
#ifdef _LIBOS_MTCP_MTCP_READ_LTC_
    static uint64_t mtcp_read_total_ticks = 0;
    static uint64_t mtcp_read_success_ticks = 0;
    static int mtcp_read_counter = 0;
    #define READ_OUTPUT_INTERVAL 10
#endif

#ifdef _LIBOS_MTCP_MTCP_WRITE_LTC_
    static uint64_t mtcp_write_total_ticks = 0;
    static int mtcp_write_counter = 0;
    #define WRITE_OUTPUT_INTERVAL 10
#endif

/*static inline uint64_t rdtsc(void)
{
    uint64_t eax, edx;
    __asm volatile ("rdtsc" : "=a" (eax), "=d" (edx));
    return (edx << 32) | eax;
}*/

struct timer_info ti;


/*********************************************************************/
namespace Zeus {
namespace MTCP {

static mctx_t mctx = NULL;
static int mtcp_ep;

static char mtcp_conf_name[] = "libos.conf";
static bool mtcp_env_initialized = false;

//static uint64_t rcd_read_start;
//static uint64_t rcd_read_end;
//
//static uint64_t rcd_write_start;
//static uint64_t rcd_write_end;
//
//static inline uint64_t jl_rdtsc(void)
//{
//    uint64_t eax, edx;
//    __asm volatile ("rdtsc" : "=a" (eax), "=d" (edx));
//    return (edx << 32) | eax;
//}


//void libos_mtcp_signal_handler(int signum){
    // NIY
//}

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

/* add measurement code into these wrappers */
inline ssize_t
_wrapper_mtcp_read(mctx_t mctx, int sockid, char *buf, size_t len) {
#ifdef _LIBOS_MTCP_MTCP_READ_LTC_
    uint64_t read_start_tick, read_end_tick;
    read_start_tick = rdtsc(); 
#endif
    ssize_t ret = mtcp_read(mctx, sockid, buf, len);
#ifdef _LIBOS_MTCP_MTCP_READ_LTC_
    read_end_tick = rdtsc();
    mtcp_read_total_ticks += (read_end_tick - read_start_tick);
    if(ret >= 0){
        mtcp_read_counter++;
        mtcp_read_success_ticks += (read_end_tick - read_start_tick);
    }
    if(mtcp_read_counter == READ_OUTPUT_INTERVAL){
        fprintf(stderr, "mtcp_read_success_ticks:%lu mtcp_read_total_ticks:%lu\n",
                mtcp_read_success_ticks, mtcp_read_total_ticks);
        mtcp_read_counter = 0;
    }
#endif
    return ret;
}

inline int
_wrapper_mtcp_writev(mctx_t mctx, int sockid, const struct iovec *iov, int numIOV){
#ifdef _LIBOS_MTCP_MTCP_WRITE_LTC_
    uint64_t write_start_tick, write_end_tick;
    write_start_tick = rdtsc();
#endif
    int ret = mtcp_writev(mctx, sockid, iov, numIOV);
#ifdef _LIBOS_MTCP_MTCP_WRITE_LTC_
    write_end_tick = rdtsc();
    if(ret >= 0){
        mtcp_write_counter++;
        mtcp_write_total_ticks += (write_end_tick - write_start_tick);
        if(mtcp_write_counter == WRITE_OUTPUT_INTERVAL){
            fprintf(stderr, "mtcp_write_total_ticks:%lu\n", mtcp_write_total_ticks);
        }
    }
#endif
    return ret;
}

int
MTCPQueue::socket(int domain, int type, int protocol)
{
#ifdef _LIBOS_MTCP_DEBUG_
    printf("mtcp_queue\n");
#endif
    if (!mtcp_env_initialized) {
        printf("mtcp-queue.c/queue() will init mtcp env\n");
        mtcp_env_init();
    }
    mtcp_qd = mtcp_socket(mctx, domain, type, protocol);
    // check qd?, not let the application check qd
    int n = 1;
    mtcp_setsockopt(mctx, mtcp_qd, IPPROTO_TCP, TCP_NODELAY, (char *)&n, sizeof(n));
    return qd;
}

int
MTCPQueue::bind(struct sockaddr *saddr, socklen_t size)
{
    int ret = mtcp_bind(mctx, mtcp_qd, saddr, sizeof(struct sockaddr_in));
    printf("bind ret: %d\n", ret);
    if (ret == 0){
        return ret;
    }
    return -1;
}

int
MTCPQueue::accept(struct sockaddr *saddr, socklen_t *size)
{
    assert(listening);

    struct mtcp_epoll_event ev;

    if (accepts.empty()) {
        return -1;
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
MTCPQueue::listen(int backlog)
{
    int res = mtcp_listen(mctx, mtcp_qd, backlog);
    if (res == 0) {
        listening = true;
        int ret = mtcp_setsock_nonblock(mctx, mtcp_qd);
        if(ret < 0) {
            fprintf(stderr, "error set listen socket\n");
        }
        return res;
    } else {
        fprintf(stderr, "error listen %d\n", mtcp_qd);
        return -errno;
    }
}


int
MTCPQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    struct mtcp_epoll_event ev;
    int res = mtcp_connect(mctx, mtcp_qd, saddr, size);
    fprintf(stderr, "connect() res = %i errno= %s\n", res, strerror(errno));
    if (res == 0) {
        // Always put it in non-blocking mode
        int ret = mtcp_setsock_nonblock(mctx, mtcp_qd);
        if(ret < 0){
            fprintf(stderr, "connect() cannot set the socket to nonblock\n");
        }
        ev.events = MTCP_EPOLLOUT;
        mtcp_evts |= ev.events;
        ev.data.sockid = mtcp_qd;
        mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_ADD, mtcp_qd, &ev);
        return res;
    } else {
        return errno;
    }
}

int
MTCPQueue::open(const char *pathname, int flags)
{
    // use the fd as qd
    return ::open(pathname, flags);
}

int
MTCPQueue::open(const char *pathname, int flags, mode_t mode)
{
    // use the fd as qd
    return ::open(pathname, flags, mode);
}

int
MTCPQueue::creat(const char *pathname, mode_t mode)
{
    // use the fd as qd
    return ::creat(pathname, mode);
}
    
int
MTCPQueue::close()
{
    mtcp_evts = 0;
    mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_DEL, mtcp_qd, NULL);
    return mtcp_close(mctx, mtcp_qd);
}

int
MTCPQueue::getfd()
{
    return mtcp_qd;
}

void
MTCPQueue::setfd(int fd)
{
    this->mtcp_qd = fd;
}

void
MTCPQueue::ProcessIncoming(PendingRequest &req)
{
    if (listening) {
        struct sockaddr_in saddr;
        socklen_t size = sizeof(saddr);
        int newfd = mtcp_accept(mctx, mtcp_qd, (struct sockaddr*)&saddr, &size);
        if (newfd == -1) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return;
            } else {
                req.isDone = true;
                req.res = -1;
            }
        } else {
            req.isDone = true;
            req.res = newfd;
            accepts.push_back(std::make_pair(newfd, saddr));
        }
    }
    ti.libos_pop_start = rdtsc();
    // printf("ProcessIncoming\n");
    // if we don't have a full header in our buffer, then get one
    if (req.num_bytes < sizeof(req.header)) {
        ti.device_read_start = rdtsc();
        ssize_t count = _wrapper_mtcp_read(mctx, mtcp_qd, (char*)((uint8_t *)&req.header + req.num_bytes), sizeof(req.header) - req.num_bytes);
        ti.device_read_end = rdtsc();
        //ssize_t count = mtcp_read(mctx, qd, (char*)((uint8_t *)&req.buf + req.num_bytes), sizeof(req.header) - req.num_bytes);
        // printf("0-mtcp_read() count:%d\n", count);
        // we still don't have a header
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return;
            } else {
                fprintf(stderr, "Could not read header: %s\n", strerror(errno));
                req.isDone = true;
                req.res = count;
                return;
            }
        }
        req.num_bytes += count;
        if (req.num_bytes < sizeof(req.header)) {
            return;
        }
    }

    if (req.header[0] != MAGIC) {
        // not a correctly formed packet
        fprintf(stderr, "Could not find magic %llx\n", req.header[0]);
        req.isDone = true;
        req.res = -1;
        return;
    }
    size_t dataLen = req.header[1];
    // now we'll allocate a buffer
    if (req.buf == NULL) {
        req.buf = malloc(dataLen);
    }

    size_t offset = req.num_bytes - sizeof(req.header);
    // grab the rest of the packet
    if (req.num_bytes < sizeof(req.header) + dataLen) {
        ssize_t count = _wrapper_mtcp_read(mctx, mtcp_qd, (char*)((int8_t *)req.buf + offset), dataLen - offset);
#ifdef _LIBOS_MTCP_DEBUG_
        printf("1-mtcp_read() count:%d\n", count);
#endif
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return;
            } else {
                fprintf(stderr, "Could not read data: %s\n", strerror(errno));
                req.isDone = true;
                req.res = count;
                return;
            }
        }
        req.num_bytes += count;
        if (req.num_bytes < sizeof(req.header) + dataLen) {
            return;
        }
    }
    
    // now we have the whole buffer, start filling sga
    uint8_t *ptr = (uint8_t *)req.buf;
    // printf("req.buf:%p\n", req.buf);
    req.sga.num_bufs = req.header[2];
#ifdef _LIBOS_MTCP_DEBUG_
    printf("num_bufs:%d\n", req.header[2]);
    printf("sga addr:%p\n", (void*) &req.sga);
#endif
    for (int i = 0; i < req.sga.num_bufs; i++) {
        req.sga.bufs[i].len = *(size_t *)ptr;
#ifdef _LIBOS_MTCP_DEBUG_
        printf("req.sga.bufs[%d].len:%d\n", i, req.sga.bufs[i].len);
#endif
        ptr += sizeof(uint64_t);
        req.sga.bufs[i].buf = (ioptr)ptr;
        char read_str[100];
        memcpy(read_str, req.sga.bufs[i].buf, req.sga.bufs[i].len);
        read_str[req.sga.bufs[i].len] = '\0';
#ifdef _LIBOS_MTCP_DEBUG_
        printf("read str:%s , first character:%c\n", read_str, read_str[0]);
#endif
        ptr += req.sga.bufs[i].len;
    }
    req.isDone = true;
    req.res = dataLen - (req.sga.num_bufs * sizeof(uint64_t));
#ifdef _LIBOS_MTCP_DEBUG_
    printf("End ProcessIncoming\n");
#endif
    ti.libos_pop_end = rdtsc();
    return;
}
    
void
MTCPQueue::ProcessOutgoing(PendingRequest &req)
{
    ti.libos_push_start = rdtsc();
    //uint64_t rcd_tick = jl_rdtsc();
    sgarray &sga = req.sga;
#ifdef _LIBOS_MTCP_DEBUG_
    printf("DEBUG:ProcessOutgoing:req.num_bytes = %lu req.header[1] = %lu\n", req.num_bytes, req.header[1]);
#endif
    struct iovec vsga[2*sga.num_bufs + 1];
    uint64_t lens[sga.num_bufs];
    size_t dataSize = 0;
    size_t totalLen = 0;

    // calculate size and fill in iov
    for (int i = 0; i < sga.num_bufs; i++) {
        lens[i] = sga.bufs[i].len;
        vsga[2*i+1].iov_base = &lens[i];
        vsga[2*i+1].iov_len = sizeof(uint64_t);
        
        vsga[2*i+2].iov_base = (void *)sga.bufs[i].buf;
        vsga[2*i+2].iov_len = sga.bufs[i].len;
        
        // add up actual data size
        dataSize += (uint64_t)sga.bufs[i].len;
        
        // add up expected packet size minus header
        totalLen += (uint64_t)sga.bufs[i].len;
        totalLen += sizeof(uint64_t);
        pin((void *)sga.bufs[i].buf);
    }

    // fill in header
    req.header[0] = MAGIC;
    req.header[1] = totalLen;
    req.header[2] = req.sga.num_bufs;

    // set up header at beginning of packet
    vsga[0].iov_base = &req.header;
    vsga[0].iov_len = sizeof(req.header);
    totalLen += sizeof(req.header);
   
    ti.device_send_start = rdtsc();
    ssize_t count = _wrapper_mtcp_writev(mctx, mtcp_qd,  vsga, 2*sga.num_bufs +1);
    ti.device_send_end = rdtsc();
    // if error
    if (count < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return;
        } else {
            fprintf(stderr, "Could not write packet: %s\n", strerror(errno));
            req.isDone = true;
            req.res = count;
            return;
        }
    }

    // otherwise
    req.num_bytes += count;
    if (req.num_bytes < totalLen) {
        assert(req.num_bytes == 0);
        return;
    }
    for (int i = 0; i < sga.num_bufs; i++) {
        unpin((void *)sga.bufs[i].buf);
    }

    req.res = dataSize;
    req.isDone = true;
    //uint64_t rcd_tick_end = jl_rdtsc();
#ifdef _LIBOS_MTCP_TOTAL_SERVER_LTC_
    fprintf(stderr, "ProcessOutgoing(writev) start:%lu end:%lu\n", rcd_tick, rcd_tick_end);
#endif
    ti.libos_push_end = rdtsc();
}
    
void
MTCPQueue::ProcessQ(size_t maxRequests)
{
    size_t done = 0;
#ifdef _LIBOS_MTCP_DEBUG_
//    printf("ProcessQ()\n");
#endif
    while (!workQ.empty() && done < maxRequests) {
        qtoken qt = workQ.front();
        auto it = pending.find(qt);
        done++;
        if (it == pending.end()) {
            workQ.pop_front();
            continue;
        }
        
        PendingRequest &req = it->second; 
        if (IS_PUSH(qt)) {
#ifdef _LIBOS_MTCP_DEBUG_
//            printf("isPush\n");
#endif
            ProcessOutgoing(req);
        } else {
#ifdef _LIBOS_MTCP_DEBUG_
//            printf("isPop\n");
#endif
            ProcessIncoming(req);
        }

        if (req.isDone) {
            workQ.pop_front();
        }            
    }
    //printf("Processed %lu requests", done);
}
    
ssize_t
MTCPQueue::Enqueue(qtoken qt, sgarray &sga)
{
 	auto it = pending.find(qt);
    if (it == pending.end()) {
        pending.insert(std::make_pair(qt, PendingRequest(sga)));
        workQ.push_back(qt);

        // let's try processing here because we know our sockets are
        // non-blocking
        if (workQ.front() == qt) {
            ProcessQ(1);
        }
    }
    PendingRequest &req = pending.find(qt)->second;

    if (req.isDone) {
        assert(sga.num_bufs > 0);
        return req.res;
    } else {
        //printf("Enqueue() req.is Done = false will return 0\n");
        return 0;
    } 
}

ssize_t
MTCPQueue::push(qtoken qt, struct sgarray &sga)
{
    struct mtcp_epoll_event ev;
    ev.events = MTCP_EPOLLOUT | mtcp_evts;
    ev.data.sockid = mtcp_qd;
    mtcp_evts = ev.events;
    mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_MOD, mtcp_qd, &ev);
#ifdef _LIBOS_MTCP_DEBUG_
    printf("MTCPQueue::push() qt:%d\n", qt);
#endif
    return Enqueue(qt, sga);
}
    
ssize_t
MTCPQueue::pop(qtoken qt, struct sgarray &sga)
{
    struct mtcp_epoll_event ev;
    ssize_t ret;
    ev.events = MTCP_EPOLLIN | mtcp_evts;
    ev.data.sockid = mtcp_qd;
    mtcp_evts = ev.events;
    mtcp_epoll_ctl(mctx, mtcp_ep, MTCP_EPOLL_CTL_MOD, mtcp_qd, &ev);
#ifdef _LIBOS_MTCP_DEBUG_
    printf("MTCPQueue::pop() qt:%d\n", qt);
#endif
    ret = Enqueue(qt, sga);
    if (ret != 0){
        // read success, feed the result sgarray
        auto it = pending.find(qt);
        assert(it != pending.end());
        sga.num_bufs = (it->second).sga.num_bufs;
        for(int i = 0; i < sga.num_bufs; i++){
            sga.bufs[i].buf = (it->second).sga.bufs[i].buf;
            sga.bufs[i].len = (it->second).sga.bufs[i].len;
        }
    }
    return ret;
}

ssize_t
MTCPQueue::peek(struct sgarray &sga){
    //uint64_t rcd_tick;
    //rcd_tick = jl_rdtsc();
    PendingRequest req = PendingRequest(sga);
    ProcessIncoming(req);
    if (req.isDone){
#ifdef _LIBOS_MTCP_TOTAL_SERVER_LTC_
        if(req.res > 0){
            fprintf(stderr, "light_pop  success size:%d time_before_read:%lu\n", req.res, rcd_tick);
        }
#endif
        return req.res;
    }else{
        return -1;
    }
}
    
ssize_t
MTCPQueue::wait(qtoken qt, struct sgarray &sga)
{
	ssize_t ret;
    auto it = pending.find(qt);
    assert(it != pending.end());
#ifdef _LIBOS_MTCP_DEBUG_
    printf("MTCPQueue::wait(%d) isDone:%d\n", qt, it->second.isDone);
#endif

    while(!it->second.isDone) {
        ProcessQ(1);
    }
#ifdef _LIBOS_MTCP_DEBUG_
    printf("wait() return\n");
#endif

    //sga = it->second.sga;
    // NOTE: not assign sga here, since I never wait for pop
	ret = it->second.res;
    return ret;
}

ssize_t
MTCPQueue::poll(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    if (it->second.isDone) {
        sga = it->second.sga;
        return it->second.res;
    } else {
        return 0;
    }
}


} // namespace MTCP    
} // namespace Zeus
