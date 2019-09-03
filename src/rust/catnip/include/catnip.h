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

typedef enum nip_event_code {
    NIP_ICMPV4_ERROR = 0,
    NIP_TCP_BYTES_AVAILABLE = 1,
    NIP_TCP_CONNECTION_CLOSED = 2,
    NIP_TCP_CONECTION_ESTABLISHED = 3,
    NIP_TRANSMIT = 4,
    NIP_UDP_DATAGRAM_RECEIVED = 5,
} nip_event_code_t;

typedef struct nip_icmpv4_error {
    uint8_t *context_bytes;
    size_t context_length;
    uint16_t next_hop_mtu;
    uint8_t type;
    uint8_t code;
} nip_icmpv4_error_t;

int nip_set_my_ipv4_addr(int32_t ipv4_addr);
int nip_set_my_link_addr(uint8_t link_addr[6]);
int nip_new_engine(nip_engine_t *engine_out);
int nip_receive_datagram(void *bytes, size_t length);
int nip_poll_event(nip_event_code_t *event_code_out, nip_engine_t engine);
int nip_drop_event(void *engine);
int nip_get_transmit_event(uint8_t **bytes_out, size_t *length_out, nip_engine_t engine);
int nip_get_icmpv4_error_event(nip_icmpv4_error_t *error_out, nip_engine_t engine);

#ifdef __cplusplus
}
#endif

#endif /* CATNIP_H_IS_INCLUDED */
