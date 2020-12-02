#ifndef DMTR_LIBOS_CATNIP_LIBOS_INCLUDED
#define DMTR_LIBOS_CATNIP_LIBOS_INCLUDED

#include <catnip_libos.h>

#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_mbuf.h>

/*
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ethdev_core.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
*/

int catnip_libos_noop();

extern "C" {
  void catnip_libos_free_pkt(struct rte_mbuf *packet);
  struct rte_mbuf* catnip_libos_alloc_pkt(struct rte_mempool *mp);
  uint16_t catnip_libos_eth_tx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts);
  uint16_t catnip_libos_eth_rx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts);
}


#endif
