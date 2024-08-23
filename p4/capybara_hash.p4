/* -*- P4_16 -*- */


control calc_hash(
    in    bit<32>       ip,
    in    bit<16>       port,
    out   bit<16>       hash)

    (CRCPolynomial<bit<32>>       poly)
{
    Hash<bit<16>>(HashAlgorithm_t.CUSTOM, poly) hash_algo;

    action do_hash() {
        hash = hash_algo.get({
                ip,
                port
            });
    }

    apply {
        do_hash();
    }
}

/* -*- P4_16 -*- */


// control calc_hash(
//     in      my_ingress_headers_t        hdr,
//     in      bit<1>       is_load,
//     out     index_t       hash1,
//     out     index_t       hash2)

//     (CRCPolynomial<bit<32>>       poly)
// {
//     Hash<index_t>(HashAlgorithm_t.CUSTOM, poly) hash_algo1;
//     Hash<index_t>(HashAlgorithm_t.CUSTOM, poly) hash_algo2;
//     Hash<index_t>(HashAlgorithm_t.CUSTOM, poly) hash_algo3;


//     action do_hash_load() {
//         hash1 = hash_algo1.get({
//                 hdr.tcp_migration_header.client_ip,
//                 hdr.tcp_migration_header.client_port
//             });
//         hash2 = hash1;
//     }
//     action do_hash_migration() {
//         hash1 = hash_algo2.get({
//                 hdr.ipv4.src_ip,
//                 hdr.tcp.src_port
//             });

//         // hash2 = hash_algo3.get({
//         //         hdr.ipv4.dst_ip,
//         //         hdr.tcp.dst_port
//         //     });
//     }

//     table tbl_action_selection {
//         key = {
//             is_load           : exact;
//         }
//         actions = {
//             do_hash_load;
//             do_hash_migration;
//             NoAction;
//         }
//         size = 8;
//         const entries = {
//             1 : do_hash_load();
//             0 : do_hash_migration();
//         }
//         const default_action = NoAction();
//     }

//     apply {
//         // do_hash_load();
//         tbl_action_selection.apply();
//     }
// }