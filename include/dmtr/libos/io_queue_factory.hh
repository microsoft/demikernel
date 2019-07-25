// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_IO_QUEUE_FACTORY_HH_IS_INCLUDED
#define DMTR_LIBOS_IO_QUEUE_FACTORY_HH_IS_INCLUDED

#include "io_queue.hh"
#include <condition_variable>
#include <dmtr/types.h>
#include <memory>
#include <queue>
#include <unordered_map>

namespace dmtr {

class io_queue_factory
{
    public: typedef int (*ctor_type)(std::unique_ptr<io_queue> &q_out, int qd);
    private: typedef std::unordered_map<enum io_queue::category_id, ctor_type> ctors_type;

    private: ctors_type my_ctors;

    public: io_queue_factory();
    public: int register_ctor(enum io_queue::category_id cid, ctor_type ctor);
    public: int construct(std::unique_ptr<io_queue> &q_out, enum io_queue::category_id cid, int qd) const;
};

} // namespace dmtr

#endif /* DMTR_LIBOS_IO_QUEUE_FACTORY_HH_IS_INCLUDED */
