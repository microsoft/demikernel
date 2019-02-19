#include "io_queue_factory.hh"

#include <dmtr/annot.h>

dmtr::io_queue_factory::io_queue_factory() {
}

int dmtr::io_queue_factory::register_ctor(enum io_queue::category_id cid, ctor_type ctor) {
    DMTR_NOTNULL(EINVAL, ctor);
    DMTR_TRUE(EEXIST, my_ctors.find(cid) == my_ctors.cend());

    my_ctors.insert(std::make_pair(cid, ctor));
    return 0;
}

int dmtr::io_queue_factory::construct(io_queue *&q_out, enum io_queue::category_id cid, int qd) const {
    auto it = my_ctors.find(cid);
    DMTR_TRUE(ENOENT, it != my_ctors.cend());
    ctor_type f = it->second;
    DMTR_NOTNULL(EINVAL, f);
    DMTR_OK(f(q_out, qd));
    return 0;
}

