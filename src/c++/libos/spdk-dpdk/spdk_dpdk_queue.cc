// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "spdk_dpdk_queue.hh"

#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/sga.h>
#include <iostream>
#include <dmtr/libos/mem.h>
#include <dmtr/libos/raii_guard.hh>
#include <netinet/in.h>
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
#include <spdk/env.h>
#include <spdk/log.h>
#include <spdk/nvme.h>
#include <unistd.h>

namespace bpo = boost::program_options;

bool dmtr::spdk_dpdk_queue::our_init_flag = false;

dmtr::spdk_dpdk_queue::spdk_dpdk_queue(int qd, dmtr::io_queue::category_id cid) :
    io_queue(cid, qd)
{
    if (cid == NETWORK_Q)
        net_queue = new lwip_queue(qd);
    else
        file_queue = new spdk_queue(qd);
        
}
 
int dmtr::spdk_dpdk_queue::init_spdk_dpdk(int argc, char *argv[]) {
    DMTR_TRUE(ERANGE, argc >= 0);
    if (argc > 0) {
        DMTR_NOTNULL(EINVAL, argv);
    }
    DMTR_TRUE(EPERM, !our_init_flag);

    if (getenv("SPDK_OFF") != NULL) {
        printf("Turning SPDK off\n");
        // didn't find any devices, but turn on dpdk anyway
        lwip_queue::init_dpdk(argc, argv);
    } else {
        std::string config_path;
        bpo::options_description desc("Allowed options");
        desc.add_options()
            ("help", "display usage information")
            ("config-path,c", bpo::value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file");
    
        bpo::variables_map vm;
        bpo::store(bpo::command_line_parser(argc, argv).options(desc).allow_unregistered().run(), vm);
        bpo::notify(vm);
        
        if (vm.count("help")) {
            std::cout << desc << std::endl;
            return 0;
        }

        if (access(config_path.c_str(), R_OK) == -1) {
            std::cerr << "Unable to find config file at `" << config_path << "`." << std::endl;
            return ENOENT;
        }
        YAML::Node config = YAML::LoadFile(config_path);

    
        struct spdk_env_opts opts;
        spdk_env_opts_init(&opts);
        struct spdk_pci_addr nic = {0,0x37,0, 0};
        opts.pci_whitelist = &nic;
        opts.num_pci_addr = 1;
        std::string eal_args = "--proc-type=auto";
        opts.env_context = (void*)eal_args.c_str();

        // use SPDK to init DPDK
    
        DMTR_OK(spdk_queue::init_spdk(config, &opts));
        DMTR_OK(lwip_queue::finish_dpdk_init(config));
    }
    our_init_flag = true;
    return 0;
}

int dmtr::spdk_dpdk_queue::new_net_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = NULL;
    DMTR_TRUE(EPERM, our_init_flag);
    q_out = std::unique_ptr<io_queue>(new spdk_dpdk_queue(qd, NETWORK_Q));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

int dmtr::spdk_dpdk_queue::new_file_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = NULL;
    DMTR_TRUE(EPERM, our_init_flag);
    q_out = std::unique_ptr<io_queue>(new spdk_dpdk_queue(qd, FILE_Q));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

int dmtr::spdk_dpdk_queue::socket(int domain, int type, int protocol) {
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(NETWORK_Q, my_cid);
    DMTR_NOTNULL(EINVAL, net_queue);
    return net_queue->socket(domain, type, protocol);
}

int
dmtr::spdk_dpdk_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size) {
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(NETWORK_Q, my_cid);
    DMTR_NOTNULL(EINVAL, net_queue);
    return net_queue->getsockname(saddr, size);
}

int dmtr::spdk_dpdk_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qt, int new_qd)
{
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(NETWORK_Q, my_cid);
    DMTR_NOTNULL(EINVAL, net_queue);
    return net_queue->accept(q_out, qt, new_qd);
}

int dmtr::spdk_dpdk_queue::listen(int backlog)
{
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(NETWORK_Q, my_cid);
    DMTR_NOTNULL(EINVAL, net_queue);
    return net_queue->listen(backlog);
}

int dmtr::spdk_dpdk_queue::bind(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(NETWORK_Q, my_cid);
    DMTR_NOTNULL(EINVAL, net_queue);
    return net_queue->bind(saddr, size);
}

int dmtr::spdk_dpdk_queue::connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(NETWORK_Q, my_cid);
    DMTR_NOTNULL(EINVAL, net_queue);
    return net_queue->connect(qt, saddr, size);
}

int dmtr::spdk_dpdk_queue::close() {
    int ret;
    if (my_cid == NETWORK_Q) {
        ret = net_queue->close();
        delete net_queue;
    } else {
        ret = file_queue->close();
        delete file_queue;
    }
    return ret;
}

int dmtr::spdk_dpdk_queue::open(const char *pathname, int flags)
{
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(FILE_Q, my_cid);
    DMTR_NOTNULL(EINVAL, file_queue);
    //TODO(ashmrtnz): Yay for only supporing a single file, so we do nothing! If
    // we choose to support multiple files we will need to so some sort of
    // lookup or something here.
    // TODO(ashmrtnz): O_TRUNC?
    file_queue->open(pathname, flags);
    return 0;
}

int dmtr::spdk_dpdk_queue::open2(const char *pathname, int flags, mode_t mode)
{
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(FILE_Q, my_cid);
    DMTR_NOTNULL(EINVAL, file_queue);
    //TODO(ashmrtnz): Yay for only supporing a single file, so we do nothing! If
    // we choose to support multiple files we will need to so some sort of
    // lookup or something here.
    // TODO(ashmrtnz): O_TRUNC?
    file_queue->open2(pathname, flags, mode);
    return 0;
}

int dmtr::spdk_dpdk_queue::creat(const char *pathname, mode_t mode)
{
    DMTR_TRUE(EPERM, our_init_flag);
    DMTR_TRUE(FILE_Q, my_cid);
    DMTR_NOTNULL(EINVAL, file_queue);
    //TODO(ashmrtnz): Yay for only supporing a single file, so we do nothing! If
    // we choose to support multiple files we will need to so some sort of
    // lookup or something here.
    // TODO(ashmrtnz): O_TRUNC?
    file_queue->creat(pathname, mode);
    return 0;
}

int dmtr::spdk_dpdk_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga)
{
    DMTR_TRUE(EPERM, our_init_flag);
    if (my_cid == NETWORK_Q) return net_queue->push(qt, sga);
    else return file_queue->push(qt, sga);    
}

int dmtr::spdk_dpdk_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EPERM, our_init_flag);
    if (my_cid == NETWORK_Q) return net_queue->pop(qt);
    else return file_queue->pop(qt);    
}

int dmtr::spdk_dpdk_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{ 
    DMTR_TRUE(EPERM, our_init_flag);
    if (my_cid == NETWORK_Q) return net_queue->poll(qr_out, qt);
    else return file_queue->poll(qr_out, qt);    
}
