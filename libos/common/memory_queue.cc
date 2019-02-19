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
#include "memory_queue.hh"
#include <dmtr/annot.h>
#include <dmtr/mem.h>

dmtr::memory_queue::completion::completion() :
    my_sga{}
{}

dmtr::memory_queue::memory_queue(int qd) :
    io_queue(MEMORY_Q, qd)
{}

int dmtr::memory_queue::new_object(io_queue *&q_out, int qd) {
    q_out = new memory_queue(qd);
    return 0;
}

int
dmtr::memory_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    // we invariably allocate memory here, so it's safe to move the allocation
    // outside of the lock scope.
    auto *req = new completion();
    std::lock_guard<std::mutex> lock(my_lock);
    my_ready_queue.push(sga);
    // push always completes immediately.
    req->complete();
    my_completions.insert(std::make_pair(qt, req));
    return 0;
}

int
dmtr::memory_queue::pop(dmtr_qtoken_t qt) {
    // we invariably allocate memory here, so it's safe to move the allocation
    // outside of the lock scope.
    auto *req = new completion();
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_TRUE(EEXIST, my_completions.find(qt) == my_completions.cend());
    my_completions.insert(std::make_pair(qt, req));

    return 0;
}

int
dmtr::memory_queue::poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt) {
    DMTR_NOTNULL(EINVAL, sga_out);
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_TRUE(ENOENT, my_completions.find(qt) != my_completions.cend());
    auto it = my_completions.find(qt);
    auto * const req = it->second;
    // if we've opened the box beforehand, report the result we saw last.
    if (req->completed()) {
        *sga_out = it->second->sga();
        return 0;
    }

    // if there's something to dequeue, then we can complete the `pop()`
    // operation.
    if (!my_ready_queue.empty()) {
        req->sga(my_ready_queue.front());
        my_ready_queue.pop();
        *sga_out = it->second->sga();
        return 0;
    }

    // the ready queue is empty-- we have nothing to peek at.
    return EAGAIN;
}

int dmtr::memory_queue::drop(dmtr_qtoken_t qt)
{
    dmtr_sgarray_t sga = {};
    std::lock_guard<std::mutex> lock(my_lock);
    int ret = poll(&sga, qt);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            break;
    }

    // if the `peek()` succeeds, we forget the completion state.
    auto it = my_completions.find(qt);
    DMTR_TRUE(EPERM, it != my_completions.cend());
    delete it->second;
    my_completions.erase(it);
    return 0;
}

