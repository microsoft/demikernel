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
#include <libos/common/mem.h>

dmtr::memory_queue::memory_queue(int qd) :
    io_queue(MEMORY_Q, qd)
{}

int dmtr::memory_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = std::unique_ptr<io_queue>(new memory_queue(qd));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

int
dmtr::memory_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    // we invariably allocate memory here, so it's safe to move the allocation
    // outside of the lock scope.
    task *t = NULL;
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_OK(new_task(t, qt, DMTR_OPC_PUSH));
    my_ready_queue.push(sga);
    // push always completes immediately.
    t->done = true;
    return 0;
}

int
dmtr::memory_queue::pop(dmtr_qtoken_t qt) {
    task *t = NULL;
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_OK(new_task(t, qt, DMTR_OPC_POP));
    return 0;
}

int
dmtr::memory_queue::poll(dmtr_qresult_t * const qr_out, dmtr_qtoken_t qt) {
    if (qr_out != NULL) {
        *qr_out = {};
    }

    task *t = NULL;
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_OK(get_task(t, qt));

    // if we've opened the box beforehand, report the result we saw last.
    if (t->done) {
        return t->to_qresult(qr_out, qd());
    }

    if (DMTR_OPC_POP == t->opcode && !my_ready_queue.empty()) {
        // if there's something to dequeue, then we can complete a `pop()`
        // operation.
        t->sga = my_ready_queue.front();
        my_ready_queue.pop();
        t->done = true;
    }

    return t->to_qresult(qr_out, qd());
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
    DMTR_OK(drop_task(qt));
    return 0;
}

