// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef CATNIP_H_IS_INCLUDED
#define CATNIP_H_IS_INCLUDED

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void * nip_engine_t;
typedef uint16_t nip_tcp_connection_handle_t;
typedef void * nip_future_t;

#define NIP_LINK_ADDRESS_BYTES 6

typedef enum nip_event_code {
    NIP_ICMPV4_ERROR = 0,
    NIP_TCP_BYTES_AVAILABLE = 1,
    NIP_TCP_CONNECTION_CLOSED = 2,
    NIP_INCOMING_TCP_CONNECTION = 3,
    NIP_TRANSMIT = 4,
    NIP_UDP_DATAGRAM_RECEIVED = 5,
} nip_event_code_t;

typedef struct nip_icmpv4_error {
    uint8_t *context_bytes;
    uintptr_t context_length;
    uint16_t next_hop_mtu;
    uint8_t type;
    uint8_t code;
} nip_icmpv4_error_t;

typedef struct nip_udp_datagram {
    uint8_t *payload_bytes;
    uintptr_t payload_length;
    uint32_t dest_ipv4_addr;
    uint32_t src_ipv4_addr;
    uint16_t dest_port;
    uint16_t src_port;
    uint8_t dest_link_addr[NIP_LINK_ADDRESS_BYTES];
    uint8_t src_link_addr[NIP_LINK_ADDRESS_BYTES];
} nip_udp_datagram_t;

int nip_advance_clock(void *engine);
int nip_drop_event(void *engine);
int nip_get_icmpv4_error_event(nip_icmpv4_error_t *error_out, nip_engine_t engine);
int nip_get_incoming_tcp_connection_event(nip_tcp_connection_handle_t *handle_out, nip_engine_t engine);
int nip_get_tcp_connection_closed_event(nip_tcp_connection_handle_t *handle_out, int *error_out, nip_engine_t engine);
int nip_get_transmit_event(const uint8_t **bytes_out, uintptr_t *length_out, nip_engine_t engine);
int nip_get_udp_datagram_event(nip_udp_datagram_t *udp_out, nip_engine_t engine);
int nip_new_engine(nip_engine_t *engine_out);
int nip_next_event(nip_event_code_t *event_code_out, nip_engine_t engine);
int nip_receive_datagram(nip_engine_t engine, void *bytes, uintptr_t length);
int nip_set_my_ipv4_addr(uint32_t ipv4_addr);
int nip_set_my_link_addr(uint8_t link_addr[6]);
int nip_start_logger();
int nip_tcp_connect(nip_future_t *future_out, nip_engine_t engine, uint32_t remote_addr, uint16_t remote_port);
int nip_tcp_connected(nip_tcp_connection_handle_t *handle_out, nip_future_t future);
int nip_tcp_get_local_endpoint(uint32_t *addr_out, uint16_t *port_out, nip_engine_t engine, nip_tcp_connection_handle_t handle);
int nip_tcp_get_remote_endpoint(uint32_t *addr_out, uint16_t *port_out, nip_engine_t engine, nip_tcp_connection_handle_t handle);
int nip_tcp_listen(nip_engine_t engine, uint16_t port);
int nip_tcp_peek(const uint8_t **bytes_out, uintptr_t *length_out, nip_engine_t engine, nip_tcp_connection_handle_t handle);
int nip_tcp_read(nip_engine_t engine, nip_tcp_connection_handle_t handle);
int nip_tcp_write(nip_engine_t engine, nip_tcp_connection_handle_t handle, void *bytes, size_t length);

#ifdef __cplusplus
}
#endif

#endif /* CATNIP_H_IS_INCLUDED */
