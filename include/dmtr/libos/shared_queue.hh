// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_SHARED_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_SHARED_QUEUE_HH_IS_INCLUDED

#include "io_queue.hh"

#include <condition_variable>
#include <dmtr/types.h>
#include <memory>
#include <mutex>
#include <queue>
#include <unordered_map>
#include <unordered_set>

namespace dmtr {

class shared_item {

    public: shared_item() {
        empty_flag.test_and_set();
    }

    public: std::atomic_flag empty_flag;
    public: std::atomic_flag full_flag = ATOMIC_FLAG_INIT;
    public: dmtr_sgarray_t to_pop;
};

class shared_queue : public io_queue {

    private: shared_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: void set_producer(shared_item *p) { producer = p; }
    public: void set_consumer(shared_item *c) { consumer = c; }
    public: virtual int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: virtual int pop(dmtr_qtoken_t qt);
    public: virtual int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);
    public: virtual int drop(dmtr_qtoken_t qt);
    public: virtual int close();

    private: std::unordered_map<dmtr_qtoken_t, dmtr_sgarray_t> push_queue;
    private: std::unordered_set<dmtr_qtoken_t> pop_set;

    private: shared_item *producer;
    private: shared_item *consumer;
    private: bool my_good_flag;

    private: int push_task(const dmtr_sgarray_t &sga);
    private: int pop_task(dmtr_qresult_t &qr_out);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_SHARED_QUEUE_HH_IS_INCLUDED */
