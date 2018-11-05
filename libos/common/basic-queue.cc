// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/basic-queue.cc
 *   Basic queue implementation
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
#include "basic-queue.h"
#include <assert.h>

namespace Zeus {

ssize_t
BasicQueue::push(qtoken qt, struct sgarray &sga)
{
    size_t len = 0;
    std::lock_guard<std::mutex> lock(qLock);
    if (!waiting.empty()) {
        qtoken pop = waiting.front();
        waiting.pop_front();
        auto it = pending.find(pop);
        PendingRequest *req = it->second;
        len = req->sga.copy(sga);
	assert(len > 0);
        req->cv.notify_all();
        req->isDone = true;
    } else {
	for (int i = 0; i < sga.num_bufs; i++) {
	    len += sga.bufs[i].len;
	}
        buffer.push_back(sga);
    }
    //fprintf(stderr, "[%x] pushing %lx size(%ld)", qd, qt, len);
    return len;
}

ssize_t
BasicQueue::flush_push(qtoken qt, struct sgarray &sga)
{
    return 0;
}
ssize_t
BasicQueue::pop(qtoken qt, struct sgarray &sga) {
    size_t len = 0;
    std::lock_guard<std::mutex> lock(qLock);
    // if we have a buffered push already
    if (!buffer.empty()) {
        len = sga.copy(buffer.front());
        buffer.pop_front();
    } else {
        pending.insert(
            std::make_pair(qt, new PendingRequest(sga)));
        waiting.push_back(qt);
        assert(pending.find(qt) != pending.end());
    }
    return len;
}

ssize_t
BasicQueue::peek(struct sgarray &sga) {
    size_t len = 0;
    std::lock_guard<std::mutex> lock(qLock);
    // if we have a buffered push already
    if (!buffer.empty()) {
        len = sga.copy(buffer.front());
        buffer.pop_front();
    }
    return len;
}

ssize_t
BasicQueue::wait(qtoken qt, struct sgarray &sga)
{
    std::unique_lock<std::mutex> lock(qLock);
    lock.lock();
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest *req = it->second;
    while (!req->isDone) {
        req->cv.wait(lock);
    }
    size_t len = sga.copy(req->sga);
    delete it->second;
    lock.unlock();
    delete req;
    return len;
}

ssize_t
BasicQueue::poll(qtoken qt, struct sgarray &sga)
{
    size_t len = 0;
    std::lock_guard<std::mutex> lock(qLock);
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest *req = it->second;

    if (req->isDone) {
        len = sga.copy(req->sga);
        pending.erase(it);
        delete req;
	assert(len > 0);
        return len;
    } else {
        return 0;
    }

}

int
BasicQueue::flush(qtoken qt, int flags)
{
    return 0;
}

}// namespace Zeus
