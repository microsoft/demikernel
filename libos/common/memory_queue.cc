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

dmtr::memory_queue::task::task(bool pull) :
    my_pull_flag(pull),
    my_sga{}
{}

int dmtr::memory_queue::task::to_qresult(dmtr_qresult * const qr_out) const {
    if (qr_out != NULL) {
        qr_out->qr_tid = DMTR_QR_NIL;
    }

    if (!completed()) {
        return EAGAIN;
    }

    if (my_pull_flag) {
        DMTR_NOTNULL(EINVAL, qr_out);
        qr_out->qr_tid = DMTR_QR_SGA;
        qr_out->qr_value.sga = this->sga();
    }

    return 0;
}

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
    auto * const t = new task(false);
    std::lock_guard<std::mutex> lock(my_lock);
    my_ready_queue.push(sga);
    // push always completes immediately.
    t->complete();
    my_tasks.insert(std::make_pair(qt, t));
    return 0;
}

int
dmtr::memory_queue::pop(dmtr_qtoken_t qt) {
    // we invariably allocate memory here, so it's safe to move the allocation
    // outside of the lock scope.
    auto * const t = new task(true);
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_TRUE(EEXIST, my_tasks.find(qt) == my_tasks.cend());
    my_tasks.insert(std::make_pair(qt, t));

    return 0;
}

int
dmtr::memory_queue::poll(dmtr_qresult_t * const qr_out, dmtr_qtoken_t qt) {
    if (qr_out != NULL) {
        qr_out->qr_tid = DMTR_QR_NIL;
    }

    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_TRUE(ENOENT, my_tasks.find(qt) != my_tasks.cend());

    auto it = my_tasks.find(qt);
    auto * const t = it->second;
    // if we've opened the box beforehand, report the result we saw last.
    if (t->completed()) {
        return t->to_qresult(qr_out);
    }

    if (t->is_pull() && !my_ready_queue.empty()) {
        // if there's something to dequeue, then we can complete a `pop()`
        // operation.
        t->sga(my_ready_queue.front());
        my_ready_queue.pop();
        t->complete();
    }

    return t->to_qresult(qr_out);
}

int dmtr::memory_queue::drop(dmtr_qtoken_t qt)
{
    dmtr_qresult_t qr = {};
    std::lock_guard<std::mutex> lock(my_lock);
    int ret = poll(&qr, qt);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            break;
    }

    // if the `poll()` succeeds, we forget the task state.
    auto it = my_tasks.find(qt);
    DMTR_TRUE(EPERM, it != my_tasks.cend());
    delete it->second;
    my_tasks.erase(it);
    return 0;
}

