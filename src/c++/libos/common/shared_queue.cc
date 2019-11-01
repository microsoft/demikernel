// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/libos/shared_queue.hh>

#include <dmtr/annot.h>
#include <dmtr/libos/mem.h>
#include <iostream>

dmtr::shared_queue::shared_queue(int qd) :
    io_queue(SHARED_Q, qd),
    producer(NULL), consumer(NULL),
    my_good_flag(true)
{
    // reserve push_queue
}

int dmtr::shared_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    auto * const q = new shared_queue(qd);
    DMTR_NOTNULL(ENOMEM, q);
    q_out = std::unique_ptr<io_queue>(q);

    return 0;
}

int dmtr::shared_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) {
    DMTR_TRUE(EINVAL, my_good_flag);
    DMTR_NOTNULL(EINVAL, consumer);
    push_queue[qt] = sga;
    return 0;
}

int dmtr::shared_queue::push_task(const dmtr_sgarray_t &sga) {
    DMTR_NOTNULL(EINVAL, consumer);
    if (consumer->full_flag.test_and_set()) {
        return EAGAIN;
    }
    consumer->to_pop = sga;
    consumer->empty_flag.clear();
    return 0;
}

int dmtr::shared_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, my_good_flag);
    DMTR_NOTNULL(EINVAL, producer);
    pop_set.insert(qt);
    return 0;
}

int dmtr::shared_queue::pop_task(dmtr_qresult_t &qr_out) {
    DMTR_NOTNULL(EINVAL, producer);
    if (producer->empty_flag.test_and_set()) {
        return EAGAIN;
    }
    qr_out.qr_value.sga = producer->to_pop;
    producer->full_flag.clear();
    return 0;
}

int dmtr::shared_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, my_good_flag);
    qr_out.qr_qt = qt;
    qr_out.qr_qd = qd();
    auto it_push = push_queue.find(qt);
    if (it_push != push_queue.end()) {
        qr_out.qr_opcode = DMTR_OPC_PUSH;
        return push_task(it_push->second);
    }
    auto it_pop = pop_set.find(qt);
    if (it_pop != pop_set.end()) {
        qr_out.qr_opcode = DMTR_OPC_POP;
        return pop_task(qr_out);
    }
    DMTR_UNREACHABLE();
}

int dmtr::shared_queue::drop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EINVAL, my_good_flag);
    auto it_push = push_queue.find(qt);
    if (it_push != push_queue.end()) {
        push_queue.erase(it_push);
        return 0;
    }
    auto it_pop = pop_set.find(qt);
    if (it_pop != pop_set.end()) {
        pop_set.erase(it_pop);
        return 0;
    }
    DMTR_UNREACHABLE();
}

int dmtr::shared_queue::close() {
    my_good_flag = false;
    return 0;
}
