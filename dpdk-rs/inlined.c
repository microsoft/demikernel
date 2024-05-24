/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

#include <rte_errno.h>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_mbuf.h>

void rte_pktmbuf_free_(struct rte_mbuf *packet)
{
    rte_pktmbuf_free(packet);
}

struct rte_mbuf *rte_pktmbuf_alloc_(struct rte_mempool *mp)
{
    return rte_pktmbuf_alloc(mp);
}

uint16_t rte_eth_tx_burst_(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts)
{
    return rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
}

uint16_t rte_eth_rx_burst_(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts)
{
    return rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
}

uint16_t rte_mbuf_refcnt_read_(const struct rte_mbuf *m)
{
    return rte_mbuf_refcnt_read(m);
}

uint16_t rte_mbuf_refcnt_update_(struct rte_mbuf *m, int16_t value)
{
    return rte_mbuf_refcnt_update(m, value);
}

char *rte_pktmbuf_adj_(struct rte_mbuf *m, uint16_t len)
{
    return rte_pktmbuf_adj(m, len);
}

int rte_pktmbuf_trim_(struct rte_mbuf *m, uint16_t len)
{
    return rte_pktmbuf_trim(m, len);
}

uint16_t rte_pktmbuf_headroom_(const struct rte_mbuf *m)
{
    return rte_pktmbuf_headroom(m);
}

uint16_t rte_pktmbuf_tailroom_(const struct rte_mbuf *m)
{
    return rte_pktmbuf_tailroom(m);
}

int rte_errno_()
{
    return rte_errno;
}

int rte_pktmbuf_chain_(struct rte_mbuf *head, struct rte_mbuf *tail)
{
    return rte_pktmbuf_chain(head, tail);
}

int rte_eth_rss_ip_()
{
    return RTE_ETH_RSS_IP;
}

int rte_eth_tx_offload_tcp_cksum_()
{
    return RTE_ETH_TX_OFFLOAD_TCP_CKSUM;
}

int rte_eth_rx_offload_tcp_cksum_()
{
    return RTE_ETH_RX_OFFLOAD_TCP_CKSUM;
}

int rte_eth_tx_offload_udp_cksum_()
{
    return RTE_ETH_TX_OFFLOAD_UDP_CKSUM;
}

int rte_eth_rx_offload_udp_cksum_()
{
    return RTE_ETH_RX_OFFLOAD_TCP_CKSUM;
}

int rte_eth_tx_offload_multi_segs_()
{
    return RTE_ETH_TX_OFFLOAD_MULTI_SEGS;
}

char *rte_pktmbuf_prepend_(struct rte_mbuf *m, uint16_t len)
{
    return rte_pktmbuf_prepend(m, len);
}
