#include "catnip_libos_queue.hh"
#include <dmtr/fail.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int catnip_libos_noop() {
    catnip_libos_noop();
    return 0;
}

__attribute__((__visibility__("default")))
void catnip_libos_free_pkt(struct rte_mbuf *packet) {
  rte_pktmbuf_free(packet);
}

__attribute__((__visibility__("default")))
struct rte_mbuf* catnip_libos_alloc_pkt(struct rte_mempool *mp) {
  return rte_pktmbuf_alloc(mp);
}

__attribute__((__visibility__("default")))
uint16_t catnip_libos_eth_tx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts) {
  return rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
}

__attribute__((__visibility__("default")))
uint16_t catnip_libos_eth_rx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts) {
  return rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
}

