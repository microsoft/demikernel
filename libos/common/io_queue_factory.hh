// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/queue.h
 *   Basic queue
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

#ifndef DMTR_LIBOS_IO_QUEUE_FACTORY_HH_IS_INCLUDED
#define DMTR_LIBOS_IO_QUEUE_FACTORY_HH_IS_INCLUDED

#include "io_queue.hh"
#include <dmtr/types.h>

#include <condition_variable>
#include <mutex>
#include <queue>
#include <unordered_map>

namespace dmtr {

class io_queue_factory
{
    public: typedef int (*ctor_type)(io_queue *&q_out, int qd);
    private: typedef std::unordered_map<enum io_queue::category_id, ctor_type> ctors_type;

    private: ctors_type my_ctors;

    public: io_queue_factory();
    public: int register_ctor(enum io_queue::category_id cid, ctor_type ctor);
    public: int construct(io_queue *&q_out, enum io_queue::category_id cid, int qd) const;
};

} // namespace dmtr

#endif /* DMTR_LIBOS_IO_QUEUE_FACTORY_HH_IS_INCLUDED */
