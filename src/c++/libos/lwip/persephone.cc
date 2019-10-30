#include <stdint.h>
#include <iostream>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <dmtr/libos/persephone.hh>
#include <dmtr/annot.h>
#include <dmtr/libos/io_queue.hh>
#include <dmtr/libos/memory_queue.hh>
#include "lwip_queue.hh"


int PspServiceUnit::init(int argc, char *argv[]) {
    if (my_type == dmtr::io_queue::category_id::NETWORK_Q) { //maybe prefix
        DMTR_OK(dmtr::lwip_queue::init_dpdk(argc, argv));
    }

    ioqapi.register_queue_ctor(dmtr::io_queue::MEMORY_Q, dmtr::memory_queue::new_object);
    ioqapi.register_queue_ctor(dmtr::io_queue::NETWORK_Q, dmtr::lwip_queue::new_object);

    return 0;
}
