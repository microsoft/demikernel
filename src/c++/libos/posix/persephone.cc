#include <stdint.h>
#include <iostream>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <dmtr/libos/persephone.hh>
#include <dmtr/annot.h>
#include <dmtr/libos/io_queue.hh>
#include <dmtr/libos/memory_queue.hh>
#include "posix_queue.hh"

int PspServiceUnit::init(int argc, char *argv[]) {

    ioqapi.register_queue_ctor(dmtr::io_queue::MEMORY_Q, dmtr::memory_queue::new_object);
    ioqapi.register_queue_ctor(dmtr::io_queue::NETWORK_Q, dmtr::posix_queue::new_net_object);
    ioqapi.register_queue_ctor(dmtr::io_queue::FILE_Q, dmtr::posix_queue::new_file_object);
    ioqapi.register_queue_ctor(dmtr::io_queue::SHARED_Q, dmtr::shared_queue::new_object);

    return 0;
}
