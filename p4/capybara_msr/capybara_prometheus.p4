/* -*- P4_16 -*- */
// #include <core.p4>
// #if __TARGET_TOFINO__ == 3
// #include <t3na.p4>
// #elif __TARGET_TOFINO__ == 2
// #include <t2na.p4>
// #else
// #include <tna.p4>
// #endif
#include "../capybara_header.h"

/*************************************************************************
 ************* C O N S T A N T S    A N D   T Y P E S  *******************
**************************************************************************/

#define BIP_p40 0xc613c923 // 198.19.201.35
#define BIP_p41 0xc613c924 // 198.19.201.36
#define BIP_p42 0xc613c925 // 198.19.201.37

#define DIP_p40 0xc613c828 // 198.19.200.40
#define DIP_p41 0xc613c829 // 198.19.200.41
#define DIP_p42 0xc613c82a // 198.19.200.42

#define VIP 0xc613c922 // 198.19.201.34
#define SERVER_PORT 10000

/*************************************************************************
 ***********************  H E A D E R S  *********************************
 *************************************************************************/


header ipv4_options_h {
    varbit<320> data;
}


/*************************************************************************
 **************  I N G R E S S   P R O C E S S I N G   *******************
 *************************************************************************/

    /***********************  H E A D E R S  ************************/

struct my_ingress_headers_t {
    ethernet_h           ethernet;
    ipv4_h               ipv4;
    ipv4_options_h     ipv4_options;
    tcp_h                       tcp;
    udp_h                       udp;
    tcpmig_h                    tcpmig;
    heartbeat_h                 heartbeat;
}

/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_metadata_t {
    bit<48> src_mac;
    bit<48> dst_mac;
    bit<16> l4_payload_checksum;

    value32b_t client_ip;
    value16b_t client_port;
    value32b_t owner_ip;
    value16b_t owner_port;
    value32b_t src_ip;
    value16b_t src_port;
    
    
    bit<8> flag;
    bit<1> initial_distribution;
    bit<1> result00;
    bit<1> result01;
    bit<1> result02;
    bit<1> result03;
    bit<1> result10;
    bit<1> result11;
    bit<1> result12;
    bit<1> result13;
}

/***********************  P A R S E R  **************************/
parser IngressParser(packet_in        pkt,
    /* User */
    out my_ingress_headers_t          hdr,
    out my_ingress_metadata_t         meta,
    /* Intrinsic */
    out ingress_intrinsic_metadata_t  ig_intr_md)
{
    Checksum() tcp_checksum;

    /* This is a mandatory state, required by Tofino Architecture */
     state start {
        meta.l4_payload_checksum  = 0;
        meta.client_ip = 0;
        meta.client_port = 0;
        meta.owner_ip = 0;
        meta.owner_port = 0;
        meta.src_ip = 0;
        meta.src_port = 0;
        
        meta.flag = 0;
        meta.initial_distribution = 0;

        meta.result00 = 0;
        meta.result01 = 0;
        meta.result02 = 0;
        meta.result03 = 0;
        meta.result10 = 0;
        meta.result11 = 0;
        meta.result12 = 0;
        meta.result13 = 0;

        pkt.extract(ig_intr_md);
        pkt.advance(PORT_METADATA_SIZE);
	    pkt.extract(hdr.ethernet);
        
        meta.src_mac = hdr.ethernet.src_mac;
        meta.dst_mac = hdr.ethernet.dst_mac;

        transition select(hdr.ethernet.ether_type){
            0x0800: parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        pkt.extract(hdr.ipv4);

        meta.src_ip = hdr.ipv4.src_ip;

        tcp_checksum.subtract({
            hdr.ipv4.src_ip,
            hdr.ipv4.dst_ip,
            8w0, hdr.ipv4.protocol
        });

        transition select(hdr.ipv4.ihl) {
            0x5 : parse_ipv4_no_options;
            0x6 &&& 0xE : parse_ipv4_options;
            0x8 &&& 0x8 : parse_ipv4_options;
            /* 
             * Packets with other values of IHL are illegal and will be
             * dropped by the parser
             */
        }
    }

    state parse_ipv4_options {
        pkt.extract(
            hdr.ipv4_options,
            ((bit<32>)hdr.ipv4.ihl - 5) * 32);
        
        transition parse_ipv4_no_options;
    }

    state parse_ipv4_no_options {
        // parser_md.l4_lookup = pkt.lookahead<l4_lookup_t>();
        
        transition select(hdr.ipv4.protocol){
            IP_PROTOCOL_TCP: parse_tcp;
            IP_PROTOCOL_UDP: parse_udp;
            default: accept;
        }
    }

    state parse_tcp {
        pkt.extract(hdr.tcp);

        /* Calculate Payload _m */
        tcp_checksum.subtract({
            hdr.tcp.src_port,
            hdr.tcp.dst_port,
            hdr.tcp.seq_no,
            hdr.tcp.ack_no,
            hdr.tcp.data_offset, hdr.tcp.res, hdr.tcp.flags,
            hdr.tcp.window,
            hdr.tcp.checksum,
            hdr.tcp.urgent_ptr
        });

        meta.l4_payload_checksum = tcp_checksum.get();

        transition accept;
    }

    state parse_udp {
        pkt.extract(hdr.udp);
        meta.src_port = hdr.udp.src_port;
        transition select(pkt.lookahead<bit<32>>()) {
            MIGRATION_SIGNATURE: parse_tcpmig;
            default: accept;
        }
    }

    state parse_tcpmig {
        pkt.extract(hdr.tcpmig);
        meta.client_ip = hdr.tcpmig.client_ip;
        meta.client_port = hdr.tcpmig.client_port;
        meta.flag = hdr.tcpmig.flag;
        transition accept;
    }
}

/***************** M A T C H - A C T I O N  *********************/
#include "../capybara_hash.p4"
#include "migration_helper.p4"
control Ingress(
    /* User */
    inout my_ingress_headers_t                       hdr,
    inout my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_t               ig_intr_md,
    in    ingress_intrinsic_metadata_from_parser_t   ig_prsr_md,
    inout ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md,
    inout ingress_intrinsic_metadata_for_tm_t        ig_tm_md)
{

    action ip_rewriting(bit<32> srcip, bit<32> dstip) {
        hdr.ipv4.src_ip = srcip;
        hdr.ipv4.dst_ip = dstip;
    }

    // action broadcast() {
    //     ig_tm_md.mcast_grp_a = 1;
    //     ig_tm_md.level2_exclusion_id = ig_intr_md.ingress_port;
    // }

    table tbl_ip_rewriting {
        key = {
            hdr.ipv4.src_ip : exact;
            hdr.ipv4.dst_ip : exact;
        }
        actions = {
            ip_rewriting;
            @defaultonly NoAction();
        }
    }
    action exec_return_to_7050a() {
        hdr.ethernet.src_mac = meta.dst_mac;
        hdr.ethernet.dst_mac = meta.src_mac;
        ig_tm_md.ucast_egress_port = ig_intr_md.ingress_port;
    }


    calc_hash(CRCPolynomial<bit<32>>(
            coeff=32w0x04C11DB7, reversed=true, msb=false, extended=false,
            init=32w0xFFFFFFFF, xor=32w0xFFFFFFFF)) hash;
    
    MigrationRequestIdentifier32b() request_client_ip_0;
    MigrationRequestIdentifier16b() request_client_port_0;
    MigrationReplyIdentifier32b() reply_client_ip_0;
    MigrationReplyIdentifier16b() reply_client_port_0;

    MigrationRequest32b0() owner_ip_0;
    MigrationRequest16b0() owner_port_0;

    action exec_reply_rewrite() {
        // hdr.ethernet.src_mac = FE_MAC;
        hdr.ipv4.src_ip = VIP;
        hdr.tcp.src_port = SERVER_PORT;
    }
    table tbl_reply_rewrite {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag               : ternary;
            meta.result02           : ternary;
            meta.result03           : ternary;
        }
        actions = {
            exec_reply_rewrite;
            NoAction;
        }
        size = 16;
        const entries = {
            (0, 0, 1, 1) : exec_reply_rewrite();
        }
        const default_action = NoAction();
    }

    apply {
        if(hdr.ipv4.isValid()){
            tbl_ip_rewriting.apply();
            exec_return_to_7050a();


            bit<16> hash1;
            bit<16> hash2;
            bit<1> holder_1b_00;
            bit<1> holder_1b_01;
            bit<1> holder_1b_02;
            bit<1> holder_1b_03;
            // hash1 = 0;
            // hash2 = 0;
            if(hdr.tcp.isValid()){
                hash.apply(hdr.ipv4.src_ip, hdr.tcp.src_port, hash1);
                hash.apply(hdr.ipv4.dst_ip, hdr.tcp.dst_port, hash2);
                if(hdr.tcp.flags == 0b00000010 && hdr.ipv4.dst_ip == DIP_p41){ // SYN to p41
                    // ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;

                    meta.client_ip = hdr.ipv4.src_ip;
                    meta.client_port = hdr.tcp.src_port;

                    // meta.backend_idx = get_be_idx.execute(meta.client_port);
                    // exec_read_backend_mac_hi32();
                    // exec_read_backend_mac_lo16();
                    // exec_read_backend_ip();
                    // exec_read_backend_port();
                    meta.owner_ip = hdr.ipv4.dst_ip;
                    meta.owner_port = hdr.tcp.dst_port;
                    
                    hash2 = hash1;
                    meta.initial_distribution = 1; // initial migration from FE (switch) to a BE

                    // hdr.ethernet.dst_mac = meta.owner_mac;
                    // hdr.ipv4.dst_ip = meta.owner_ip;
                    // hdr.tcp.dst_port = meta.owner_port;
                }
            }
            else if(hdr.tcpmig.isValid()){
                hash.apply(meta.client_ip, meta.client_port, hash1);
                hash2 = hash1;
                hdr.udp.checksum = 0;
            }

            if(meta.flag[0:0] == 1){ // chown
                meta.owner_ip = meta.src_ip;
                meta.owner_port = meta.src_port;
            }
            request_client_ip_0.apply(hash1, hdr, meta, holder_1b_00);
            request_client_port_0.apply(hash1, hdr, meta, holder_1b_01);
            reply_client_ip_0.apply(hash2, hdr, meta, holder_1b_02);
            reply_client_port_0.apply(hash2, hdr, meta, holder_1b_03);
            
            meta.result00 = holder_1b_00;
            meta.result01 = holder_1b_01;
            meta.result02 = holder_1b_02;
            meta.result03 = holder_1b_03;

            owner_ip_0.apply(hash1, meta.owner_ip, meta, hdr.ipv4.dst_ip);
            owner_port_0.apply(hash1, meta.owner_port, meta, hdr.tcp.dst_port);

            // tbl_reply_rewrite.apply();
        }
    }
}

/*********************  D E P A R S E R  ************************/

control IngressDeparser(packet_out pkt,
    /* User */
    inout my_ingress_headers_t                       hdr,
    in    my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md)
{
    Checksum() ipv4_checksum; 
    Checksum()  tcp_checksum;

    apply {
        hdr.ipv4.hdr_checksum = ipv4_checksum.update({
            hdr.ipv4.version,
            hdr.ipv4.ihl,
            hdr.ipv4.diffserv,
            hdr.ipv4.total_len,
            hdr.ipv4.identification,
            hdr.ipv4.flags,
            hdr.ipv4.frag_offset,
            hdr.ipv4.ttl,
            hdr.ipv4.protocol,
            hdr.ipv4.src_ip,
            hdr.ipv4.dst_ip,
            hdr.ipv4_options.data
        });
        if (hdr.tcp.isValid()) {
            hdr.tcp.checksum = tcp_checksum.update({
                hdr.ipv4.src_ip,
                hdr.ipv4.dst_ip,
                8w0, hdr.ipv4.protocol,
                hdr.tcp.src_port,
                hdr.tcp.dst_port,
                hdr.tcp.seq_no,
                hdr.tcp.ack_no,
                hdr.tcp.data_offset, hdr.tcp.res, hdr.tcp.flags,
                hdr.tcp.window,
                hdr.tcp.urgent_ptr,
                /* Any headers past TCP */
                meta.l4_payload_checksum
            });
        }
        pkt.emit(hdr);
    }
}


/*************************************************************************
 ****************  E G R E S S   P R O C E S S I N G   *******************
 *************************************************************************/

    /***********************  H E A D E R S  ************************/

struct my_egress_headers_t {
}

    /********  G L O B A L   E G R E S S   M E T A D A T A  *********/

struct my_egress_metadata_t {
}

    /***********************  P A R S E R  **************************/

parser EgressParser(packet_in        pkt,
    /* User */
    out my_egress_headers_t          hdr,
    out my_egress_metadata_t         meta,
    /* Intrinsic */
    out egress_intrinsic_metadata_t  eg_intr_md)
{
    /* This is a mandatory state, required by Tofino Architecture */
    state start {
        pkt.extract(eg_intr_md);
        transition accept;
    }
}

    /***************** M A T C H - A C T I O N  *********************/

control Egress(
    /* User */
    inout my_egress_headers_t                          hdr,
    inout my_egress_metadata_t                         meta,
    /* Intrinsic */
    in    egress_intrinsic_metadata_t                  eg_intr_md,
    in    egress_intrinsic_metadata_from_parser_t      eg_prsr_md,
    inout egress_intrinsic_metadata_for_deparser_t     eg_dprsr_md,
    inout egress_intrinsic_metadata_for_output_port_t  eg_oport_md)
{
    apply {
    }
}

    /*********************  D E P A R S E R  ************************/

control EgressDeparser(packet_out pkt,
    /* User */
    inout my_egress_headers_t                       hdr,
    in    my_egress_metadata_t                      meta,
    /* Intrinsic */
    in    egress_intrinsic_metadata_for_deparser_t  eg_dprsr_md)
{
    apply {
        pkt.emit(hdr);
    }
}


/************ F I N A L   P A C K A G E ******************************/
Pipeline(
    IngressParser(),
    Ingress(),
    IngressDeparser(),
    EgressParser(),
    Egress(),
    EgressDeparser()
) pipe;

Switch(pipe) main;
