/* -*- P4_16 -*- */
#include <core.p4>
#if __TARGET_TOFINO__ == 3
#include <t3na.p4>
#elif __TARGET_TOFINO__ == 2
#include <t2na.p4>
#else
#include <tna.p4>
#endif

#include "includes/port_mirror.p4" // <== To Add

#define ETHERTYPE_TPID    0x8100
#define ETHERTYPE_IPV4  0x0800
#define ETHERTYPE_ARP   0x0806
#define ETHERTYPE_PKTGEN 16w0x7777
#define IP_PROTOCOL_UDP 0x11
#define IP_PROTOCOL_TCP 0x06
#define IP_PROTOCOL_TCPMIG 0xC0
#define UDP_DSTPORT_PKTGEN 7777
#define PIPE_0_RECIRC 68

#define IPV4_HOST_SIZE 1024

#define MIGRATION_SIGNATURE 0xCAFEDEAD
#define HEARTBEAT_SIGNATURE 0xCAFECAFE
#define RPS_SIGNAL_SIGNATURE 0xABCDABCD

// #define FE_MAC 0xb8cef62a2f95
// #define FE_IP 0x0a000101
#define FE_MAC 0x08c0ebb6e805
#define FE_IP 0x0a000108
#define FE_PORT 10000 

#define BE_MAC 0x08c0ebb6c5ad
#define BE_IP 0x0a000109


#define NUM_BACKENDS 4


const int MAC_TABLE_SIZE        = 65536;
const bit<3> L2_LEARN_DIGEST = 1;
const bit<3> TCP_MIGRATION_DIGEST = 2;
const int TWO_POWER_SIXTEEN = 1 << 16;


struct pair {
    bit<32>     first;
    bit<32>     second;
}

typedef bit<32> value32b_t;
typedef bit<16> value16b_t;
typedef bit<16> index_t;

/*************************************************************************
 ***********************  H E A D E R S  *********************************
 *************************************************************************/

/*  Define all the headers the program will recognize             */
/*  The actual sets of headers processed by each gress can differ */

/* Standard ethernet header */
header ethernet_h {
    bit<48>   dst_mac;
    bit<48>   src_mac;
    bit<16>   ether_type;
}

header remaining_ethernet_h {
    bit<48> src_mac;
    bit<16> ether_type;
}

header vlan_tag_h {
    bit<3>   pcp;
    bit<1>   cfi;
    bit<12>  vid;
    bit<16>  ether_type;
}

header arp_h {
    bit<16> htype;
    bit<16> ptype;
    bit<8> hlen;
    bit<8> plen;
    bit<16> oper;
    bit<48> sender_hw_addr;
    bit<32> sender_ip_addr;
    bit<48> target_hw_addr;
    bit<32> target_ip_addr;
}

header ipv4_h {
    bit<4>   version;
    bit<4>   ihl;
    bit<8>   diffserv;
    bit<16>  total_len;
    bit<16>  identification;
    bit<3>   flags;
    bit<13>  frag_offset;
    bit<8>   ttl;
    bit<8>   protocol;
    bit<16>  hdr_checksum;
    bit<32>  src_ip;
    bit<32>  dst_ip;
} // 20

header tcp_h {
    bit<16>  src_port;
    bit<16>  dst_port;
    bit<32>  seq_no;
    bit<32>  ack_no;
    bit<4>   data_offset;
    bit<4>   res;
    bit<8>   flags;
    bit<16>  window;
    bit<16>  checksum;
    bit<16>  urgent_ptr;
} // 20

header udp_h {
    bit<16>  src_port;
    bit<16>  dst_port;
    bit<16>  len;
    bit<16>  checksum;
}


header tcpmig_h {
    bit<32>  signature;

    bit<32>  origin_ip;
    bit<16>  origin_port;
    
    bit<32>  client_ip;
    bit<16>  client_port;

    // bit<16>  payload_len;
    bit<16>  frag_offset;
    
    bit<8>  flag;
    bit<8> unused;
    // bit<16> checksum;
}

header heartbeat_h {
    bit<32>  queue_len;
}


header rps_signal_h {
    bit<32>  signature;
    bit<32>  sum;
    bit<32>  individual;
}




// PRISM HEADERS

header prism_req_base_h {
    bit<8>  type; // 0: add, 1: delete, 2: chown, 3: lock, 4: unlock
    bit<16>  status;

    bit<32>  peer_addr; // peer: client
    bit<16>  peer_port;
}

header prism_add_req_h {
    bit<32>  virtual_addr; // virtual: frontend
    bit<16>  virtual_port;
    bit<32>  owner_addr; // owner: frontend
    bit<16>  owner_port;
    bit<48> owner_mac;

    bit<8> lock;
}


// header prism_delete_req_h {
//     bit<8>  type;
//     bit<16>  status;

//     bit<32>  peer_addr; // peer: client
//     bit<16>  peer_port;
// }


header prism_chown_req_h {
    bit<32>  owner_addr; // owner: backend http server
    bit<16>  owner_port;
    bit<48> owner_mac;

    bit<8> unlock;
}

