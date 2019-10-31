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
        uint16_t port;
        namespace po = boost::program_options;
        po::options_description desc{"LWIP libos options"};
        desc.add_options()
            ("port", po::value<uint16_t>(&port)->default_value(6789),
             "Port for lwip queues to listen to");
        po::variables_map vm;
        po::parsed_options parsed =
            po::command_line_parser(argc, argv).options(desc).allow_unregistered().run();
        po::store(parsed, vm);
        po::notify(vm);
        //dmtr::lwip_queue::set_app_port(port);
    }

    ioqapi.register_queue_ctor(dmtr::io_queue::MEMORY_Q, dmtr::memory_queue::new_object);
    ioqapi.register_queue_ctor(dmtr::io_queue::NETWORK_Q, dmtr::lwip_queue::new_object);
    ioqapi.register_queue_ctor(dmtr::io_queue::SHARED_Q, dmtr::shared_queue::new_object);

    return 0;
}
