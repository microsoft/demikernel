// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/posix/posix-queue.h
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

#include "posix-queue.h"
#include <unistd.h>

namespace Zeus {

using namespace POSIX;
LibIOQueue libqueue;

int queue(int domain, int type, int protocol)
{
    return socket(domain, type, protocol);
}

int bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    return bind(qd, saddr, size);
}

int
accept(int qd, struct sockaddr *saddr, socklen_t *size)
{
    return ::accept(qd, saddr, size);
}

int
listen(int qd, int backlog)
{
    return listen(qd, backlog);
}
        

int
connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    return connect(qd, saddr, size);
}

int
qd2fd(int qd) {
    return qd;
}

int
push(int qd, struct Zeus::sgarray &bufs)
{
    size_t total = 0;

    // qd is same as fd
    for (int i = 0; i < bufs.num_bufs; i++) {
        size_t count = write(qd, bufs.bufs[i].buf,
                             bufs.bufs[i].len);
        if (count < bufs.bufs[i].len) {
            return errno;
        }
        total += count;
    }
    return total;
}

int
pop(int qd, struct Zeus::sgarray &bufs)
{
    int num_bufs = 0;
    size_t count, total = 0;
    do {
        ioptr buf = malloc(BUFFER_SIZE);
        count = read(qd, buf, BUFFER_SIZE);
        if (count < 0) {
            return errno;
        }
        bufs.bufs[num_bufs].buf = buf;
        bufs.bufs[num_bufs].len = count;
        total += count;
    } while (count == BUFFER_SIZE &&
             num_bufs < MAX_SGARRAY_SIZE);
    return total;
}

} // namespace Zeus
