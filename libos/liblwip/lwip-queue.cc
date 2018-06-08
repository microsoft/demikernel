/*
 * Copyright (c) 2014, University of Washington.
 * All rights reserved.
 *
 * This file is distributed under the terms in the attached LICENSE file.
 * If you do not find this file, copies can be found by writing to:
 * ETH Zurich D-INFK, CAB F.78, Universitaetstr. 6, CH-8092 Zurich. 
 * Attn: Systems Group.
 */

/**
 * \file
 * \brief Arranet library code
 */

#include <stdio.h>
#include <assert.h>
#include <sys/socket.h>
#include <lwip/sock_chan_support.h>
#include <netdb.h>
#include <arranet.h>
#include <arranet_impl.h>
#include <acpi_client/acpi_client.h>

#include "inet_chksum.h"

#include <arranet_debug.h>

#include <stdint.h>
#include <inttypes.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_cycles.h>
#include <rte_lcore.h>
#include <rte_mbuf.h>
#include <rte_ip.h>
#include <limits.h>
#include <sys/time.h>
#include <getopt.h>

#define RX_RING_SIZE 128
#define TX_RING_SIZE 512

#define NUM_MBUFS            8191
#define MBUF_CACHE_SIZE       250

/* Values for the PTP messageType field. */
#define SYNC                  0x0
#define DELAY_REQ             0x1
#define PDELAY_REQ            0x2
#define PDELAY_RESP           0x3
#define FOLLOW_UP             0x8
#define DELAY_RESP            0x9
#define PDELAY_RESP_FOLLOW_UP 0xA
#define ANNOUNCE              0xB
#define SIGNALING             0xC
#define MANAGEMENT            0xD

#define NSEC_PER_SEC        1000000000L
#define KERNEL_TIME_ADJUST_LIMIT  20000
#define PTP_PROTOCOL             0x88F7

struct rte_mempool *mbuf_pool;
uint32_t ptp_enabled_port_mask;
uint8_t ptp_enabled_port_nb;
static uint8_t ptp_enabled_ports[RTE_MAX_ETHPORTS];

static const struct rte_eth_conf port_conf_default = {
	.rxmode = { .max_rx_pkt_len = ETHER_MAX_LEN }
};

static const struct ether_addr ether_multicast = {
	.addr_bytes = {0x01, 0x1b, 0x19, 0x0, 0x0, 0x0}
};

static ether_terminate_queue ether_terminate_queue_ptr = NULL;
static ether_get_mac_address_t ether_get_mac_address_ptr = NULL;
static ether_transmit_pbuf_list_t ether_transmit_pbuf_list_ptr = NULL;
static ether_get_tx_free_slots tx_free_slots_fn_ptr = NULL;
static ether_handle_free_TX_slot handle_free_tx_slot_fn_ptr = NULL;
static ether_rx_register_buffer rx_register_buffer_fn_ptr = NULL;
static ether_rx_get_free_slots rx_get_free_slots_fn_ptr = NULL;

uint64_t interrupt_counter = 0;
uint64_t total_rx_p_count = 0;
uint64_t total_rx_datasize = 0;
struct client_closure *g_cl = NULL;

//#define MAX_PACKETS     1024
#define MAX_PACKETS     2000
#define PACKET_SIZE     2048

#define MAX_PEERS       256

static int use_vtd = 0;
static int vtd_coherency = 1;

struct peer {
    uint32_t ip;
    struct eth_addr mac;
};

// Configure static ARP entries here
// IP addresses are in network byte order!
static struct peer peers[MAX_PEERS] = {
    {
        // XXX: This needs to be updated each time the tap interface is re-initialized
        .ip = 0x0102000a,       // 10.0.2.1
        /* .mac.addr = "\x86\x86\x0b\xda\x22\xd7", */
        .mac.addr = "\x12\x67\xb9\x3e\xe2\x2c",
    },
    {
        // XXX: This needs to be updated each time the tap interface is re-initialized
        .ip = 0x0164a8c0,       // 192.168.100.1
        .mac.addr = "\x5e\x93\xf2\xf1\xeb\xfa",
    },
    {
        .ip = 0xaf06d080,       // 128.208.6.175 - swingout2
        .mac.addr = "\x90\xe2\xba\x3a\x2e\xdd",
    },
    {
        .ip = 0xec06d080,       // 128.208.6.236 - swingout3
        .mac.addr = "\xa0\x36\x9f\x0f\xfb\xe2",
    },
    {
        .ip = 0x8106d080,       // 128.208.6.129 - swingout4
        .mac.addr = "\xa0\x36\x9f\x10\x01\x6e",
    },
    {
        .ip = 0x8206d080,       // 128.208.6.130 - swingout5
        .mac.addr = "\xa0\x36\x9f\x10\x00\xa2",
    },
    {
        .ip = 0xc506d080,       // 128.208.6.197 - swingout6
        .mac.addr = "\xa0\x36\x9f\x10\x03\x52",
    },
};
static int peers_alloc = 7;             // Set number of static ARP here!

#ifdef DEBUG_LATENCIES
static int rx_packets_available = MAX_PACKETS;
#endif

struct socket {
    struct socket *prev, *next;
    int type, protocol;
    int fd;
    bool passive, nonblocking, connected, hangup, shutdown;
    struct sockaddr_in bound_addr;
    struct sockaddr_in peer_addr;
    uint32_t my_seq, peer_seq, next_ack;
};

struct pkt_ip_headers {
    struct eth_hdr eth;
    struct ip_hdr ip;
} __attribute__ ((packed));

struct pkt_udp_headers {
    struct eth_hdr eth;
    struct ip_hdr ip;
    struct udp_hdr udp;
} __attribute__ ((packed));

struct pkt_tcp_headers {
    struct eth_hdr eth;
    struct ip_hdr ip;
    struct tcp_hdr tcp;
} __attribute__ ((packed));

// All known connections and those in progress
static struct socket *connections = NULL;

static struct socket sockets[MAX_FD];
static struct packet rx_packets[MAX_PACKETS];

// XXX: Needs to be per socket later on
static struct waitset_chanstate recv_chanstate;
static struct waitset_chanstate send_chanstate;

static struct packet *inpkt = NULL;

#ifdef DEBUG_LATENCIES
static size_t memcache_packets_received = 0;
static size_t output_pipeline_stalled = 0;
static size_t port_cnt[65536];
static int lwip_send_time[POSIX_TRANSA];       // Time until packet was delivered to network interface
static size_t lwip_send_transactions = 0;
int posix_recv_time[POSIX_TRANSA];       // Time until packet at exit of recvfrom
size_t posix_recv_transactions = 0;
static int posix_send_time[POSIX_TRANSA];       // Time until packet at entry to sendto
static size_t posix_send_transactions = 0;

int memcache_times[20][POSIX_TRANSA];       // Time until packet was delivered to network interface
size_t memcache_transactions[20];

size_t hash_option1 = 0;
size_t hash_option2 = 0;
size_t hash_option3 = 0;
size_t hash_length = 0;
size_t hash_calls = 0;
size_t hash_aligned = 0;
size_t hash_unaligned = 0;
#endif

static bool arranet_udp_accepted = false;
static bool arranet_tcp_accepted = false;
static bool arranet_raw_accepted = false;

//#define TCP_LOCAL_PORT_RANGE_START        0xc000
#define TCP_LOCAL_PORT_RANGE_START        8081
#define TCP_LOCAL_PORT_RANGE_END          0xffff

static uint16_t free_tcp_ports[TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START + 1];
static uint16_t free_tcp_tail = TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START,
    free_tcp_free = TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START + 1;

#ifdef SENDMSG_WITH_COPY
static uint16_t free_tcp_head = 0;

// In network byte order
static uint16_t tcp_new_port(void)
{
  if(free_tcp_free > 0) {
      free_tcp_free--;
      u16_t new_port = free_tcp_ports[free_tcp_head];
      free_tcp_head = (free_tcp_head + 1) % (TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START + 1);
      /* printf("Allocating port %d\n", new_port); */
      return new_port;
  } else {
      printf("No more free ports!\n");
      return 0;
  }
}
#endif

static void tcp_free_port(uint16_t port)
{
    /* if(pcb->local_port == 8080) { */
    /*     return; */
    /* } */
    /* if(pcb->local_port == 8080) { */
    /*     printf("Freeing 8080 from %p %p %p\n", */
    /*            __builtin_return_address(0), */
    /*            __builtin_return_address(1), */
    /*            __builtin_return_address(2)); */
    /* } */
    /* assert(pcb->local_port != 8080); */
    assert(free_tcp_free < TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START + 1);

    /* printf("Freeing port %d\n", pcb->local_port); */

    /* for(int i = 0; i < free_tcp_free; i++) { */
    /*     u16_t entry = free_tcp_ports[(i + free_tcp_head) % (TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START + 1)]; */
    /*     assert(entry != pcb->local_port); */
    /* } */

    free_tcp_free++;
    free_tcp_tail = (free_tcp_tail + 1) % (TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START + 1);
    free_tcp_ports[free_tcp_tail] = port;
}

static struct socket *free_sockets_queue[MAX_FD];
static int free_sockets_head = 0, free_sockets_tail = MAX_FD - 1,
    free_sockets = MAX_FD;

static struct socket *alloc_socket(void)
{
    if(free_sockets == 0) {
        return NULL;
    }

    free_sockets--;
    struct socket *new_socket = free_sockets_queue[free_sockets_head];
    // Reset all fields except FD
    int fd_save = new_socket->fd;
    uint32_t seq_save = new_socket->my_seq;
    memset(new_socket, 0, sizeof(struct socket));
    new_socket->fd = fd_save;
    new_socket->my_seq = seq_save + 1000;
    free_sockets_head = (free_sockets_head + 1) % MAX_FD;
    /* printf("alloc_socket: returned %p\n", new_socket); */
    return new_socket;
}

static void free_socket(struct socket *sock)
{
    /* printf("free_socket: %p\n", sock); */
    assert(sock != NULL);
    assert(free_sockets < MAX_FD);
    free_sockets++;
    free_sockets_tail = (free_sockets_tail + 1) % MAX_FD;
    free_sockets_queue[free_sockets_tail] = sock;
}

/******** IP config *********/

struct mac2ip {
    uint8_t mac[ETHARP_HWADDR_LEN];
    uint32_t ip;
};

static struct mac2ip ip_config[] = {
    {   // QEMU
        .mac = "\x52\x54\x00\x12\x34\x56",
        /* .ip = 0x0a00020f,       // 10.0.2.15 */
        .ip = 0xc0a8640f,       // 192.168.100.15
    },
    {
        // QEMU2
        .mac = "\x52\x54\x00\x12\x34\x57",
        .ip = 0xc0a80102,       // 192.168.1.2
    },
    {   // swingout1 (and swingout1-vf0)
        .mac = "\xa0\x36\x9f\x10\x00\xa6",
        .ip = 0x80d00643,       // 128.208.6.67
    },
    {   // swingout1-vf1
        .mac = "\x22\xc9\xfc\x96\x83\xfc",
        .ip = 0x80d00644,       // 128.208.6.68
    },
    {   // swingout1-vf2
        .mac = "\xce\x43\x5b\xf7\x3e\x60",
        .ip = 0x80d00602,       // 128.208.6.2
    },
    {   // swingout1-vf3
        .mac = "\x6a\xb0\x62\xf6\xa7\x21",
        .ip = 0x80d00603,       // 128.208.6.3
    },
    {   // swingout1-vf4
        .mac = "\xb2\xdf\xf9\x39\xc6\x10",
        .ip = 0x80d00604,       // 128.208.6.4
    },
    {   // swingout1-vf5
        .mac = "\x92\x77\xe7\x3f\x80\x30",
        .ip = 0x80d0060c,       // 128.208.6.12
    },
    {   // swingout5
        .mac = "\xa0\x36\x9f\x10\x00\xa2",
        .ip = 0x80d00682,       // 128.208.6.130
    },
};

static uint8_t arranet_mymac[ETHARP_HWADDR_LEN];
static uint32_t arranet_myip = 0;

int lwip_read(int s, void *mem, size_t len)
{
    return lwip_recv(s, mem, len, 0);
}

int lwip_write(int s, const void *data, size_t size)
{
    return lwip_send(s, data, size, 0);
}

int lwip_fcntl(int s, int cmd, int val)
{
    struct socket *sock = &sockets[s];
    int retval = 0;

    switch(cmd) {
    case F_GETFL:
        retval = sock->nonblocking ? O_NONBLOCK : 0;
        break;

    case F_SETFL:
        sock->nonblocking = val & O_NONBLOCK ? true : false;
        break;

    default:
        assert(!"NYI");
        retval = -1;
        break;
    }

    return retval;
}

int lwip_listen(int s, int backlog)
{
    struct socket *sock = &sockets[s];
    sock->passive = true;
    return 0;
}

int lwip_getsockopt(int s, int level, int optname, void *optval, socklen_t *optlen)
{
    int retval = 0;

    switch(level) {
    case SOL_SOCKET:
        switch(optname) {
        case SO_SNDBUF:
            {
                assert(*optlen >= sizeof(int));
                int *ret = optval;
                *ret = PACKET_SIZE;
                *optlen = sizeof(int);
            }
            break;

        case SO_ERROR:
            {
                assert(*optlen >= sizeof(int));
                int *ret = optval;
                struct socket *sock = &sockets[s];
                assert(sock != NULL);
                *ret = sock->connected ? 0 : EINPROGRESS;
                *optlen = sizeof(int);
            }
            break;

        default:
            assert(!"NYI");
            retval = -1;
            break;
        }
        break;

    default:
        assert(!"NYI");
        retval = -1;
        break;
    }

    return retval;
}

int lwip_setsockopt(int s, int level, int optname, const void *optval, socklen_t optlen)
{
    int retval = 0;

    switch(level) {
    case SOL_SOCKET:
        switch(optname) {
        case SO_REUSEADDR:
        case SO_REUSEPORT:
            // No-op
            break;

        case SO_SNDBUF:
            {
                int len = *(const int *)optval;
                if(len > PACKET_SIZE) {
                    retval = -1;
                }
            }
            break;

        default:
            printf("%d, %d\n", level, optname);
            assert(!"NYI");
            retval = -1;
            break;
        }
        break;

    case IPPROTO_TCP:
        switch(optname) {
        case TCP_NODELAY:
            // XXX: No-op. We don't support Nagling anyway.
            break;
        }
        break;

    default:
        assert(!"NYI");
        retval = -1;
        break;
    }

    return retval;
}

int lwip_getsockname(int s, struct sockaddr *name, socklen_t *namelen)
{
    struct socket *sock = &sockets[s];
    assert(sock != NULL);
    assert(*namelen >= sizeof(struct sockaddr_in));

    memcpy(name, &sock->bound_addr, sizeof(struct sockaddr_in));
    *namelen = sizeof(struct sockaddr_in);

    return 0;
}

int lwip_getaddrinfo(const char *nodename, const char *servname,
                     const struct addrinfo *hints, struct addrinfo **res)
{
    struct addrinfo *r = calloc(1, sizeof(struct addrinfo));
    struct sockaddr_in *sa = calloc(1, sizeof(struct sockaddr_in));

    assert(hints != NULL);

    sa->sin_family = AF_INET;
    sa->sin_port = htons(atoi(servname));
    sa->sin_addr.s_addr = INADDR_ANY;

    // Return dummy UDP socket address
    r->ai_flags = AI_PASSIVE;
    r->ai_family = AF_INET;
    if(hints->ai_socktype != 0) {
        r->ai_socktype = hints->ai_socktype;
    } else {
        r->ai_socktype = SOCK_DGRAM;
    }
    r->ai_protocol = hints->ai_protocol;
    r->ai_addrlen = sizeof(struct sockaddr_in);
    r->ai_addr = (struct sockaddr *)sa;
    r->ai_canonname = NULL;
    r->ai_next = NULL;

    *res = r;
    return 0;
}

void lwip_freeaddrinfo(struct addrinfo *ai)
{
    for(struct addrinfo *i = ai; i != NULL;) {
        struct addrinfo *oldi = i;
        free(i->ai_addr);
        i = i->ai_next;
        free(oldi);
    }
}

/* The following 2 are #defined in lwIP 1.4.1, but not in 1.3.1, duplicating them here */

char *inet_ntoa(struct in_addr addr)
{
    return ipaddr_ntoa((ip_addr_t *)&addr);
}

int inet_aton(const char *cp, struct in_addr *addr)
{
    return ipaddr_aton(cp, (ip_addr_t *)addr);
}

u32_t inet_addr(const char *cp)
{
    return ipaddr_addr(cp);
}

/***** lwIP-compatibility functions, so that NFS and RPC code compiles *****/

u8_t pbuf_free_tagged(struct pbuf *p, const char *func_name, int line_no)
{
    assert(!"NYI");
}

u8_t pbuf_header(struct pbuf *p, s16_t header_size_increment)
{
    assert(!"NYI");
}

struct udp_pcb;

err_t udp_send(struct udp_pcb *pcb, struct pbuf *p);
err_t udp_send(struct udp_pcb *pcb, struct pbuf *p)
{
    assert(!"NYI");
}

struct udp_pcb *udp_new(void);
struct udp_pcb *udp_new(void)
{
    assert(!"NYI");
}

void udp_recv(struct udp_pcb *pcb,
              void (*recvfn) (void *arg, struct udp_pcb * upcb,
                            struct pbuf * p,
                            struct ip_addr * addr,
                            u16_t port), void *recv_arg);
void udp_recv(struct udp_pcb *pcb,
              void (*recvfn) (void *arg, struct udp_pcb * upcb,
                            struct pbuf * p,
                            struct ip_addr * addr,
                            u16_t port), void *recv_arg)
{
    assert(!"NYI");
}

void udp_remove(struct udp_pcb *pcb);
void udp_remove(struct udp_pcb *pcb)
{
    assert(!"NYI");
}

err_t udp_connect(struct udp_pcb *pcb, ip_addr_t *ipaddr, u16_t port);
err_t udp_connect(struct udp_pcb *pcb, ip_addr_t *ipaddr, u16_t port)
{
    assert(!"NYI");
}

struct pbuf *pbuf_alloc_tagged(pbuf_layer layer, u16_t length, pbuf_type type, const char *func_name, int line_no)
{
    assert(!"NYI");
}

void lwip_record_event_simple(uint8_t event_type, uint64_t ts);
void lwip_record_event_simple(uint8_t event_type, uint64_t ts)
{
    assert(!"NYI");
}

uint64_t wrapper_perform_lwip_work(void);
uint64_t wrapper_perform_lwip_work(void)
{
    assert(!"NYI");
}

bool lwip_init_auto(void);
bool lwip_init_auto(void)
{
    assert(!"NYI");
}

/******** NYI *********/

struct thread_mutex *lwip_mutex = NULL;
struct waitset *lwip_waitset = NULL;

void lwip_mutex_lock(void)
{
}

void lwip_mutex_unlock(void)
{
}

struct hostent *lwip_gethostbyname(const char *name)
{
    assert(!"NYI");
}

int lwip_getpeername(int s, struct sockaddr *name, socklen_t *namelen)
{
    assert(!"NYI");
}

/******** NYI END *********/

void ethernetif_backend_init(char *service_name, uint64_t queueid,
                             ether_get_mac_address_t get_mac_ptr,
                             ether_terminate_queue terminate_queue_ptr,
                             ether_transmit_pbuf_list_t transmit_ptr,
                             ether_get_tx_free_slots tx_free_slots_ptr,
                             ether_handle_free_TX_slot handle_free_tx_slot_ptr,
                             size_t rx_bufsz,
                             ether_rx_register_buffer rx_register_buffer_ptr,
                             ether_rx_get_free_slots rx_get_free_slots_ptr)
{
    ether_terminate_queue_ptr = terminate_queue_ptr;
    ether_get_mac_address_ptr = get_mac_ptr;
    ether_transmit_pbuf_list_ptr = transmit_ptr;
    tx_free_slots_fn_ptr = tx_free_slots_ptr;
    handle_free_tx_slot_fn_ptr = handle_free_tx_slot_ptr;
    rx_register_buffer_fn_ptr = rx_register_buffer_ptr;
    rx_get_free_slots_fn_ptr = rx_get_free_slots_ptr;
    /* printf("PBUF_POOL_BUFSIZE = %u, rx buffer size = %zu\n", PBUF_POOL_BUFSIZE, */
    /*        rx_bufsz); */
}

#define MAX_DRIVER_BUFS         16

static genpaddr_t rx_pbase = 0, tx_pbase = 0;
static genvaddr_t rx_vbase = 0, tx_vbase = 0;

static struct packet tx_packets[MAX_PACKETS];
/* static uint8_t tx_bufs[MAX_PACKETS][PACKET_SIZE]; */
static unsigned int tx_idx = 0;
/* static ssize_t tx_packets_available = MAX_PACKETS; */

#include <barrelfish/deferred.h>

static void packet_output(struct packet *p)
{
    struct driver_buffer bufs[MAX_DRIVER_BUFS];
    int n = 0;

#ifdef DEBUG_LATENCIES
    if(memcache_transactions[6] < POSIX_TRANSA) {
        if(p->next == NULL) {
            assert(p->next == NULL && p->len >= sizeof(protocol_binary_request_no_extras));
            protocol_binary_request_no_extras *mypayload = (void *)p->payload + SIZEOF_ETH_HDR + 20 + sizeof(struct udp_hdr) + UDP_HEADLEN;
            memcache_times[6][memcache_transactions[6]] = get_time() - mypayload->message.header.request.opaque;
            memcache_transactions[6]++;
        } else {
            protocol_binary_request_no_extras *mypayload = (void *)p->next->payload + UDP_HEADLEN;
            memcache_times[6][memcache_transactions[6]] = get_time() - mypayload->message.header.request.opaque;
            memcache_transactions[6]++;
        }
    }
#endif

    for (struct packet *q = p; q != NULL; q = q->next) {
        struct driver_buffer *buf = &bufs[n];

        /* if(q->payload < &tx_bufs[0][0] || q->payload >= &tx_bufs[MAX_PACKETS][PACKET_SIZE]) { */
        /*     printf("Called from %p %p\n", */
        /*            __builtin_return_address(0), */
        /*            __builtin_return_address(1)); */
        /*     assert(q->payload >= &tx_bufs[0][0] && q->payload < &tx_bufs[MAX_PACKETS][PACKET_SIZE]); */
        /* } */

        /* Send the data from the pbuf to the interface, one pbuf at a
           time. The size of the data in each pbuf is kept in the ->len
           variable. */
        assert(q->len > 0);

        // Check if it's from the RX region
        /* printf("RX region: Comparing %p against [%p:%p]\n", */
        /*        q->payload, */
        /*        (void *)rx_vbase, */
        /*        (void *)(rx_vbase + (MAX_PACKETS * PACKET_SIZE + 4096))); */
	if (!use_vtd) {
            if(((genvaddr_t)q->payload) >= rx_vbase &&
               ((genvaddr_t)q->payload) < rx_vbase + (MAX_PACKETS * PACKET_SIZE + 4096)) {
                buf->pa = rx_pbase + ((genvaddr_t)q->payload - rx_vbase);
            } else if(((genvaddr_t)q->payload) >= tx_vbase &&
                      ((genvaddr_t)q->payload) < tx_vbase + (MAX_PACKETS * PACKET_SIZE)) {
                // It is from the TX region!
                buf->pa = tx_pbase + ((genvaddr_t)q->payload - tx_vbase);
            } else {
                // Check if it's in morecore's region
                struct morecore_state *mc_state = get_morecore_state();
                struct vspace_mmu_aware *mmu_state = &mc_state->mmu_state;
                genvaddr_t base = vregion_get_base_addr(&mmu_state->vregion);
                struct memobj_frame_list *i;

                // Walk frame list
                for(i = mmu_state->memobj.frame_list; i != NULL; i = i->next) {
                    // If address is completely within frame, we can resolve
                    // XXX: Everything else would be easier with an IOMMU
		    /* printf("Heap: Comparing [%p:%p] against [%p:%p]\n", */
		    /*        q->payload, q->payload + q->len, */
		    /*        (void *)(base + i->offset), */
		    /*        (void *)(base + i->offset + i->size)); */
                    if(base + i->offset <= (genvaddr_t)q->payload &&
                       ((genvaddr_t)q->payload) + q->len < base + i->offset + i->size) {
                        assert(i->pa != 0);

                        /* buf->pa = id.base + ((genvaddr_t)q->payload - base - i->offset); */
                        buf->pa = i->pa + ((genvaddr_t)q->payload - base - i->offset);
                        break;
                    }
                }

                if(i == NULL) {
                    // Check if it's in text/data region
                    int entry;
                    for(entry = 0; entry < mc_state->v2p_entries; entry++) {
                        struct v2pmap *pmap = &mc_state->v2p_mappings[entry];

                        // If address is completely within frame, we can resolve
                        // XXX: Everything else would be easier with an IOMMU
                        /* printf("BSS: Comparing [%p:%p] against [%p:%p]\n", */
                        /*        q->payload, q->payload + q->len, */
                        /*        (void *)(pmap->va), */
                        /*        (void *)(pmap->va + pmap->size)); */
                        if(pmap->va <= (genvaddr_t)q->payload &&
                                ((genvaddr_t)q->payload) + q->len < pmap->va + pmap->size) {
                            buf->pa = pmap->pa + ((genvaddr_t)q->payload - pmap->va);
                            break;
                        }
                    }

                    if(entry == mc_state->v2p_entries) {
                        printf("Called from %p %p\n",
                                __builtin_return_address(0),
                                __builtin_return_address(1),
                                __builtin_return_address(2));

                        USER_PANIC("Invalid pbuf! payload = %p, pa = %p, subpacket = %d\n",
                                   q->payload, buf->pa, n);
                    }
                }
            }
        }

        /* printf("Sending: '%s'\n", (char *)q->payload); */

        buf->va = q->payload;
        buf->len = q->len;
#ifndef SENDMSG_WITH_COPY
        buf->opaque = q->opaque;
#else
        buf->opaque = q;
#endif
        buf->flags = q->flags;

        n++;
    }

#ifdef DEBUG_LATENCIES
    if(lwip_send_transactions < POSIX_TRANSA) {
        struct ip_hdr *iphdr = (struct ip_hdr *)(p->payload + SIZEOF_ETH_HDR);

        if(IPH_PROTO(iphdr) == IP_PROTO_UDP) {
            struct udp_hdr *udphdr = (struct udp_hdr *)(p->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
            if(htons(udphdr->src) == 11212 || htons(udphdr->src) == 11211) {
                protocol_binary_response_no_extras *mypayload;
                if(p->next != NULL) {
                    mypayload = (void *)p->next->next->payload;
                } else {
                    mypayload = (void *)p->payload + sizeof(struct pkt_udp_headers) + UDP_HEADLEN;
                }
                lwip_send_time[lwip_send_transactions] = get_time() - mypayload->message.header.response.opaque;
                lwip_send_transactions++;
            } else if (htons(udphdr->src) == 1234) {
                protocol_binary_request_no_extras *mypayload;
                if(p->next == NULL) {
                    mypayload = (void *)p->payload + sizeof(struct pkt_udp_headers) + UDP_HEADLEN;
                } else {
                    mypayload = (void *)p->next->payload + UDP_HEADLEN;
                }
                lwip_send_time[lwip_send_transactions] = get_time() - mypayload->message.header.request.opaque;
                lwip_send_transactions++;
            }
        }
    }
#endif

    errval_t err = ether_transmit_pbuf_list_ptr(bufs, n);
    assert(err_is_ok(err));
}

void arranet_recv_free(struct packet *p)
{
    assert(p >= rx_packets && p < &rx_packets[MAX_PACKETS]);

#ifdef DEBUG_LATENCIES
    rx_packets_available++;
#endif
    errval_t err = rx_register_buffer_fn_ptr(p->pa, p->payload, p);
    assert(err_is_ok(err));
}

struct recv_udp_args {
    void *buf;
    size_t len;
    int recv_len;
    struct sockaddr *src_addr;
    socklen_t *addrlen;
    struct packet **inpkt;
};

struct recv_tcp_args {
    void *buf;
    size_t len;
    int recv_len;
    struct sockaddr *src_addr;
    socklen_t *addrlen;
    struct packet **inpkt;
    bool syn, for_me;
    uint32_t in_seqno;
    struct socket *sock;
};

struct recv_raw_args {
    void *buf;
    size_t len;
    int recv_len;
    struct sockaddr *src_addr;
    socklen_t *addrlen;
    /* struct packet **inpkt; */
};

static void sock_recved_udp_packet(void *arg)
{
    struct recv_udp_args *args = arg;
    assert(inpkt != NULL);
    assert(inpkt->next == NULL);

    // Process headers
    struct ip_hdr *iphdr = (struct ip_hdr *)(inpkt->payload + SIZEOF_ETH_HDR);

    assert(IPH_PROTO(iphdr) == IP_PROTO_UDP);

    struct udp_hdr *udphdr = (struct udp_hdr *)(inpkt->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
    size_t hdr_len = SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr);
    uint8_t *payload = inpkt->payload + hdr_len;
    uint16_t pkt_len = htons(udphdr->len) - sizeof(struct udp_hdr);
    assert(args->buf != NULL);      // No accept() allowed

    // Fill in src_addr if provided
    if(args->src_addr != NULL) {
        struct sockaddr_in *addr = (struct sockaddr_in *)args->src_addr;

        assert(*args->addrlen >= sizeof(struct sockaddr_in));
        memset(addr, 0, sizeof(struct sockaddr_in));
        addr->sin_len = sizeof(struct sockaddr_in);
        addr->sin_family = AF_INET;
        addr->sin_port = udphdr->src;
        addr->sin_addr.s_addr = iphdr->src.addr;
        *args->addrlen = sizeof(struct sockaddr_in);
    }

    // It's a recvfrom!
    if(args->len != 0) {
#ifdef DEBUG_LATENCIES
    if(memcache_transactions[0] < POSIX_TRANSA) {
        protocol_binary_request_no_extras *mypayload = (void *)payload + UDP_HEADLEN;
        memcache_times[0][memcache_transactions[0]] = get_time() - mypayload->message.header.request.opaque;
        memcache_transactions[0]++;
    }
#endif

        args->recv_len = MIN(args->len, pkt_len);
        memcpy(args->buf, payload, args->recv_len);

#ifdef DEBUG_LATENCIES
    if(memcache_transactions[1] < POSIX_TRANSA) {
        protocol_binary_request_no_extras *mypayload = (void *)payload + UDP_HEADLEN;
        memcache_times[1][memcache_transactions[1]] = get_time() - mypayload->message.header.request.opaque;
        memcache_transactions[1]++;
    }
#endif

#ifdef DEBUG_LATENCIES
        rx_packets_available++;
#endif
        errval_t err = rx_register_buffer_fn_ptr(inpkt->pa, inpkt->payload, inpkt);
        assert(err_is_ok(err));
    } else {
        args->recv_len = pkt_len;
        *((void **)args->buf) = payload;
        *args->inpkt = inpkt;
    }

    // Input packet is consumed in stack
    inpkt = NULL;
}

static void sock_recved_raw_packet(void *arg)
{
    struct recv_raw_args *args = arg;
    assert(inpkt != NULL);
    assert(inpkt->next == NULL);

    // Process headers
    struct ip_hdr *iphdr = (struct ip_hdr *)(inpkt->payload + SIZEOF_ETH_HDR);
    assert(args->buf != NULL);      // No accept() allowed
    uint16_t pkt_len = ntohs(IPH_LEN(iphdr));
    uint8_t *payload = (void *)iphdr;

    // Fill in src_addr if provided
    if(args->src_addr != NULL) {
        struct sockaddr_in *addr = (struct sockaddr_in *)args->src_addr;

        assert(*args->addrlen >= sizeof(struct sockaddr_in));
        memset(addr, 0, sizeof(struct sockaddr_in));
        addr->sin_len = sizeof(struct sockaddr_in);
        addr->sin_family = AF_INET;
        addr->sin_port = 0;
        addr->sin_addr.s_addr = iphdr->src.addr;
        *args->addrlen = sizeof(struct sockaddr_in);
    }

    // It's a recvfrom!
    assert(args->len != 0);
    args->recv_len = MIN(args->len, pkt_len);
    memcpy(args->buf, payload, args->recv_len);
    errval_t err = rx_register_buffer_fn_ptr(inpkt->pa, inpkt->payload, inpkt);
    assert(err_is_ok(err));

    // Input packet is consumed in stack
    inpkt = NULL;
}

static void sock_recved_tcp_packet(void *arg)
{
    struct recv_tcp_args *args = arg;

    // Received only a FIN?
    if(inpkt == NULL) {
        args->recv_len = 0;
        args->for_me = true;
        return;
    }
    assert(inpkt != NULL);
    assert(inpkt->next == NULL);

    // Process headers
    struct ip_hdr *iphdr = (struct ip_hdr *)(inpkt->payload + SIZEOF_ETH_HDR);
    assert(IPH_PROTO(iphdr) == IP_PROTO_TCP);
    struct tcp_hdr *tcphdr = (struct tcp_hdr *)(inpkt->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
    size_t hdr_len = SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + (TCPH_HDRLEN(tcphdr) * 4);
    uint8_t *payload = inpkt->payload + hdr_len;
    uint16_t pkt_len = htons(IPH_LEN(iphdr)) - (TCPH_HDRLEN(tcphdr) * 4) - (IPH_HL(iphdr) * 4);

    args->in_seqno = tcphdr->seqno;
    args->for_me = true;

    // Is this from an accept() call?
    if(args->buf == NULL) {
        if(TCPH_FLAGS(tcphdr) & TCP_SYN) {
            args->syn = true;
        } else {
            // Don't consume packet
            args->for_me = false;
            return;
        }
    } else {    // From a recv() call
        if(TCPH_FLAGS(tcphdr) & TCP_SYN) {
            // Don't consume packet
            args->syn = true;
            args->for_me = false;
            return;
        } else {
            assert(args->sock != NULL);
            // Is this for the socket that's calling?
            if(tcphdr->dest != args->sock->bound_addr.sin_port ||
               tcphdr->src != args->sock->peer_addr.sin_port) {
                // Don't consume packet
                args->for_me = false;
                return;
            }

            if(args->len != 0) {
                assert(args->len >= pkt_len);
                args->recv_len = MIN(args->len, pkt_len);
                memcpy(args->buf, payload, args->recv_len);
            } else {
                assert(!"NYI");
                args->recv_len = pkt_len;
                *((void **)args->buf) = payload;
                *args->inpkt = inpkt;
            }
        }
    }

    // Fill in src_addr if provided
    if(args->src_addr != NULL) {
        struct sockaddr_in *addr = (struct sockaddr_in *)args->src_addr;

        assert(*args->addrlen >= sizeof(struct sockaddr_in));
        memset(addr, 0, sizeof(struct sockaddr_in));
        addr->sin_len = sizeof(struct sockaddr_in);
        addr->sin_family = AF_INET;
        addr->sin_port = tcphdr->src;
        addr->sin_addr.s_addr = iphdr->src.addr;
        *args->addrlen = sizeof(struct sockaddr_in);
    }

#ifdef DEBUG_LATENCIES
    rx_packets_available++;
#endif
    errval_t err = rx_register_buffer_fn_ptr(inpkt->pa, inpkt->payload, inpkt);
    assert(err_is_ok(err));

    // Input packet is consumed in stack
    inpkt = NULL;
}

int lwip_recv(int s, void *mem, size_t len, int flags)
{
    /* printf("lwip_recv(%d)\n", s); */
    assert(arranet_tcp_accepted);
    struct socket *sock = &sockets[s];
    struct recv_tcp_args args = {
        .buf = mem,
        .len = len,
        .src_addr = NULL,
        .syn = false,
        .sock = sock,
    };
    struct waitset ws;
    waitset_init(&ws);

    errval_t err = waitset_chan_register_polled(&ws, &recv_chanstate,
                                                MKCLOSURE(sock_recved_tcp_packet, &args));
    assert(err_is_ok(err));

    /* if socket is ready, trigger event right away */
    if (lwip_sock_ready_read(s)) {
        err = waitset_chan_trigger(&recv_chanstate);
        assert(err_is_ok(err));
    }

    if(sock->nonblocking) {
        err = event_dispatch_non_block(&ws);
        if(err_no(err) == LIB_ERR_NO_EVENT || !args.for_me) {
            err = waitset_chan_deregister(&recv_chanstate);
            assert(err_is_ok(err) ||
                   (err_no(err) == LIB_ERR_CHAN_NOT_REGISTERED && !args.for_me));
            errno = EAGAIN;
            args.recv_len = -1;
        } else {
            errno = 0;
            assert(err_is_ok(err));
        }
    } else {
        err = event_dispatch(&ws);
        assert(err_is_ok(err));
        if(args.syn) {
            assert(!"Will block forever");
        }
        errno = 0;
    }

    if(errno != EAGAIN) {
        sock->peer_seq = htonl(args.in_seqno);
        sock->next_ack = sock->peer_seq + args.recv_len;
        /* printf("lwip_recv: Assigning %p, %x\n", sock, sock->next_ack); */
    } else {
        // Did it shutdown?
        if(sock->hangup) {
            errno = 0;
            args.recv_len = 0;
        }
    }

#ifdef DEBUG_LATENCIES
    if(posix_recv_transactions < POSIX_TRANSA) {
        protocol_binary_request_no_extras *mypayload = mem + UDP_HEADLEN;
        posix_recv_time[posix_recv_transactions] = get_time() - mypayload->message.header.request.opaque;
        posix_recv_transactions++;
    }
#endif

    // Packet is now in buffer
    /* printf("lwip_recv returned %d\n", args.recv_len); */
    return args.recv_len;
}

int lwip_sendto(int s, const void *data, size_t size, int flags,
                const struct sockaddr *to, socklen_t tolen)
{
    struct iovec io = {
        .iov_base = (void *)data,
        .iov_len = size,
    };

    struct msghdr msg = {
        .msg_name = (void *)to,
        .msg_namelen = tolen,
        .msg_iov = &io,
        .msg_iovlen = 1,
        .msg_flags = 0,
    };

    return lwip_sendmsg(s, &msg, flags);
}

int lwip_socket(int domain, int type, int protocol)
{
    // XXX: Accept UDP or TCP, based on created sockets
    switch(type) {
    case SOCK_STREAM:
        assert(!arranet_udp_accepted);
        arranet_tcp_accepted = true;
        break;

    case SOCK_DGRAM:
        assert(!arranet_tcp_accepted);
        arranet_udp_accepted = true;
        break;

    case SOCK_RAW:
      assert(!arranet_tcp_accepted && !arranet_udp_accepted);
      assert(protocol == IPPROTO_TCP);
      arranet_raw_accepted = true;
      break;
    }

    struct socket *sock = alloc_socket();
    assert(sock != NULL);
    sock->type = type;
    sock->protocol = protocol;
    /* printf("lwip_socket() = %d\n", sock->fd); */
    return sock->fd;
}

int lwip_bind(int s, const struct sockaddr *name, socklen_t namelen)
{
    struct socket *sock = &sockets[s];
    assert(name->sa_family == AF_INET);
    assert(namelen >= sizeof(struct sockaddr_in));
    sock->bound_addr = *(struct sockaddr_in *)name;
    return 0;
}

int lwip_recvfrom(int sockfd, void *buf, size_t len, int flags,
                  struct sockaddr *src_addr, socklen_t *addrlen)
{
    assert(arranet_udp_accepted || arranet_raw_accepted);
    struct socket *sock = &sockets[sockfd];
    struct waitset ws;
    waitset_init(&ws);
    int *recv_len;
    errval_t err;
    struct recv_udp_args udp_args;
    struct recv_raw_args raw_args;

    switch(sock->type) {
    case SOCK_DGRAM:
      {
	  udp_args.buf = buf;
	  udp_args.len = len;
	  udp_args.src_addr = src_addr;
	  udp_args.addrlen = addrlen;

	  err = waitset_chan_register_polled(&ws, &recv_chanstate,
					     MKCLOSURE(sock_recved_udp_packet, &udp_args));
	  assert(err_is_ok(err));

	  recv_len = &udp_args.recv_len;
      }
      break;

    case SOCK_RAW:
      {
	  raw_args.buf = buf;
	  raw_args.len = len;
	  raw_args.src_addr = src_addr;
	  raw_args.addrlen = addrlen;

	  err = waitset_chan_register_polled(&ws, &recv_chanstate,
					     MKCLOSURE(sock_recved_raw_packet, &raw_args));
	  assert(err_is_ok(err));

	  recv_len = &raw_args.recv_len;
      }
      break;

    default:
        assert(!"NYI");
        break;
    }

    assert(err_is_ok(err));

    /* if socket is ready, trigger event right away */
    if (lwip_sock_ready_read(sockfd)) {
        err = waitset_chan_trigger(&recv_chanstate);
        assert(err_is_ok(err));
    }

    if(sock->nonblocking) {
        err = event_dispatch_non_block(&ws);
        if(err_no(err) == LIB_ERR_NO_EVENT) {
            err = waitset_chan_deregister(&recv_chanstate);
            assert(err_is_ok(err));
            errno = EAGAIN;
            *recv_len = -1;
        } else {
            assert(err_is_ok(err));
        }
    } else {
        err = event_dispatch(&ws);
        assert(err_is_ok(err));
    }

/* #ifdef DEBUG_LATENCIES */
/*     if(posix_recv_transactions < POSIX_TRANSA) { */
/*         protocol_binary_request_no_extras *mypayload = buf + UDP_HEADLEN; */
/*         posix_recv_time[posix_recv_transactions] = get_time() - mypayload->message.header.request.opaque; */
/*         posix_recv_transactions++; */
/*     } */
/* #endif */

    // Packet is now in buffer
    return *recv_len;
}

int recvfrom_arranet(int sockfd, void **buf, struct packet **p,
                     struct sockaddr *src_addr, socklen_t *addrlen)
{
    assert(arranet_udp_accepted);
    struct fdtab_entry *e = fdtab_get(sockfd);
    struct socket *sock = &sockets[e->fd];
    struct recv_udp_args args = {
        .buf = buf,
        .len = 0,
        .src_addr = src_addr,
        .addrlen = addrlen,
        .inpkt = p,
        .recv_len = 0,
    };
    struct waitset ws;
    waitset_init(&ws);

    errval_t err = waitset_chan_register_polled(&ws, &recv_chanstate,
                                                MKCLOSURE(sock_recved_udp_packet, &args));
    assert(err_is_ok(err));

    /* if socket is ready, trigger event right away */
    if (lwip_sock_ready_read(e->fd)) {
        err = waitset_chan_trigger(&recv_chanstate);
        assert(err_is_ok(err));
    }

    if(sock->nonblocking) {
        err = event_dispatch_non_block(&ws);
        if(err_no(err) == LIB_ERR_NO_EVENT) {
            err = waitset_chan_deregister(&recv_chanstate);
            assert(err_is_ok(err));
            errno = EAGAIN;
            args.recv_len = -1;
        } else {
            assert(err_is_ok(err));
        }
    } else {
        err = event_dispatch(&ws);
        assert(err_is_ok(err));
    }

/* #ifdef DEBUG_LATENCIES */
/*     if(posix_recv_transactions < POSIX_TRANSA) { */
/*         protocol_binary_request_no_extras *mypayload = (*buf) + UDP_HEADLEN; */
/*         posix_recv_time[posix_recv_transactions] = get_time() - mypayload->message.header.request.opaque; */
/*         posix_recv_transactions++; */
/*     } */
/* #endif */

    // XXX: Assert dword alignment
    assert(((long)*buf) % 8 == 0);

    // Packet is now in buffer
    return args.recv_len;
}

static struct pkt_ip_headers packet_ip_header;
static struct pkt_udp_headers packet_udp_header;
static struct pkt_tcp_headers packet_tcp_header;

static struct peer *peers_get_from_ip(uint32_t ip)
{
    for(int i = 0; i < MAX_PEERS; i++) {
        if(ip == peers[i].ip) {
            return &peers[i];
        }
    }

    /* printf("NOT FOUND: %x\n", ip); */

    return NULL;
}

static struct peer *peers_get_next_free(void)
{
    if(peers_alloc < MAX_PEERS) {
        return &peers[peers_alloc++];
    } else {
        return NULL;
    }
}

#define MAX_SENDMSG     16

int sendmsg_arranet(int sockfd, const struct msghdr *msg)
{
    assert(arranet_udp_accepted);
    struct fdtab_entry *e = fdtab_get(sockfd);
    struct socket *sock = &sockets[e->fd];
    ssize_t short_size = 0;
    struct packet packets[MAX_SENDMSG];
    struct packet hdrpkt;
    struct packet *oldp = NULL;

#ifdef DEBUG_LATENCIES
    if(posix_send_transactions < POSIX_TRANSA) {
        if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) {
            protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base;
            posix_send_time[posix_send_transactions] = get_time() - mypayload->message.header.response.opaque;
            posix_send_transactions++;
        } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) {
            protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base;
            posix_send_time[posix_send_transactions] = get_time() - mypayload->message.header.request.opaque;
            posix_send_transactions++;
        }
    }
#endif

    assert(msg->msg_iovlen < MAX_SENDMSG);

    for(int i = 0; i < msg->msg_iovlen; i++) {
        struct packet *newp = &packets[i];

        newp->payload = (uint8_t *)msg->msg_iov[i].iov_base;
        newp->len = msg->msg_iov[i].iov_len;
        newp->next = NULL;
        newp->flags = 0;
        if(oldp != NULL) {
            oldp->next = newp;
        }
        short_size += msg->msg_iov[i].iov_len;
        oldp = newp;
    }

    // Slap UDP/IP/Ethernet headers in front
    struct pkt_udp_headers myhdr = packet_udp_header;
    hdrpkt.payload = (uint8_t *)&myhdr;
    struct pkt_udp_headers *p = (struct pkt_udp_headers *)hdrpkt.payload;
    hdrpkt.len = sizeof(struct pkt_udp_headers);
    hdrpkt.next = packets;

    // Fine-tune headers
    assert(msg->msg_name != NULL);
    struct sockaddr_in *saddr = msg->msg_name;
    assert(saddr->sin_family == AF_INET);
    p->ip.dest.addr = saddr->sin_addr.s_addr;
    p->udp.dest = saddr->sin_port;
    struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
    p->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port != 0);
    p->udp.src = sock->bound_addr.sin_port;
    p->udp.len = htons(short_size + sizeof(struct udp_hdr));
    p->ip._len = htons(short_size + sizeof(struct udp_hdr) + IP_HLEN);
#ifdef CONFIG_QEMU_NETWORK
    p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
    hdrpkt.flags = 0;
#else
    // Hardware IP header checksumming on
    p->ip._chksum = 0;
    hdrpkt.flags = NETIF_TXFLAG_IPCHECKSUM;
#endif

    packet_output(&hdrpkt);

    return short_size;
}

static struct packet *get_tx_packet(void)
{
    struct packet *p = &tx_packets[tx_idx];

    // Busy-wait until packet not in flight
    while(p->len != 0) {
#ifdef DEBUG_LATENCIES
        output_pipeline_stalled++;
#endif
        /* printf("Pipeline stalled! tx_packets_available = %zd\n", tx_packets_available); */
        handle_free_tx_slot_fn_ptr();
        /* if(!handle_free_tx_slot_fn_ptr()) { */
        /*     printf("No packets could be freed!\n"); */
        /* } */
    }

    /* tx_packets_available--; */

    tx_idx = (tx_idx + 1) % MAX_PACKETS;
    return p;
}

int lwip_shutdown(int s, int how)
{
    assert(arranet_tcp_accepted);
    struct socket *sock = &sockets[s];
    assert(sock->nonblocking);

    /* printf("lwip_shutdown(%d)\n", s); */

    if(how == SHUT_RD || sock->shutdown) {
        return 0;
    }

    sock->shutdown = true;

#ifdef SENDMSG_WITH_COPY
    // Get new TX packet and send FIN-ACK
    struct packet *newp = get_tx_packet();
    newp->len = sizeof(struct pkt_tcp_headers);
    newp->next = NULL;

    // Slap TCP/IP/Ethernet headers in front
    memcpy(newp->payload, &packet_tcp_header, sizeof(struct pkt_tcp_headers));

    // Fine-tune headers
    struct pkt_tcp_headers *p = (struct pkt_tcp_headers *)newp->payload;
    p->ip.dest.addr = sock->peer_addr.sin_addr.s_addr;
    p->tcp.dest = sock->peer_addr.sin_port;
    struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
    p->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port != 0);
    p->tcp.src = sock->bound_addr.sin_port;
    p->ip._len = htons(sizeof(struct tcp_hdr) + IP_HLEN);
    p->tcp.seqno = htonl(sock->my_seq++);
    p->tcp.ackno = htonl(sock->next_ack);
    /* printf("lwip_shutdown: Sending %p, seq %x, ack %x\n", sock, sock->my_seq - 1, sock->next_ack); */
    TCPH_FLAGS_SET(&p->tcp, TCP_FIN | TCP_ACK); // Set FIN-ACK
    TCPH_HDRLEN_SET(&p->tcp, 5);   // 20 / 4
    p->tcp.wnd = htons(11680);
#ifdef CONFIG_QEMU_NETWORK
    p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
    p->tcp.chksum = 0;
    newp->payload = (uint8_t *)&p->tcp;
    newp->len -= (uint8_t *)&p->tcp - (uint8_t *)p;
    p->tcp.chksum = inet_chksum_pseudo(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                       IP_PROTO_TCP, TCP_HLEN);
    newp->payload = (uint8_t *)p;
    newp->len = sizeof(struct pkt_tcp_headers);
    newp->flags = 0;
#else
    // Hardware IP/TCP header checksumming on
    p->ip._chksum = 0;
    p->tcp.chksum =
        (~inet_chksum_pseudo_partial(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                     IP_PROTO_TCP, TCP_HLEN, 0)) & 0xffff;
    newp->flags = (NETIF_TXFLAG_IPCHECKSUM | NETIF_TXFLAG_TCPCHECKSUM) |
        (TCPH_HDRLEN(&p->tcp) << NETIF_TXFLAG_TCPHDRLEN_SHIFT);
#endif

    packet_output(newp);

    return 0;
#else
    assert(!"NYI");
#endif
}

int lwip_close(int s)
{
  if(arranet_udp_accepted) {
    // XXX: Ignore for now
    return 0;
  }

    assert(arranet_tcp_accepted);
    struct socket *sock = &sockets[s];

    lwip_shutdown(s, SHUT_RDWR);

    // Might need to return port if it was bound
    if(sock->bound_addr.sin_port != 0 && htons(sock->bound_addr.sin_port) != 8080) {
        tcp_free_port(sock->bound_addr.sin_port);
    }

    // Remove from active connections
    if(sock->prev != NULL) {
        sock->prev->next = sock->next;
    }
    if(sock->next != NULL) {
        sock->next->prev = sock->prev;
    }
    if(connections == sock) {
        connections = sock->next;
    }
    sock->next = sock->prev = NULL;

    free_socket(sock);
    return 0;
}

int lwip_send(int s, const void *data, size_t size, int flags)
{
    assert(arranet_tcp_accepted);
    struct socket *sock = &sockets[s];
    assert(sock->nonblocking);
    assert(size + sizeof(struct pkt_tcp_headers) <= 1500);

    /* printf("lwip_send(%d, , %zu)\n", s, size); */

#ifdef SENDMSG_WITH_COPY
    // Get new TX packet and copy data into it
    struct packet *newp = get_tx_packet();
    newp->len = sizeof(struct pkt_tcp_headers) + size;
    newp->next = NULL;
    uint8_t *buf = newp->payload + sizeof(struct pkt_tcp_headers);
    memcpy(buf, data, size);

    // Slap TCP/IP/Ethernet headers in front
    memcpy(newp->payload, &packet_tcp_header, sizeof(struct pkt_tcp_headers));

    // Fine-tune headers
    struct pkt_tcp_headers *p = (struct pkt_tcp_headers *)newp->payload;
    p->ip.dest.addr = sock->peer_addr.sin_addr.s_addr;
    p->tcp.dest = sock->peer_addr.sin_port;
    struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
    assert(peer != NULL);
    p->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port != 0);
    p->tcp.src = sock->bound_addr.sin_port;
    p->ip._len = htons(sizeof(struct tcp_hdr) + IP_HLEN + size);
    p->tcp.seqno = htonl(sock->my_seq);
    sock->my_seq += size;
    /* printf("lwip_send: Assigning %p, seq %x\n", sock, sock->my_seq); */
    p->tcp.ackno = htonl(sock->next_ack);
    /* printf("lwip_send: Sending %p, %x\n", sock, sock->next_ack); */
    TCPH_FLAGS_SET(&p->tcp, TCP_ACK | TCP_PSH);
    TCPH_HDRLEN_SET(&p->tcp, 5);   // 20 / 4
    p->tcp.wnd = htons(11680);
#ifdef CONFIG_QEMU_NETWORK
    p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
    p->tcp.chksum = 0;
    newp->payload = (uint8_t *)&p->tcp;
    newp->len -= (uint8_t *)&p->tcp - (uint8_t *)p;
    p->tcp.chksum = inet_chksum_pseudo(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                       IP_PROTO_TCP, TCP_HLEN + size);
    newp->payload = (uint8_t *)p;
    newp->len = sizeof(struct pkt_tcp_headers) + size;
    newp->flags = 0;
#else
    // Hardware IP/TCP header checksumming on
    p->ip._chksum = 0;
    p->tcp.chksum =
        (~inet_chksum_pseudo_partial(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                     IP_PROTO_TCP, TCP_HLEN + size, 0)) & 0xffff;
    newp->flags = (NETIF_TXFLAG_IPCHECKSUM | NETIF_TXFLAG_TCPCHECKSUM) |
        (TCPH_HDRLEN(&p->tcp) << NETIF_TXFLAG_TCPHDRLEN_SHIFT);
#endif

    packet_output(newp);

    return size;
#else
    assert(!"NYI");
#endif
}

int lwip_connect(int s, const struct sockaddr *name, socklen_t namelen)
{
    /* printf("lwip_connect(%d)\n", s); */
    assert(arranet_tcp_accepted);
    struct socket *sock = &sockets[s];
    assert(sock->nonblocking);
    assert(namelen == sizeof(struct sockaddr_in));
    struct sockaddr_in *sa = (struct sockaddr_in *)name;
    assert(sa->sin_family == AF_INET);

    // Store peer address on socket
    sock->peer_addr = *sa;

#ifdef SENDMSG_WITH_COPY
    // Get new TX packet and send SYN
    struct packet *newp = get_tx_packet();
    newp->len = sizeof(struct pkt_tcp_headers) + 6;
    newp->next = NULL;

    // Slap TCP/IP/Ethernet headers in front
    memcpy(newp->payload, &packet_tcp_header, sizeof(struct pkt_tcp_headers));

    // Fine-tune headers
    struct pkt_tcp_headers *p = (struct pkt_tcp_headers *)newp->payload;
    uint32_t *payload = (void *)p + sizeof(struct pkt_tcp_headers);
    memset(payload, 0, 6);
    p->ip.dest.addr = sa->sin_addr.s_addr;
    p->tcp.dest = sa->sin_port;
    struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
    assert(peer != NULL);
    p->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port == 0);
    sock->bound_addr.sin_port = tcp_new_port();
    p->tcp.src = sock->bound_addr.sin_port;
    p->ip._len = htons(sizeof(struct tcp_hdr) + IP_HLEN + 4);
    p->tcp.seqno = htonl(++sock->my_seq); sock->my_seq++;
    /* printf("lwip_connect: Assigning %p seq %x\n", sock, sock->my_seq); */
    p->tcp.ackno = 0;
    TCPH_FLAGS_SET(&p->tcp, TCP_SYN);
    TCPH_HDRLEN_SET(&p->tcp, 6);   // 24 / 4
    p->tcp.wnd = htons(11680);
    *payload = TCP_BUILD_MSS_OPTION(1460);
#ifdef CONFIG_QEMU_NETWORK
    p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
    p->tcp.chksum = 0;
    newp->payload = (uint8_t *)&p->tcp;
    newp->len -= (uint8_t *)&p->tcp - (uint8_t *)p;
    p->tcp.chksum = inet_chksum_pseudo(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                       IP_PROTO_TCP, TCP_HLEN + 4);
    newp->payload = (uint8_t *)p;
    newp->len = sizeof(struct pkt_tcp_headers) + 6;
    newp->flags = 0;
#else
    // Hardware IP/TCP header checksumming on
    p->ip._chksum = 0;
    p->tcp.chksum = 0;
    p->tcp.chksum =
        (~inet_chksum_pseudo_partial(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                     IP_PROTO_TCP, TCP_HLEN + 4, 0)) & 0xffff;
    newp->flags = (NETIF_TXFLAG_IPCHECKSUM | NETIF_TXFLAG_TCPCHECKSUM) |
        (TCPH_HDRLEN(&p->tcp) << NETIF_TXFLAG_TCPHDRLEN_SHIFT);
#endif

    packet_output(newp);

    assert(sock->prev == NULL && sock->next == NULL);
    sock->next = connections;
    if(connections != NULL) {
        assert(connections->prev == NULL);
        connections->prev = sock;
    }
    sock->prev = NULL;
    connections = sock;

    errno = EINPROGRESS;
    return -1;
#else
    assert(!"NYI");
#endif
}

#ifdef SENDMSG_WITH_COPY

int lwip_sendmsg(int sockfd, const struct msghdr *msg, int flags)
{
    assert(arranet_udp_accepted || arranet_raw_accepted);
    struct socket *sock = &sockets[sockfd];

#ifdef DEBUG_LATENCIES
    if(posix_send_transactions < POSIX_TRANSA) {
        if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) {
            protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base;
            posix_send_time[posix_send_transactions] = get_time() - mypayload->message.header.response.opaque;
            posix_send_transactions++;
        } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) {
            protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN;
            posix_send_time[posix_send_transactions] = get_time() - mypayload->message.header.request.opaque;
            posix_send_transactions++;
        }
    }
#endif

    assert(msg->msg_iovlen < MAX_SENDMSG);

    // Determine length of sendmsg vector
    ssize_t short_size = 0;
    for(int i = 0; i < msg->msg_iovlen; i++) {
        short_size += msg->msg_iov[i].iov_len;
    }
    assert(short_size <= PACKET_SIZE);

/* #ifdef DEBUG_LATENCIES */
/*     if(memcache_transactions[0] < POSIX_TRANSA) { */
/*         if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) { */
/*             protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base; */
/*             memcache_times[0][memcache_transactions[0]] = get_time() - mypayload->message.header.response.opaque; */
/*             memcache_transactions[0]++; */
/*         } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) { */
/*             protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN; */
/*             memcache_times[0][memcache_transactions[0]] = get_time() - mypayload->message.header.request.opaque; */
/*             memcache_transactions[0]++; */
/*         } */
/*     } */
/* #endif */

    // Get new TX packet and copy data into it
    struct packet *newp = get_tx_packet();
    uint8_t *buf = newp->payload;
    size_t pos;
    if(sock->type == SOCK_DGRAM) {
      pos = sizeof(struct pkt_udp_headers);
    } else {
      assert(sock->type == SOCK_RAW);
      pos = sizeof(struct pkt_ip_headers);
    }

/* #ifdef DEBUG_LATENCIES */
/*     if(memcache_transactions[1] < POSIX_TRANSA) { */
/*         if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) { */
/*             protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base; */
/*             memcache_times[1][memcache_transactions[1]] = get_time() - mypayload->message.header.response.opaque; */
/*             memcache_transactions[1]++; */
/*         } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) { */
/*             protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN; */
/*             memcache_times[1][memcache_transactions[1]] = get_time() - mypayload->message.header.request.opaque; */
/*             memcache_transactions[1]++; */
/*         } */
/*     } */

/*     uint64_t last = rdpmc(0); */
/* #endif */

    //    assert(msg->msg_iovlen == 1);
    for(int i = 0; i < msg->msg_iovlen; i++) {
        /* assert((uintptr_t)(&buf[pos]) % 8 == 0); */
        //        assert((uintptr_t)msg->msg_iov[i].iov_base % 8 == 0);
        memcpy(&buf[pos], msg->msg_iov[i].iov_base, msg->msg_iov[i].iov_len);
        pos += msg->msg_iov[i].iov_len;
    }

#ifdef DEBUG_LATENCIES
    /* uint64_t now = rdpmc(0); */

    /* if(memcache_transactions[19] < POSIX_TRANSA) {   // ZZZ 19 */
    /*     memcache_times[19][memcache_transactions[19]] = now - last; */
    /*     memcache_transactions[19]++; */
    /* } */

    if(memcache_transactions[2] < POSIX_TRANSA) {
        if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) {
            protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base;
            memcache_times[2][memcache_transactions[2]] = get_time() - mypayload->message.header.response.opaque;
            memcache_transactions[2]++;
        } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) {
            protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN;
            memcache_times[2][memcache_transactions[2]] = get_time() - mypayload->message.header.request.opaque;
            memcache_transactions[2]++;
        }
    }
#endif

    if(sock->type == SOCK_DGRAM) {
        newp->len = short_size + sizeof(struct pkt_udp_headers);

        // Slap UDP/IP/Ethernet headers in front
        memcpy(buf, &packet_udp_header, sizeof(struct pkt_udp_headers));
    } else {
        assert(sock->type == SOCK_RAW);
        newp->len = short_size + sizeof(struct pkt_ip_headers);
        // Slap IP/Ethernet headers in front
        memcpy(buf, &packet_ip_header, sizeof(struct pkt_ip_headers));
    }
    newp->next = NULL;

/* #ifdef DEBUG_LATENCIES */
/*     if(memcache_transactions[3] < POSIX_TRANSA) { */
/*         if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) { */
/*             protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base; */
/*             memcache_times[3][memcache_transactions[3]] = get_time() - mypayload->message.header.response.opaque; */
/*             memcache_transactions[3]++; */
/*         } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) { */
/*             protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN; */
/*             memcache_times[3][memcache_transactions[3]] = get_time() - mypayload->message.header.request.opaque; */
/*             memcache_transactions[3]++; */
/*         } */
/*     } */
/* #endif */

    if (sock->type = SOCK_DGRAM) {
        // Fine-tune headers
        struct pkt_udp_headers *p = (struct pkt_udp_headers *)buf;
        assert(msg->msg_name != NULL);
        struct sockaddr_in *saddr = msg->msg_name;
        assert(saddr->sin_family == AF_INET);
        p->ip.dest.addr = saddr->sin_addr.s_addr;
        p->udp.dest = saddr->sin_port;
        struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
        p->eth.dest = peer->mac;
        assert(sock->bound_addr.sin_port != 0);
        p->udp.src = sock->bound_addr.sin_port;
        p->udp.len = htons(short_size + sizeof(struct udp_hdr));
        p->ip._len = htons(short_size + sizeof(struct udp_hdr) + IP_HLEN);
#ifdef CONFIG_QEMU_NETWORK
        p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
        newp->flags = 0;
#else
        // Hardware IP header checksumming on
        p->ip._chksum = 0;
        newp->flags = NETIF_TXFLAG_IPCHECKSUM;
#endif
    } else {
      assert(sock->type == SOCK_RAW);
      // Fine-tune headers
      struct pkt_ip_headers *p = (struct pkt_ip_headers *)buf;
      assert(msg->msg_name != NULL);
      struct sockaddr_in *saddr = msg->msg_name;
      assert(saddr->sin_family == AF_INET);
      p->ip.dest.addr = saddr->sin_addr.s_addr;
      struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
      assert(peer != NULL);
      p->eth.dest = peer->mac;
      p->ip._len = htons(short_size + IP_HLEN);
      IPH_PROTO_SET(&p->ip, sock->protocol);
#ifdef CONFIG_QEMU_NETWORK
      p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
      newp->flags = 0;
#else
      // Hardware IP header checksumming on
      p->ip._chksum = 0;
      newp->flags = NETIF_TXFLAG_IPCHECKSUM;
#endif
    }

/* #ifdef DEBUG_LATENCIES */
/*     if(memcache_transactions[4] < POSIX_TRANSA) { */
/*         if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) { */
/*             protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base; */
/*             memcache_times[4][memcache_transactions[4]] = get_time() - mypayload->message.header.response.opaque; */
/*             memcache_transactions[4]++; */
/*         } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) { */
/*             protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN; */
/*             memcache_times[4][memcache_transactions[4]] = get_time() - mypayload->message.header.request.opaque; */
/*             memcache_transactions[4]++; */
/*         } */
/*     } */
/* #endif */

    packet_output(newp);

/* #ifdef DEBUG_LATENCIES */
/*     if(memcache_transactions[5] < POSIX_TRANSA) { */
/*         if(msg->msg_iovlen > 1 && msg->msg_iov[1].iov_len == sizeof(protocol_binary_response_no_extras)) { */
/*             protocol_binary_response_no_extras *mypayload = msg->msg_iov[1].iov_base; */
/*             memcache_times[5][memcache_transactions[5]] = get_time() - mypayload->message.header.response.opaque; */
/*             memcache_transactions[5]++; */
/*         } else if(msg->msg_iov[0].iov_len >= sizeof(protocol_binary_request_no_extras)) { */
/*             protocol_binary_request_no_extras *mypayload = msg->msg_iov[0].iov_base + UDP_HEADLEN; */
/*             memcache_times[5][memcache_transactions[5]] = get_time() - mypayload->message.header.request.opaque; */
/*             memcache_transactions[5]++; */
/*         } */
/*     } */
/* #endif */

    return short_size;
}

#else

int lwip_sendmsg(int sockfd, const struct msghdr *msg, int flags)
{
    struct socket *sock = &sockets[sockfd];
    ssize_t short_size = 0;
    struct packet packets[MAX_SENDMSG];
    struct packet *oldp = NULL;

    assert(msg->msg_iovlen < MAX_SENDMSG);

    for(int i = 0; i < msg->msg_iovlen; i++) {
        struct packet *newp = &packets[i];

        newp->payload = (uint8_t *)msg->msg_iov[i].iov_base;
        newp->len = msg->msg_iov[i].iov_len;
        newp->next = NULL;
        newp->flags = 0;
        newp->opaque = msg->msg_iov[i].iov_opaque;
        if(oldp != NULL) {
            oldp->next = newp;
        }
        short_size += msg->msg_iov[i].iov_len;
        oldp = newp;
    }

    // Slap UDP/IP/Ethernet headers in front
    struct packet *hdrpkt = get_tx_packet();
    memcpy(hdrpkt->payload, &packet_udp_header, sizeof(struct pkt_udp_headers));
    struct pkt_udp_headers *p = (struct pkt_udp_headers *)hdrpkt->payload;
    hdrpkt->len = sizeof(struct pkt_udp_headers);
    hdrpkt->next = packets;
    hdrpkt->opaque = hdrpkt;

    // Fine-tune headers
    assert(msg->msg_name != NULL);
    struct sockaddr_in *saddr = msg->msg_name;
    assert(saddr->sin_family == AF_INET);
    p->ip.dest.addr = saddr->sin_addr.s_addr;
    p->udp.dest = saddr->sin_port;
    struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
    p->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port != 0);
    p->udp.src = sock->bound_addr.sin_port;
    p->udp.len = htons(short_size + sizeof(struct udp_hdr));
    p->ip._len = htons(short_size + sizeof(struct udp_hdr) + IP_HLEN);
#ifdef CONFIG_QEMU_NETWORK
    p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
    hdrpkt->flags = 0;
#else
    // Hardware IP header checksumming on
    p->ip._chksum = 0;
    hdrpkt->flags = NETIF_TXFLAG_IPCHECKSUM;
#endif

    packet_output(hdrpkt);

    // If we sent the data directly, we need to wait here until everything is out.
    // Else, data might be overwritten by application before card can send it.
    /* while(!e1000n_queue_empty()) thread_yield(); */

    return short_size;
}

#endif

int lwip_accept(int s, struct sockaddr *addr, socklen_t *addrlen)
{
    assert(arranet_tcp_accepted);
    struct socket *sock = &sockets[s];
    assert(sock->passive);
    struct socket *newsock = alloc_socket();
    newsock->nonblocking = sock->nonblocking;
    newsock->bound_addr = sock->bound_addr;
    newsock->type = sock->type;
    socklen_t adlen = sizeof(struct sockaddr_in);
    struct recv_tcp_args args = {
        .buf = NULL,
        .len = 0,
        .src_addr = (struct sockaddr *)&newsock->peer_addr,
        .addrlen = &adlen,
        .syn = false,
        .sock = newsock,
    };
    struct waitset ws;
    waitset_init(&ws);

    errval_t err = waitset_chan_register_polled(&ws, &recv_chanstate,
                                                MKCLOSURE(sock_recved_tcp_packet, &args));
    assert(err_is_ok(err));

    /* if socket is ready, trigger event right away */
    if (lwip_sock_ready_read(s)) {
        err = waitset_chan_trigger(&recv_chanstate);
        assert(err_is_ok(err));
    }

    if(sock->nonblocking) {
        err = event_dispatch_non_block(&ws);
        if(err_no(err) == LIB_ERR_NO_EVENT) {   // Deregister if it didn't fire
            err = waitset_chan_deregister(&recv_chanstate);
            assert(err_is_ok(err));
        }

        if(err_no(err) == LIB_ERR_NO_EVENT || !args.syn) {
            free_socket(newsock);
            errno = EAGAIN;
            return -1;
        } else {
            assert(err_is_ok(err));
        }

        if(!args.syn) {
            free_socket(newsock);
            errno = EAGAIN;
            return -1;
        }
    } else {
        err = event_dispatch(&ws);
        assert(err_is_ok(err));

        if(!args.syn) {
            assert(!"Will block forever");
        }
    }

    assert(adlen == sizeof(struct sockaddr_in));
    assert(*addrlen >= sizeof(struct sockaddr_in));
    // Set caller's addr buffers
    if(addr != NULL) {
        memcpy(addr, &newsock->peer_addr, sizeof(struct sockaddr_in));
        *addrlen = adlen;
    }

    /* newsock->my_seq = 0; */
    newsock->peer_seq = htonl(args.in_seqno);
    /* printf("lwip_accept: Assigning %p seq %x\n", newsock, newsock->my_seq); */

#ifdef SENDMSG_WITH_COPY
    // Get new TX packet and send SYN-ACK
    struct packet *newp = get_tx_packet();
    newp->len = sizeof(struct pkt_tcp_headers) + 4;
    newp->next = NULL;

    // Slap TCP/IP/Ethernet headers in front
    memcpy(newp->payload, &packet_tcp_header, sizeof(struct pkt_tcp_headers));

    // Fine-tune headers
    struct pkt_tcp_headers *p = (struct pkt_tcp_headers *)newp->payload;
    uint32_t *payload = (void *)p + sizeof(struct pkt_tcp_headers);
    memset(payload, 0, 4);
    p->ip.dest.addr = newsock->peer_addr.sin_addr.s_addr;
    p->tcp.dest = newsock->peer_addr.sin_port;
    struct peer *peer = peers_get_from_ip(p->ip.dest.addr);
    p->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port != 0);
    p->tcp.src = sock->bound_addr.sin_port;
    p->ip._len = htons(sizeof(struct tcp_hdr) + IP_HLEN + 4);
    p->tcp.seqno = htonl(++newsock->my_seq); newsock->my_seq++;
    /* printf("lwip_accept: Assigning %p seq %x\n", newsock, newsock->my_seq); */
    newsock->next_ack = newsock->peer_seq + 1;
    /* printf("lwip_accept: Assigning %p, %x\n", newsock, newsock->next_ack); */
    p->tcp.ackno = htonl(newsock->next_ack);
    /* printf("lwip_accept: Sending %p, %x\n", newsock, newsock->next_ack); */
    TCPH_FLAGS_SET(&p->tcp, TCP_SYN | TCP_ACK); // Set SYN-ACK
    TCPH_HDRLEN_SET(&p->tcp, 6);   // 24 / 4
    p->tcp.wnd = htons(11680);
    *payload = TCP_BUILD_MSS_OPTION(1460);
#ifdef CONFIG_QEMU_NETWORK
    p->ip._chksum = inet_chksum(&p->ip, IP_HLEN);
    p->tcp.chksum = 0;
    newp->payload = (uint8_t *)&p->tcp;
    newp->len -= (uint8_t *)&p->tcp - (uint8_t *)p;
    p->tcp.chksum = inet_chksum_pseudo(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                       IP_PROTO_TCP, TCP_HLEN + 4);
    newp->payload = (uint8_t *)p;
    newp->len = sizeof(struct pkt_tcp_headers) + 4;
    newp->flags = 0;
#else
    // Hardware IP/TCP header checksumming on
    p->ip._chksum = 0;
    p->tcp.chksum =
        (~inet_chksum_pseudo_partial(newp, (ip_addr_t *)&p->ip.src, (ip_addr_t *)&p->ip.dest,
                                     IP_PROTO_TCP, TCP_HLEN + 4, 0)) & 0xffff;
    newp->flags = (NETIF_TXFLAG_IPCHECKSUM | NETIF_TXFLAG_TCPCHECKSUM) |
        (6 << NETIF_TXFLAG_TCPHDRLEN_SHIFT);
#endif

    packet_output(newp);
#else
    assert(!"NYI");
#endif

    /* printf("Returned %d\n", newsock->fd); */
    newsock->connected = true;
    assert(newsock->prev == NULL && newsock->next == NULL);
    newsock->next = connections;
    if(connections != NULL) {
        assert(connections->prev == NULL);
        connections->prev = newsock;
    }
    newsock->prev = NULL;
    connections = newsock;

    /* printf("lwip_accept(%d) = %d\n", s, newsock->fd); */
    return newsock->fd;
}

void process_received_packet(struct driver_rx_buffer *buffer, size_t count,
                             uint64_t flags)
{
    struct packet *p = buffer->opaque;
    assert(p != NULL);
    assert(count == 1);
    p->len = buffer->len;

    /* printf("Got %p from driver\n", p); */

    assert(p >= rx_packets && p < &rx_packets[MAX_PACKETS]);

#ifdef DEBUG_LATENCIES
    rx_packets_available--;
    if(rx_packets_available < 10) {
        printf("Too many RX packets in flight!\n");
    }
#endif

    // Drop packets with invalid checksums
    if(flags & NETIF_RXFLAG_IPCHECKSUM) {
        if(!(flags & NETIF_RXFLAG_IPCHECKSUM_GOOD)) {
            goto out;
        }
    }

    if(flags & NETIF_RXFLAG_L4CHECKSUM) {
        if(!(flags & NETIF_RXFLAG_L4CHECKSUM_GOOD)) {
            goto out;
        }
    }

    struct eth_hdr *ethhdr = (struct eth_hdr *)p->payload;
    switch (htons(ethhdr->type)) {
    case ETHTYPE_ARP:
        {
            struct etharp_hdr *arphdr = (struct etharp_hdr *)(p->payload + SIZEOF_ETH_HDR);
            uint32_t dipaddr = (arphdr->dipaddr.addrw[1] << 16) | arphdr->dipaddr.addrw[0];

            /* printf("%d: ARP request, dip = %x\n", disp_get_core_id(), dipaddr); */

            if(htons(arphdr->opcode) == ARP_REQUEST &&
               dipaddr == arranet_myip) {
                // Send reply
                struct packet outp;
		// XXX: Static payload! Need to lock if multithreaded!
                static uint8_t payload[PACKET_SIZE];
                struct eth_hdr *myeth = (struct eth_hdr *)payload;
                struct etharp_hdr *myarp = (struct etharp_hdr *)(payload + SIZEOF_ETH_HDR);

                /* printf("%d: ARP request for us!\n", disp_get_core_id()); */

                // ETH header
                memcpy(&myeth->dest, &arphdr->shwaddr, ETHARP_HWADDR_LEN);
                memcpy(&myeth->src, arranet_mymac, ETHARP_HWADDR_LEN);
                myeth->type = htons(ETHTYPE_ARP);

                // ARP header
                myarp->hwtype = htons(1);
                myarp->proto = htons(ETHTYPE_IP);
                myarp->hwlen = 6;
                myarp->protolen = 4;
                myarp->opcode = htons(ARP_REPLY);
                memcpy(&myarp->shwaddr, arranet_mymac, ETHARP_HWADDR_LEN);
                memcpy(&myarp->sipaddr, &arphdr->dipaddr, sizeof(myarp->sipaddr));
                memcpy(&myarp->dhwaddr, &arphdr->shwaddr, ETHARP_HWADDR_LEN);
                memcpy(&myarp->dipaddr, &arphdr->sipaddr, sizeof(myarp->dipaddr));

                outp.payload = payload;
                outp.len = SIZEOF_ETHARP_PACKET;
                /* outp.len = p->len; */
                outp.next = NULL;
                outp.flags = 0;
                outp.opaque = NULL;

                packet_output(&outp);
		static int arp_count = 0;
		arp_count++;
		if(arp_count > 100) {
		  printf("High ARP count!\n");
		}
                while(!e1000n_queue_empty()) thread_yield();
            }
        }
        break;

    case ETHTYPE_IP:
        {
            struct ip_hdr *iphdr = (struct ip_hdr *)(p->payload + SIZEOF_ETH_HDR);

            /* printf("%d: Is an IP packet, type %x\n", disp_get_core_id(), IPH_PROTO(iphdr)); */

#ifdef DEBUG_LATENCIES
            if(IPH_PROTO(iphdr) == IP_PROTO_ICMP) {
                static uint64_t cache_misses = 0;
                uint64_t new_cache_misses = rdpmc(0);
                printf("Cache misses = %" PRIu64 "\n", new_cache_misses - cache_misses);
                cache_misses = new_cache_misses;

                printf("hash_option1 = %zd, hash_option2 = %zd, hash_option3 = %zd\n",
                       hash_option1, hash_option2, hash_option3);
                printf("hash_calls = %zd, hash_length = %zd\n",
                       hash_calls, hash_length);

                printf("output pipeline stalled = %zd\n", output_pipeline_stalled);
                output_pipeline_stalled = 0;

                printf("memcache packets received = %zd\n", memcache_packets_received);
                memcache_packets_received = 0;
                for(int i = 0; i < 65536; i++) {
                    if(port_cnt[i] != 0) {
                        printf("port %d = %zu\n", i, port_cnt[i]);
                        port_cnt[i] = 0;
                    }
                }

                printf("recv_transa = %zu, send_transa = %zu\n",
                       posix_recv_transactions, posix_send_transactions);
                printf("posix_recv_transactions:\n");
                for(int i = 0; i < posix_recv_transactions; i++) {
                    printf("%u us\n", posix_recv_time[i]);
                }
                printf("posix_send_transactions:\n");
                for(int i = 0; i < posix_send_transactions; i++) {
                    printf("%u us\n", posix_send_time[i]);
                }
                posix_recv_transactions = posix_send_transactions = 0;

                printf("lwip_send_transa = %zu\n", lwip_send_transactions);
                printf("lwip_send_transactions:\n");
                for(int i = 0; i < lwip_send_transactions; i++) {
                    printf("%u us\n", lwip_send_time[i]);
                }
                lwip_send_transactions = 0;

                for(int j = 0; j < 20; j++) {
                    printf("memcache_transa[%d] = %zu:\n", j, memcache_transactions[j]);
                    for(int i = 0; i < memcache_transactions[j]; i++) {
                        printf("%u us\n", memcache_times[j][i]);
                    }
                    memcache_transactions[j] = 0;
                }
            }
#endif

            // Has to be UDP or TCP
            if(IPH_PROTO(iphdr) != IP_PROTO_UDP && IPH_PROTO(iphdr) != IP_PROTO_TCP) {
                goto out;
            }

            // XXX: Filter for our IP
            if(iphdr->dest.addr != arranet_myip) {
                goto out;
            }

	    // Take raw IP packets if that's accepted and ignore the rest
	    if(arranet_raw_accepted) {
                // XXX: Accept only TCP for now
                if(IPH_PROTO(iphdr) == IP_PROTO_TCP) {
                    goto accept;
                } else {
                    goto out;
                }
	    }

            if(IPH_PROTO(iphdr) == IP_PROTO_UDP) {
                struct udp_hdr *udphdr = (struct udp_hdr *)(p->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
                /* uint8_t *payload = p->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr); */

                // Are we accepting UDP packets?
                if(!arranet_udp_accepted) {
                    goto out;
                }

                /* printf("Got UDP packet, dest IP %x, dest port %u\n", */
                /*        htonl(iphdr->dest.addr), htons(udphdr->dest)); */

                // XXX: Filter for UDP ports 1234, 11211, 11212
                // TODO: Done in hardware soon
                if(htons(udphdr->dest) != 1234 &&
                   htons(udphdr->dest) != 11211 &&
                   htons(udphdr->dest) != 11212) {
                    goto out;
                }

#ifdef DEBUG_LATENCIES
                {
                    memcache_packets_received++;
                    port_cnt[htons(udphdr->src)]++;
                    protocol_binary_request_no_extras *mypayload = (void *)p->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr) + UDP_HEADLEN;
                    mypayload->message.header.request.opaque = get_time();
                }
#endif
            }

            if(IPH_PROTO(iphdr) == IP_PROTO_TCP) {
                struct tcp_hdr *tcphdr = (struct tcp_hdr *)(p->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));

                // Are we accepting TCP packets?
                if(!arranet_tcp_accepted) {
                    goto out;
                }

                /* printf("Got TCP packet, dest IP %x, src IP %x, dest port %u, src %u\n", */
                /*        htonl(iphdr->dest.addr), htonl(iphdr->src.addr), */
                /*        htons(tcphdr->dest), htons(tcphdr->src)); */

                // XXX: Filter for TCP port 8080 and everything that we know
                // TODO: Done in hardware soon
                struct socket *sock = NULL;
                for(sock = connections; sock != NULL; sock = sock->next) {
                    if(sock->bound_addr.sin_port == tcphdr->dest &&
                       sock->peer_addr.sin_port == tcphdr->src &&
                       sock->peer_addr.sin_addr.s_addr == iphdr->src.addr) {
                        break;
                    }
                }

                p->sock = sock;

                // Handle SYN-ACKs for connections we created and FIN-ACKs
                // Also ACK any data packet that came in
                uint16_t pkt_len = htons(IPH_LEN(iphdr)) - (TCPH_HDRLEN(tcphdr) * 4) - (IPH_HL(iphdr) * 4);
                if((TCPH_FLAGS(tcphdr) & TCP_ACK) &&
                   ((TCPH_FLAGS(tcphdr) & TCP_SYN) || (TCPH_FLAGS(tcphdr) & TCP_FIN) || pkt_len > 0)) {
                    bool is_retransmit = false;

                    if(TCPH_FLAGS(tcphdr) & TCP_SYN) {
                        assert(sock != NULL);
                        sock->connected = true;
                    }
                    if((TCPH_FLAGS(tcphdr) & TCP_FIN) && sock != NULL) {
                        // It said FIN, so we're not expecting any more from that side
                        sock->connected = false;
                        sock->hangup = true;
                        // Signal application
                        if (waitset_chan_is_registered(&recv_chanstate)) {
                            errval_t err = waitset_chan_trigger(&recv_chanstate);
                            assert(err_is_ok(err));
                        }

                        if(sock->prev != NULL) {
                            sock->prev->next = sock->next;
                        }
                        if(sock->next != NULL) {
                            sock->next->prev = sock->prev;
                        }
                        if(connections == sock) {
                            connections = sock->next;
                        }
                        sock->next = sock->prev = NULL;
                    }

                    if(sock != NULL) {
                        uint32_t new_peer_seq = htonl(tcphdr->seqno);
                        if(new_peer_seq == sock->peer_seq &&
                           new_peer_seq + pkt_len == sock->next_ack) {
                            is_retransmit = true;
                            /* printf("Is a retransmit! dst = %u, src = %u, seq = %u, ack = %u\n", */
                            /*        htons(tcphdr->dest), htons(tcphdr->src), */
                            /*        htonl(tcphdr->seqno), htonl(tcphdr->ackno)); */
                        }
                        sock->peer_seq = new_peer_seq;
                        sock->next_ack = sock->peer_seq + pkt_len;
                        if((TCPH_FLAGS(tcphdr) & TCP_SYN) || (TCPH_FLAGS(tcphdr) & TCP_FIN)) {
                            sock->next_ack++;
                        }
                        /* printf("process_received_packet: Assigning %p, %x\n", sock, sock->next_ack); */
                    }

                    // Get new TX packet and send ACK
                    struct packet *newp = get_tx_packet();
                    newp->len = sizeof(struct pkt_tcp_headers);
                    newp->next = NULL;

                    // Slap TCP/IP/Ethernet headers in front
                    memcpy(newp->payload, &packet_tcp_header, sizeof(struct pkt_tcp_headers));

                    // Fine-tune headers
                    struct pkt_tcp_headers *ph = (struct pkt_tcp_headers *)newp->payload;
                    ph->ip.dest.addr = iphdr->src.addr;
                    ph->tcp.dest = tcphdr->src;
                    ph->eth.dest = ethhdr->src;
                    ph->tcp.src = tcphdr->dest;
                    ph->ip._len = htons(sizeof(struct tcp_hdr) + IP_HLEN);
                    if(sock != NULL) {
                        ph->tcp.seqno = htonl(sock->my_seq);
                        ph->tcp.ackno = htonl(sock->next_ack);
                        /* printf("process_received_packet: Sending %p, seq %x, ack %x\n", sock, sock->my_seq, sock->next_ack); */
                    } else {
                        ph->tcp.seqno = tcphdr->ackno;
                        ph->tcp.ackno = htonl(htonl(tcphdr->seqno) + pkt_len + 1);
                    }
                    TCPH_FLAGS_SET(&ph->tcp, TCP_ACK);
                    TCPH_HDRLEN_SET(&ph->tcp, 5);   // 20 / 4
                    ph->tcp.wnd = htons(11680);
#ifdef CONFIG_QEMU_NETWORK
                    ph->ip._chksum = inet_chksum(&ph->ip, IP_HLEN);
                    ph->tcp.chksum = 0;
                    void *oldpayload = newp->payload;
                    size_t oldlen = newp->len;
                    newp->payload = (uint8_t *)&ph->tcp;
                    newp->len -= (uint8_t *)&ph->tcp - (uint8_t *)oldpayload;
                    ph->tcp.chksum = inet_chksum_pseudo(newp, (ip_addr_t *)&ph->ip.src, (ip_addr_t *)&ph->ip.dest,
                                                       IP_PROTO_TCP, TCP_HLEN);
                    newp->payload = oldpayload;
                    newp->len = oldlen;
                    newp->flags = 0;
#else
                    // Hardware IP/TCP header checksumming on
                    ph->ip._chksum = 0;
                    ph->tcp.chksum =
                        (~inet_chksum_pseudo_partial(newp, (ip_addr_t *)&ph->ip.src, (ip_addr_t *)&ph->ip.dest,
                                                     IP_PROTO_TCP, TCP_HLEN, 0)) & 0xffff;
                    newp->flags = (NETIF_TXFLAG_IPCHECKSUM | NETIF_TXFLAG_TCPCHECKSUM) |
                        (TCPH_HDRLEN(&ph->tcp) << NETIF_TXFLAG_TCPHDRLEN_SHIFT);
#endif

                    packet_output(newp);

                    // Ignore retransmits -- we've already ACKed them again
                    if(is_retransmit) {
                        goto out;
                    }
                }

                if(sock == NULL) {
                    if(htons(tcphdr->dest) != 8080) {
                        goto out;
                    } else if(!(TCPH_FLAGS(tcphdr) & TCP_SYN)) {
                        /* size_t psize = htons(iphdr->_len) - (TCPH_HDRLEN(tcphdr) * 4); */
                        /* if(psize > IPH_HL(iphdr) * 4) { */
                        /*     printf("Dropping 8080 data packet! src port = %u, payload size = %zu\n", htons(tcphdr->src), psize); */
                        /* } */
                        goto out;
                    }
                }

                // Ignore stray ACKs, signaling connection establishments
                // This will also throw away empty FIN-ACKs
                if((TCPH_FLAGS(tcphdr) & TCP_ACK) &&
                   htons(iphdr->_len) - (TCPH_HDRLEN(tcphdr) * 4) == IPH_HL(iphdr) * 4) {
                    goto out;
                }

#ifdef DEBUG_LATENCIES
                {
                    memcache_packets_received++;
                    port_cnt[htons(tcphdr->src)]++;
                    /* protocol_binary_request_no_extras *mypayload = (void *)p->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr) + UDP_HEADLEN; */
                    /* mypayload->message.header.request.opaque = get_time(); */
                }
#endif
            }

	accept:
            // ARP management
            if(peers_get_from_ip(iphdr->src.addr) == NULL) {
                struct peer *newpeer = peers_get_next_free();
                assert(p != NULL);

                newpeer->ip = iphdr->src.addr;
                memcpy(&newpeer->mac.addr, &ethhdr->src.addr, ETHARP_HWADDR_LEN);
            }

            // Push packets up - signal channel
            assert(inpkt == NULL);
            inpkt = p;
            if (waitset_chan_is_registered(&recv_chanstate)) {
                errval_t err = waitset_chan_trigger(&recv_chanstate);
                assert(err_is_ok(err));
            }

            // Return here, packet is in flight to user-space
            return;
        }
        break;

    default:
        break;
    }

 out:
    {
        //now we have consumed the preregistered pbuf containing a received packet
        //which was processed in this function. Therefore we have to register a new
        //free buffer for receiving packets.
#ifdef DEBUG_LATENCIES
        rx_packets_available++;
#endif
        errval_t err = rx_register_buffer_fn_ptr(p->pa, p->payload, p);
        assert(err_is_ok(err));
    }
}

static arranet_tx_done_fn arranet_tx_done_callback = NULL;

void arranet_register_tx_done_callback(arranet_tx_done_fn callback)
{
    arranet_tx_done_callback = callback;
}

bool handle_tx_done(void *opaque)
{
    struct packet *p = opaque;
    if(p >= tx_packets && p < &tx_packets[MAX_PACKETS]) {
        /* printf("Packet from TX ring, marking available\n"); */
        // Mark packet as available, if coming from TX packet array
        p->len = 0;
        /* tx_packets_available++; */
#ifndef SENDMSG_WITH_COPY
    } else {
        if(opaque != NULL && arranet_tx_done_callback != NULL) {
            /* printf("Packet from app, handing up\n"); */
            arranet_tx_done_callback(opaque);
        /* } else { */
        /*     if(opaque == NULL) { */
        /*         printf("NULL packet\n"); */
        /*     } */
        }
#endif
    }

    return true;
}

/* allocate a single frame, mapping it into our vspace with given attributes */
static void *alloc_map_frame(vregion_flags_t attr, size_t size, struct capref *retcap)
{
    struct capref frame;
    errval_t r;

    r = frame_alloc(&frame, size, NULL);
    assert(err_is_ok(r));
    void *va;
    r = vspace_map_one_frame_attr(&va, size, frame, attr,
                                  NULL, NULL);
    if (err_is_fail(r)) {
        DEBUG_ERR(r, "vspace_map_one_frame failed");
        return NULL;
    }

    if (retcap != NULL) {
        *retcap = frame;
    }

    return va;
}

bool lwip_sock_is_open(int s)
{
    if(arranet_tcp_accepted) {
        struct socket *sock = &sockets[s];
        assert(sock != NULL);
        return !sock->hangup;
    } else {
        // XXX: Not supported on UDP yet...
        return true;
    }
}

/**
 * \brief Check if a read on the socket would not block.
 *
 * \param socket    Socket to check.
 * \return          Whether or not the socket is ready.
 */
bool lwip_sock_ready_read(int s)
{
    if(arranet_tcp_accepted) {
        /* printf("lwip_sock_ready_read(%d)\n", s); */
        if(inpkt != NULL) {
            struct socket *sock = &sockets[s];
            assert(sock != NULL);
            struct ip_hdr *iphdr = (struct ip_hdr *)(inpkt->payload + SIZEOF_ETH_HDR);
            assert(IPH_PROTO(iphdr) == IP_PROTO_TCP);
            struct tcp_hdr *tcphdr = (struct tcp_hdr *)(inpkt->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
            if(sock->passive) {
                return (TCPH_FLAGS(tcphdr) & TCP_SYN) ? true : false;
            } else {
                if(TCPH_FLAGS(tcphdr) & TCP_SYN) {
                    return false;
                }

                if(tcphdr->dest == sock->bound_addr.sin_port &&
                   tcphdr->src == sock->peer_addr.sin_port) {
                    return true;
                } else {
#if 0
                    // XXX: Remove when code works...
                    struct fdtab_entry e;
                    e.type = FDTAB_TYPE_LWIP_SOCKET;
                    e.fd = inpkt->sock->fd;
                    int rfd = fdtab_search(&e);
                    struct fdtab_entry *ne = fdtab_get_sane(rfd);
                    if(ne->epoll_fd == -1) {
                        printf("Sock: %d, Last: %p %p %p\n", inpkt->sock->fd,
                               ne->last[0], ne->last[1], ne->last[2]);

                        // Drop the packet
#ifdef DEBUG_LATENCIES
                        rx_packets_available++;
#endif
                        errval_t err = rx_register_buffer_fn_ptr(inpkt->pa, inpkt->payload, inpkt);
                        assert(err_is_ok(err));
                        inpkt = NULL;
                    }
                    /* assert(ne->epoll_fd != -1); */
#endif

                    return false;
                }
            }
        } else {
            return false;
        }
    } else {
        assert(arranet_udp_accepted || arranet_raw_accepted);
        return inpkt != NULL;
    }
}

/**
 * \brief Check if a write on the socket would not block.
 *
 * \param socket    Socket to check.
 * \return          Whether or not the socket is ready.
 */
bool lwip_sock_ready_write(int s)
{
    if(arranet_tcp_accepted) {
        struct socket *sock = &sockets[s];
        assert(sock != NULL);
        if(sock->connected) {
            // See if there's space in the queue
#ifdef SENDMSG_WITH_COPY
            /* if(tx_packets[tx_idx].len != 0) { */
            /*     return false; */
            /* } else { */
            // XXX: Just always return true, we will stall a little when calling get_tx_packet() if no output packet is available then.
                return true;
            /* } */
#else
            assert(!"NYI");
#endif
        } else {
            return false;
        }
        /* printf("lwip_sock_ready_write(%d)\n", s); */
    } else {
        assert(arranet_udp_accepted);

        return tx_packets[tx_idx].len == 0 ? true : false;
        // XXX: Can also return true when one buffer is available in queue
        // return e1000n_queue_empty();
    }
}

static void do_nothing(void *arg)
{
}

/**
 * \brief Deregister previously registered waitset on which an event is delivered
 *        when the socket is ready for reading.
 */
errval_t lwip_sock_waitset_deregister_read(int sock)
{
    return waitset_chan_deregister(&recv_chanstate);
}

/**
 * \brief Register a waitset on which an event is delivered when the socket is
 *        ready for reading.
 *
 * The event is triggered ONCE, when the socket becomes ready for reading. If
 * the socket is already ready, the event is triggered right away.
 *
 * \param socket    Socket
 * \param ws        Waitset
 */
errval_t lwip_sock_waitset_register_read(int sock, struct waitset *ws)
{
    errval_t err;

    assert(ws != NULL);

    if(waitset_chan_is_registered(&recv_chanstate) || recv_chanstate.state == CHAN_PENDING) {
        assert(recv_chanstate.waitset == ws);
        return SYS_ERR_OK;
    }

    waitset_chanstate_init(&recv_chanstate, CHANTYPE_LWIP_SOCKET);

    err = waitset_chan_register_polled(ws, &recv_chanstate,
                                       MKCLOSURE(do_nothing, NULL));
    if (err_is_fail(err)) {
        DEBUG_ERR(err, "Error register recv channel on waitset.");
        return err;
    }

    /* if socket is ready, trigger event right away */
    if (lwip_sock_ready_read(sock)) {
        err = waitset_chan_trigger(&recv_chanstate);
        if (err_is_fail(err)) {
            DEBUG_ERR(err, "Error trigger event on recv channel.");
            return err;
        }
    }

    return SYS_ERR_OK;
}

/**
 * \brief Deregister previously registered waitset on which an event is delivered
 *        when the socket is ready for writing.
 */
errval_t lwip_sock_waitset_deregister_write(int sock)
{
    return waitset_chan_deregister(&send_chanstate);
}

/**
 * \brief Register a waitset on which an event is delivered when the socket is
 *        ready for writing.
 *
 * The event is triggered ONCE, when the socket becomes ready for writing. If
 * the socket is already ready, the event is triggered right away.
 *
 * \param socket    Socket
 * \param ws        Waitset
 */
errval_t lwip_sock_waitset_register_write(int sock, struct waitset *ws)
{
    errval_t err;

    assert(ws != NULL);

    if(waitset_chan_is_registered(&send_chanstate) || send_chanstate.state == CHAN_PENDING) {
        assert(send_chanstate.waitset == ws);
        return SYS_ERR_OK;
    }

    waitset_chanstate_init(&send_chanstate, CHANTYPE_LWIP_SOCKET);

    err = waitset_chan_register_polled(ws, &send_chanstate,
                                       MKCLOSURE(do_nothing, NULL));
    if (err_is_fail(err)) {
        DEBUG_ERR(err, "Error register send channel on waitset.");
        return err;
    }

    /* if socket is ready, trigger event right away */
    if (lwip_sock_ready_write(sock)) {
        err = waitset_chan_trigger(&send_chanstate);
        if (err_is_fail(err)) {
            DEBUG_ERR(err, "Error trigger event on send channel.");
            return err;
        }
    }

    return SYS_ERR_OK;
}

void arranet_polling_loop_proxy(void);
void arranet_polling_loop_proxy(void)
{
    // Push packets up - signal channel
    if(inpkt != NULL) {
#if 0
        static struct packet *lastinpkt = NULL;
        static int count = 0;

        if(inpkt == lastinpkt) {
            count++;
            if(count > 1000) {
                struct ip_hdr *iphdr = (struct ip_hdr *)(inpkt->payload + SIZEOF_ETH_HDR);
                assert(IPH_PROTO(iphdr) == IP_PROTO_TCP);
                struct tcp_hdr *tcphdr = (struct tcp_hdr *)(inpkt->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
                size_t hdr_len = SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + (TCPH_HDRLEN(tcphdr) * 4);
                char *payload = (char *)inpkt->payload + hdr_len;
                uint16_t pkt_len = htons(IPH_LEN(iphdr)) - (TCPH_HDRLEN(tcphdr) * 4) - (IPH_HL(iphdr) * 4);

                printf("Packet in queue too long, dst = %u, src = %u\n",
                       htons(tcphdr->dest), htons(tcphdr->src));

                if(TCPH_FLAGS(tcphdr) & TCP_SYN) {
                    printf("SYN set\n");
                }

                if(TCPH_FLAGS(tcphdr) & TCP_ACK) {
                    printf("ACK set\n");
                }

                if(TCPH_FLAGS(tcphdr) & TCP_FIN) {
                    printf("FIN set\n");
                }

                if(TCPH_FLAGS(tcphdr) & TCP_RST) {
                    printf("RST set\n");
                }

                printf("Seq = %u, Ack = %u\n", tcphdr->seqno, tcphdr->ackno);

                if(pkt_len > 0) {
                    printf("payload = '%s'\n", payload);
                }

#ifdef DEBUG_LATENCIES
                rx_packets_available++;
#endif
                errval_t err = rx_register_buffer_fn_ptr(inpkt->pa, inpkt->payload, inpkt);
                assert(err_is_ok(err));
                inpkt = NULL;
            }
        } else {
            lastinpkt = inpkt;
            count = 0;
        }
#endif

        if (waitset_chan_is_registered(&recv_chanstate)) {
            errval_t err = waitset_chan_trigger(&recv_chanstate);
            assert(err_is_ok(err));
        }
    } else {
        arranet_polling_loop();
    }
}

static const char *eat_opts[] = {
    "function=", "interrupts=", "queue=", "msix=", "vf=", "device=", "bus=", "use_vtd=",
    NULL
};

/*
 * The lcore main. This is the main thread that does the work, reading from an
 * input port and writing to an output port.
 */
static __attribute__((noreturn)) void
lcore_main(void)
{
	uint8_t portid;
	unsigned nb_rx;
	struct rte_mbuf *m;

	/*
	 * Check that the port is on the same NUMA node as the polling thread
	 * for best performance.
	 */
	printf("\nCore %u Waiting for SYNC packets. [Ctrl+C to quit]\n",
			rte_lcore_id());

	/* Run until the application is quit or killed. */

	while (1) {
		/* Read packet from RX queues. */
		for (portid = 0; portid < ptp_enabled_port_nb; portid++) {

			portid = ptp_enabled_ports[portid];
			nb_rx = rte_eth_rx_burst(portid, 0, &m, 1);

			if (likely(nb_rx == 0))
				continue;

			if (m->ol_flags & PKT_RX_IEEE1588_PTP)
				parse_ptp_frames(portid, m);

			rte_pktmbuf_free(m);
		}
	}
}

static void sock_recved_udp_packet(void *arg)
{
    struct recv_udp_args *args = arg;
    assert(inpkt != NULL);
    assert(inpkt->next == NULL);

    // Process headers
    struct ip_hdr *iphdr = (struct ip_hdr *)(inpkt->payload + SIZEOF_ETH_HDR);

    assert(IPH_PROTO(iphdr) == IP_PROTO_UDP);

    struct udp_hdr *udphdr = (struct udp_hdr *)(inpkt->payload + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
    size_t hdr_len = SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr);
    uint8_t *payload = inpkt->payload + hdr_len;
    uint16_t pkt_len = htons(udphdr->len) - sizeof(struct udp_hdr);
    assert(args->buf != NULL);      // No accept() allowed

    // Fill in src_addr if provided
    if(args->src_addr != NULL) {
        struct sockaddr_in *addr = (struct sockaddr_in *)args->src_addr;

        assert(*args->addrlen >= sizeof(struct sockaddr_in));
        memset(addr, 0, sizeof(struct sockaddr_in));
        addr->sin_len = sizeof(struct sockaddr_in);
        addr->sin_family = AF_INET;
        addr->sin_port = udphdr->src;
        addr->sin_addr.s_addr = iphdr->src.addr;
        *args->addrlen = sizeof(struct sockaddr_in);
    }

    // It's a recvfrom!
    if(args->len != 0) {
#ifdef DEBUG_LATENCIES
    if(memcache_transactions[0] < POSIX_TRANSA) {
        protocol_binary_request_no_extras *mypayload = (void *)payload + UDP_HEADLEN;
        memcache_times[0][memcache_transactions[0]] = get_time() - mypayload->message.header.request.opaque;
        memcache_transactions[0]++;
    }
#endif

        args->recv_len = MIN(args->len, pkt_len);
        memcpy(args->buf, payload, args->recv_len);

#ifdef DEBUG_LATENCIES
    if(memcache_transactions[1] < POSIX_TRANSA) {
        protocol_binary_request_no_extras *mypayload = (void *)payload + UDP_HEADLEN;
        memcache_times[1][memcache_transactions[1]] = get_time() - mypayload->message.header.request.opaque;
        memcache_transactions[1]++;
    }
#endif

#ifdef DEBUG_LATENCIES
        rx_packets_available++;
#endif
        errval_t err = rx_register_buffer_fn_ptr(inpkt->pa, inpkt->payload, inpkt);
        assert(err_is_ok(err));
    } else {
        args->recv_len = pkt_len;
        *((void **)args->buf) = payload;
        *args->inpkt = inpkt;
    }

    // Input packet is consumed in stack
    inpkt = NULL;
}
/*
 * The lcore main. This is the main thread that does the work, reading from an
 * input port and writing to an output port.
 */
ssize_t
pop(int qd, struct Zeus::sgarray &sga)
{
	uint8_t portid;
	unsigned nb_rx;
	struct rte_mbuf *m;
	void* buf;

	/*
	 * Check that the port is on the same NUMA node as the polling thread
	 * for best performance.
	 */
	printf("\nCore %u Waiting for SYNC packets. [Ctrl+C to quit]\n",
			rte_lcore_id());

	/* Run until the application is quit or killed. */
    nb_rx = rte_eth_rx_burst(portid, 0, &m, 1);

    if (likely(nb_rx == 0)) {
        return 0;
    } else {
        assert(nb_rx == 1);
            
		// inpkt = m->buf_addr;
		uint8_t* packet= rte_pktmbuf_mtod(m, uint8_t*)
		// buf = malloc(m->data_len);

		// Process headers
		struct ip_hdr *iphdr = (struct ip_hdr *)(payload + SIZEOF_ETH_HDR);

		assert(IPH_PROTO(iphdr) == IP_PROTO_UDP);

		struct udp_hdr *udphdr = (struct udp_hdr *)(packet + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
		size_t hdr_len = SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr);
		uint8_t *payload = packet + hdr_len;
		uint16_t pkt_len = htons(udphdr->len) - sizeof(struct udp_hdr);

		buf = malloc(pkt_len);


		// It's a recvfrom!
		if(args->len != 0) {
			memcpy(buf, payload, pkt_len);

		}


		rte_pktmbuf_free(m);
	}
}


/*
 * Initializes a given port using global settings and with the RX buffers
 * coming from the mbuf_pool passed as a parameter.
 */
static inline int
port_init(uint8_t port, struct rte_mempool *mbuf_pool)
{
	struct rte_eth_dev_info dev_info;
	struct rte_eth_conf port_conf = port_conf_default;
	const uint16_t rx_rings = 1;
	const uint16_t tx_rings = 1;
	int retval;
	uint16_t q;

	if (port >= rte_eth_dev_count())
		return -1;

	/* Configure the Ethernet device. */
	retval = rte_eth_dev_configure(port, rx_rings, tx_rings, &port_conf);
	if (retval != 0)
		return retval;

	/* Allocate and set up 1 RX queue per Ethernet port. */
	for (q = 0; q < rx_rings; q++) {
		retval = rte_eth_rx_queue_setup(port, q, RX_RING_SIZE,
				rte_eth_dev_socket_id(port), NULL, mbuf_pool);

		if (retval < 0)
			return retval;
	}

	/* Allocate and set up 1 TX queue per Ethernet port. */
	for (q = 0; q < tx_rings; q++) {
		/* Setup txq_flags */
		struct rte_eth_txconf *txconf;

		rte_eth_dev_info_get(q, &dev_info);
		txconf = &dev_info.default_txconf;
		txconf->txq_flags = 0;

		retval = rte_eth_tx_queue_setup(port, q, TX_RING_SIZE,
				rte_eth_dev_socket_id(port), txconf);
		if (retval < 0)
			return retval;
	}

	/* Start the Ethernet port. */
	retval = rte_eth_dev_start(port);
	if (retval < 0)
		return retval;

	return 0;
}

void lwip_init(const sockaddr_in &addr)
{
    unsigned nb_ports;
	uint8_t portid;
    int ret = rte_eal_init(argc, argv);
    if (ret < 0)
        rte_exit(EXIT_FAILURE, "Error with EAL initialization\n");

    /* Check that there is an even number of ports to send/receive on. */
	nb_ports = rte_eth_dev_count_available();
    
    // Create pool of memory for ring buffers
    mbuf_pool = rte_pktmbuf_pool_create("MBUF_POOL",
                                        NUM_MBUFS * nb_ports,
                                        MBUF_CACHE_SIZE,
                                        0,
                                        RTE_MBUF_DEFAULT_BUF_SIZE,
                                        rte_socket_id());

    
    if (mbuf_pool == NULL)
		rte_exit(EXIT_FAILURE, "Cannot create mbuf pool\n");

	/* Initialize all ports. */
	for (portid = 0; portid < nb_ports; portid++) {
        if (port_init(portid, mbuf_pool) != 0) {
            rte_exit(EXIT_FAILURE,
					 "Cannot init port %"PRIu8 "\n",
					 portid);
        }
	}

    struct ether_addr mac_addr;
    ret = rte_eth_macaddr_get(port_id, &mac_addr);
    assert(ret == 0);
    mac = mac_addr.addr_bytes;
    arranet_myip = sin.sin_addr;
    
    /***** Initialize IP/Ethernet packet header template *****/
    {
        struct pkt_ip_headers *p = &packet_ip_header;

        // Initialize Ethernet header
        memcpy(&p->eth.src, mac, ETHARP_HWADDR_LEN);
        p->eth.type = htons(ETHTYPE_IP);

        // Initialize IP header
        p->ip._v_hl = 69;
        p->ip._tos = 0;
        p->ip._id = htons(3);
        p->ip._offset = 0;
        p->ip._ttl = 0xff;
        p->ip._proto = 0;
        p->ip._chksum = 0;
        p->ip.src.addr = arranet_myip;
    }

    /***** Initialize UDP/IP/Ethernet packet header template *****/
    {
        struct pkt_udp_headers *p = &packet_udp_header;

        // Initialize Ethernet header
        memcpy(&p->eth.src, mac, ETHARP_HWADDR_LEN);
        p->eth.type = htons(ETHTYPE_IP);

        // Initialize IP header
        p->ip._v_hl = 69;
        p->ip._tos = 0;
        p->ip._id = htons(3);
        p->ip._offset = 0;
        p->ip._ttl = 0xff;
        p->ip._proto = IP_PROTO_UDP;
        p->ip._chksum = 0;
        p->ip.src.addr = arranet_myip;

        // Initialize UDP header
        p->udp.chksum = 0;
    }

    /***** Initialize TCP/IP/Ethernet packet header template *****/
    {
        struct pkt_tcp_headers *p = &packet_tcp_header;

        // Initialize Ethernet header
        memcpy(&p->eth.src, mac, ETHARP_HWADDR_LEN);
        p->eth.type = htons(ETHTYPE_IP);

        // Initialize IP header
        p->ip._v_hl = 69;
        p->ip._tos = 0;
        p->ip._id = htons(3);
        p->ip._offset = 0;
        p->ip._ttl = 0xff;
        p->ip._proto = IP_PROTO_TCP;
        p->ip._chksum = 0;
        p->ip.src.addr = arranet_myip;

        // Initialize TCP header
        p->tcp.chksum = 0;
        p->tcp.wnd = 65535;
    }

    // Initialize queue of free TCP ports
    for(u16_t i = 0; i <= TCP_LOCAL_PORT_RANGE_END - TCP_LOCAL_PORT_RANGE_START; i++) {
        free_tcp_ports[i] = htons(TCP_LOCAL_PORT_RANGE_START + i);
    }

    // Initialize queue of free sockets
    for(int i = 0; i < MAX_FD; i++) {
        free_sockets_queue[i] = &sockets[i];
        sockets[i].fd = i;
    }

    /***** Eat driver-specific options *****/
    static char *new_argv[ARG_MAX];
    int new_argc = 0;
    for(int i = 0; i < *argc; i++) {
        int j;

        for(j = 0; eat_opts[j] != NULL; j++) {
            if(!strncmp((*argv)[i], eat_opts[j], strlen(eat_opts[j]) - 1)) {
                // Option matches -- delete!
                break;
            }
        }

        if(eat_opts[j] == NULL) {
            // Option doesn't match -- keep!
            new_argv[new_argc++] = (*argv)[i];
        }
    }

    if (rte_lcore_count() > 1)
		printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");

	/* Call lcore_main on the master core only. */
	lcore_main();

}
