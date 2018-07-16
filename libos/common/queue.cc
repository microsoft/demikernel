// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/queue.cc
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
 
#include "common/queue.h"

namespace Zeus {
    
ssize_t
Queue::push(qtoken qt, struct sgarray &sga)
{
    size_t len = 0;
    for (int i = 0; i < sga.num_bufs; i++) {
        size += sga.bufs[i].len;
    }
   
    std::lock_guard<std::mutex> lock(qLock);
    if (!pops.empty()) {
        qtoken pop = pops.front();
        pops.pop_front();
        auto it = wakeup.find(pop);
        if (it != wakeup.end()) {
            it->second.notify_all();
        }
        wakeup.erase(it);
        finishedPops.insert(sga);
    }
    return len;
}
        
ssize_t
Queue::pop(qtoken qt, struct sgarray &sga) {
    size_t len = 0;
    // if we have a buffered push already
    if (!pushes.empty()) {
        SGArray &in = pushes.front();
        sga.num_bufs = in.num_bufs;
        for (int i = 0; i < in.num_bufs; i++) {
            sga.bufs[i].len = in.bufs[i].len;
            len += sga.bufs[i].len;
            sga.bufs[i].buf = in.bufs[i].buf;
        }
    } else {
        pops.push_back(qt);
    }
    return len;
}
   
ssize_t
Queue::wait(qtoken qt, struct sgarray &sga)
    auto it = finishedPops.find(qt);
    if (it == finishedPops.end()
    while (it == finishedPops.end()) {
        
    ssize_t poll(qtoken qt, struct sgarray &sga); // non-blocking check on a request

} // namespace Zeus
