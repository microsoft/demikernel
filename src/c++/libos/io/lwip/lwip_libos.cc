// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "lwip_queue.hh"

#include <netinet/in.h>
#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/libos/io/memory_queue.hh>
#include <dmtr/libos/io/io_queue_api.hh>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <rte_mempool.h>
#include <rte_mbuf.h>
#include <rte_ether.h>

#include <memory>

static std::unique_ptr<dmtr::io_queue_api> ioq_api;

int _dmtr_select_ioq_api(dmtr::io_queue_api *&p)
{
    if (p == NULL) {
        DMTR_NOTNULL(EINVAL, ioq_api.get());
        p = ioq_api.get();
    }
    return 0;
}

int dmtr_net_init(const char *app_cfg)
{
    DMTR_OK(dmtr::lwip_queue::net_init(app_cfg));
    return 0;
}

int dmtr_del_net_context(void *context)
{
    DMTR_OK(dmtr::lwip_queue::del_context(context));
    return 0;
}

int dmtr_init_net_context(void **out_context, void *in_context,
                          uint16_t port_id, uint16_t ring_pair_id,
                          struct in_addr ip)
{
    DMTR_OK(dmtr::lwip_queue::generate_context(*out_context, in_context, port_id, ring_pair_id, ip));
    return 0;
}

int dmtr_net_port_init(uint16_t port_id, void *mempool, uint32_t n_tx_rings, uint32_t n_rx_rings)
{
    if (!::rte_eth_dev_is_valid_port(port_id)) {
        std::cerr << "Network interface " << port_id << " is not valid for this program." << std::endl;
        return 1;
    }
    DMTR_OK(dmtr::lwip_queue::init_dpdk_port(
        port_id, *static_cast<struct rte_mempool *>(mempool), n_tx_rings, n_rx_rings)
    );
    return 0;
}

int dmtr_net_mempool_init(void **mempool_out, uint8_t numa_socket_id)
{
    DMTR_OK(dmtr::lwip_queue::net_mempool_init(*mempool_out, numa_socket_id));
    return 0;
}

int dmtr_set_fdir(void *net_context)
{
    DMTR_OK(dmtr::lwip_queue::set_fdir(net_context));
    return 0;
}

int dmtr_init_ctors(void *r_ioq_api)
{
    dmtr::io_queue_api *p = static_cast<dmtr::io_queue_api *>(r_ioq_api);
    DMTR_OK(_dmtr_select_ioq_api(p));
    p->register_queue_ctor(dmtr::io_queue::MEMORY_Q, dmtr::memory_queue::new_object);
    p->register_queue_ctor(dmtr::io_queue::NETWORK_Q, dmtr::lwip_queue::new_object);
    p->register_queue_ctor(dmtr::io_queue::SHARED_Q, dmtr::shared_queue::new_object);
    return 0;
}

int dmtr_init(int argc, char *argv[])
{
    DMTR_NULL(EINVAL, ioq_api.get());

    dmtr::io_queue_api *p = NULL;
    DMTR_OK(dmtr::io_queue_api::init(p, argc, argv));
    ioq_api = std::unique_ptr<dmtr::io_queue_api>(p);
    ioq_api->register_queue_ctor(dmtr::io_queue::MEMORY_Q, dmtr::memory_queue::new_object);
    ioq_api->register_queue_ctor(dmtr::io_queue::NETWORK_Q, dmtr::lwip_queue::new_object);
    ioq_api->register_queue_ctor(dmtr::io_queue::SHARED_Q, dmtr::shared_queue::new_object);
    return 0;
}

int dmtr_queue(int *qd_out)
{
    DMTR_NOTNULL(EINVAL, qd_out);
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    DMTR_OK(ioq_api->queue(*qd_out));
    return 0;
}

int dmtr_socket(int *qd_out, int domain, int type, int protocol)
{
    DMTR_NOTNULL(EINVAL, qd_out);
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->socket(*qd_out, domain, type, protocol);
}

int dmtr_getsockname(int qd, struct sockaddr * const saddr, socklen_t * const size)
{
    DMTR_NOTNULL(EPERM, ioq_api.get());

    return ioq_api->getsockname(qd, saddr, size);
}

int dmtr_listen(int qd, int backlog)
{
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->listen(qd, backlog);
}

int dmtr_bind(int qd, const struct sockaddr * const saddr, socklen_t size)
{
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->bind(qd, saddr, size);
}

int dmtr_accept(dmtr_qtoken_t *qtok_out, int sockqd)
{
    DMTR_NOTNULL(EINVAL, qtok_out);
    DMTR_NOTNULL(EPERM, ioq_api.get());

    return ioq_api->accept(*qtok_out, sockqd);
}

int dmtr_connect(int qd, const struct sockaddr *saddr, socklen_t size)
{
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->connect(qd, saddr, size);
}

int dmtr_close(int qd)
{
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->close(qd);
}

int dmtr_is_qd_valid(int *flag_out, int qd)
{
    DMTR_NOTNULL(EINVAL, flag_out);
    *flag_out = 0;
    DMTR_NOTNULL(EPERM, ioq_api.get());

    bool b = false;
    DMTR_OK(ioq_api->is_qd_valid(b, qd));
    if (b) {
        *flag_out = 1;
    }

    return 0;
}

int dmtr_push(dmtr_qtoken_t *qtok_out, int qd, const dmtr_sgarray_t *sga)
{
    DMTR_NOTNULL(EINVAL, qtok_out);
    DMTR_NOTNULL(EINVAL, sga);
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->push(*qtok_out, qd, *sga);
}

int dmtr_pop(dmtr_qtoken_t *qtok_out, int qd)
{
    DMTR_NOTNULL(EINVAL, qtok_out);
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->pop(*qtok_out, qd);
}

int dmtr_poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->poll(qr_out, qt);
}

int dmtr_drop(dmtr_qtoken_t qt)
{
    DMTR_NOTNULL(EINVAL, ioq_api.get());

    return ioq_api->drop(qt);
}

int dmtr_open2(int *qd_out, const char *pathname, int flags, mode_t mode) {
    DMTR_NOTNULL(EINVAL, qd_out);
    DMTR_NOTNULL(EPERM, ioq_api.get());

    return 0;
}

int dmtr_free_mbuf(dmtr_sgarray_t *sga) {
    DMTR_NOTNULL(EINVAL, sga);

    if (sga->mbuf == NULL) {
        free(sga->sga_buf);
    } else {
        struct rte_mbuf *mbuf = static_cast<struct rte_mbuf *>(sga->mbuf);
        rte_pktmbuf_free(mbuf);
    }

    return 0;
}
