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
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, [=](task::yield_type &yield, dmtr_qresult_t &qr_out) {
        std::lock_guard<std::mutex> lock(my_lock);
        my_ready_queue.push(sga);
        init_push_qresult(qr_out, sga);
        return 0;
    }));
    return 0;
}

int
dmtr::memory_queue::pop(dmtr_qtoken_t qt) {
    std::lock_guard<std::mutex> lock(my_lock);
    DMTR_OK(new_task(qt, DMTR_OPC_POP, [=](task::yield_type &yield, dmtr_qresult_t &qr_out) {
        dmtr_sgarray_t sga;
        while (true) {
            {
                std::lock_guard<std::mutex> lock(my_lock);
                if (!my_ready_queue.empty()) {
                    sga = my_ready_queue.front();
                    my_ready_queue.pop();
                    break;
                }
            }

            yield();
        }

        init_pop_qresult(qr_out, sga);
        return 0;
    }));
    return 0;
}

int dmtr::memory_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt) {
    std::lock_guard<std::mutex> lock(my_lock);
    return io_queue::poll(qr_out, qt);
}

int dmtr::memory_queue::drop(dmtr_qtoken_t qt)
{
    std::lock_guard<std::mutex> lock(my_lock);
    return io_queue::drop(qt);
}

