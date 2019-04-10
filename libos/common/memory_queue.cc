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
#include <iostream>
#include <libos/common/mem.h>

dmtr::memory_queue::memory_queue(int qd) :
    io_queue(MEMORY_Q, qd),
    my_good_flag(true)
{}

int dmtr::memory_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    auto * const q = new memory_queue(qd);
    DMTR_NOTNULL(ENOMEM, q);
    q_out = std::unique_ptr<io_queue>(q);
    q->start_threads();
    return 0;
}

int
dmtr::memory_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EINVAL, good());
    DMTR_NOTNULL(EINVAL, my_push_thread);

    std::lock_guard<std::recursive_mutex> lock(my_lock);
    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, sga));
    my_push_thread->enqueue(qt);
    return 0;
}

int dmtr::memory_queue::push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        const dmtr_sgarray_t *sga = NULL;
        DMTR_TRUE(EINVAL, t->arg(sga));

        std::cout << "Pushing to queue ..." << std::endl;
        my_ready_queue.push(*sga);
        t->complete(0, *sga);
    }

    return 0;
}

int dmtr::memory_queue::pop(dmtr_qtoken_t qt) {
    std::lock_guard<std::recursive_mutex> lock(my_lock);
    DMTR_TRUE(EINVAL, good());
    DMTR_NOTNULL(EINVAL, my_pop_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_POP));
    my_pop_thread->enqueue(qt);
    return 0;
}

int dmtr::memory_queue::pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        while (my_ready_queue.empty()) {
            yield();
        }

        auto sga = my_ready_queue.front();
        my_ready_queue.pop();
        t->complete(0, sga);
    }

    return 0;
}

int dmtr::memory_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt) {
    std::lock_guard<std::recursive_mutex> lock(my_lock);
    DMTR_TRUE(EINVAL, good());

    task *t;
    DMTR_OK(get_task(t, qt));

    int ret;
    switch (t->opcode()) {
        default:
            return ENOTSUP;
        case DMTR_OPC_PUSH:
            ret = my_push_thread->service();
            break;
        case DMTR_OPC_POP:
            ret = my_pop_thread->service();
            break;
    }

    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            break;
    }

    return io_queue::poll(qr_out, qt);
}

int dmtr::memory_queue::drop(dmtr_qtoken_t qt) {
    std::lock_guard<std::recursive_mutex> lock(my_lock);
    DMTR_TRUE(EINVAL, good());

    return io_queue::drop(qt);
}

int dmtr::memory_queue::close() {
    std::lock_guard<std::recursive_mutex> lock(my_lock);

    my_good_flag = false;
    return 0;
}

void dmtr::memory_queue::start_threads() {
    my_push_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
        return push_thread(yield, tq);
    }));

    my_pop_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
        return pop_thread(yield, tq);
    }));
}
