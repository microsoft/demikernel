/*
 * mtcp_common.h
 *
 *  Created on: Aug 16, 2018
 *      Author: amanda
 */

#ifndef MTCP_COMMON_H_
#define MTCP_COMMON_H_

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

#include "../libos/common/latency.h"

#define MAX_SGARRAY_SIZE    10
#define BUFSIZE             10

#define PKTNUM              10000

#define MAGIC 0x10102010

DEFINE_LATENCY(pop_latency);
DEFINE_LATENCY(push_latency);
DEFINE_LATENCY(dev_read_latency);
DEFINE_LATENCY(dev_write_latency);

typedef void * ioptr;

struct sgelem {
    ioptr buf;
    size_t len;
    // for file operations
    uint64_t addr;
};

struct sgarray {
    int num_bufs;
    sgelem bufs[MAX_SGARRAY_SIZE];

    size_t copy(sgarray &sga) {
        size_t len = 0;
        num_bufs = sga.num_bufs;
        for (int i = 0; i < sga.num_bufs; i++) {
            bufs[i].len = sga.bufs[i].len;
            len += sga.bufs[i].len;
            bufs[i].buf = sga.bufs[i].buf;
        }
        return len;
    };

    struct sockaddr_in addr;
};

struct Pending {
    bool isDone;
    ssize_t res;
    // header = MAGIC, dataSize, SGA_num
    uint64_t header[3];
    // currently used incoming buffer
    void *buf;
    // number of bytes processed so far
    size_t num_bytes;
    struct sgarray &sga;

    Pending(struct sgarray &input_sga) :
        isDone(false),
        res(0),
        header{0,0,0},
        buf(NULL),
        num_bytes(0),
        sga(input_sga) { };
};

int mtcp_env_init();

int MTCP_socket(int domain, int type, int protocol);
int MTCP_bind(int qd, struct sockaddr *saddr, socklen_t size);
int MTCP_accept(int qd, struct sockaddr *saddr, socklen_t *size);
int MTCP_listen(int qd, int backlog);
int MTCP_connect(int qd, struct sockaddr *saddr, socklen_t size);
int MTCP_close(int qd);

ssize_t MTCP_recv(int qd, struct Pending* req);
ssize_t MTCP_send(int qd, struct Pending* req);

#endif /* MTCP_COMMON_H_ */
