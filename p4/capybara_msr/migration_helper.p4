#ifndef _MIGRATION_HELPER_
#define _MIGRATION_HELPER_


// Read/Write a header field for migartion
control MigrationRequestIdentifier32b(
    in index_t index,
    in my_ingress_headers_t hdr,
    in my_ingress_metadata_t meta,
    out bit<1> discriminator_out) {

    Register< value32b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value32b_t, index_t, bit<1> >(reg) write_client_ip = {
        void apply(inout value32b_t register_value, out bit<1> is_written) {
            if(register_value == 0){
                register_value = meta.client_ip;
                is_written = 1;
            }else{
                is_written = 0;
            }
        }
    };
    action exec_write_client_ip() {
        discriminator_out = write_client_ip.execute(index);
    }

    RegisterAction< value32b_t, index_t, bit<1> >(reg) match_client_ip = {
        void apply(inout value32b_t register_value, out bit<1> is_matched) {
            if(register_value == meta.client_ip){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_client_ip() {
        discriminator_out = match_client_ip.execute(index);
    }

    RegisterAction< value32b_t, index_t, bit<1> >(reg) match_src_ip = {
        void apply(inout value32b_t register_value, out bit<1> is_matched) {
            if(register_value == hdr.ipv4.src_ip){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_src_ip() {
        discriminator_out = match_src_ip.execute(index);
    }

    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
        }
        actions = {
            exec_write_client_ip;
            exec_match_client_ip;
            exec_match_src_ip;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, _) : exec_write_client_ip(); // TCP SYN
            (0, 0) : exec_match_src_ip(); // normal TCP pkts
            (0, _) : exec_match_client_ip(); // all TCPMig packets
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationRequestIdentifier16b(
    in index_t index,
    in my_ingress_headers_t hdr,
    in my_ingress_metadata_t meta,
    out bit<1> discriminator_out) {

    Register< value16b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value16b_t, index_t, bit<1> >(reg) write_client_port = {
        void apply(inout value16b_t register_value, out bit<1> is_written) {
            if(register_value == 0){
                register_value = meta.client_port;
                is_written = 1;
            }else{
                is_written = 0;
            }
        }
    };
    action exec_write_client_port() {
        discriminator_out = write_client_port.execute(index);
    }

    RegisterAction< value16b_t, index_t, bit<1> >(reg) match_client_port = {
        void apply(inout value16b_t register_value, out bit<1> is_matched) {
            if(register_value == meta.client_port){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_client_port() {
        discriminator_out = match_client_port.execute(index);
    }

    RegisterAction< value16b_t, index_t, bit<1> >(reg) match_src_port = {
        void apply(inout value16b_t register_value, out bit<1> is_matched) {
            if(register_value == hdr.tcp.src_port){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_src_port() {
        discriminator_out = match_src_port.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
        }
        actions = {
            exec_write_client_port;
            exec_match_client_port;
            exec_match_src_port;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, _) : exec_write_client_port(); // TCP SYN
            (0, 0) : exec_match_src_port(); // normal TCP pkts
            (0, _) : exec_match_client_port(); // all TCPMig packets
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationReplyIdentifier32b(
    in index_t index,
    in my_ingress_headers_t hdr,
    in my_ingress_metadata_t meta,
    out bit<1> discriminator_out) {

    Register< value32b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value32b_t, index_t, bit<1> >(reg) write_client_ip = {
        void apply(inout value32b_t register_value, out bit<1> is_written) {
            if(register_value == 0){
                register_value = meta.client_ip;
                is_written = 1;
            }else{
                is_written = 0;
            }
        }
    };
    action exec_write_client_ip() {
        discriminator_out = write_client_ip.execute(index);
    }

    RegisterAction< value32b_t, index_t, bit<1> >(reg) match_client_ip = {
        void apply(inout value32b_t register_value, out bit<1> is_matched) {
            if(register_value == meta.client_ip){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_client_ip() {
        discriminator_out = match_client_ip.execute(index);
    }

    RegisterAction< value32b_t, index_t, bit<1> >(reg) match_dst_ip = {
        void apply(inout value32b_t register_value, out bit<1> is_matched) {
            if(register_value == hdr.ipv4.dst_ip){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_dst_ip() {
        discriminator_out = match_dst_ip.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
        }
        actions = {
            exec_write_client_ip;
            exec_match_client_ip;
            exec_match_dst_ip;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, _) : exec_write_client_ip(); // TCP SYN
            (0, 0) : exec_match_dst_ip(); // normal TCP pkts
            (0, _) : exec_match_client_ip(); // all TCPMig packets
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationReplyIdentifier16b(
    in index_t index,
    in my_ingress_headers_t hdr,
    in my_ingress_metadata_t meta,
    out bit<1> discriminator_out) {

    Register< value16b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value16b_t, index_t, bit<1> >(reg) write_client_port = {
        void apply(inout value16b_t register_value, out bit<1> is_written) {
            if(register_value == 0){
                register_value = meta.client_port;
                is_written = 1;
            }else{
                is_written = 0;
            }
        }
    };
    action exec_write_client_port() {
        discriminator_out = write_client_port.execute(index);
    }

    RegisterAction< value16b_t, index_t, bit<1> >(reg) match_client_port = {
        void apply(inout value16b_t register_value, out bit<1> is_matched) {
            if(register_value == meta.client_port){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_client_port() {
        discriminator_out = match_client_port.execute(index);
    }

    RegisterAction< value16b_t, index_t, bit<1> >(reg) match_dst_port = {
        void apply(inout value16b_t register_value, out bit<1> is_matched) {
            if(register_value == hdr.tcp.dst_port){
                is_matched = 1;
            }else{
                is_matched = 0;
            }
        }
    };
    action exec_match_dst_port() {
        discriminator_out = match_dst_port.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
        }
        actions = {
            exec_write_client_port;
            exec_match_client_port;
            exec_match_dst_port;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, _) : exec_write_client_port(); // TCP SYN
            (0, 0) : exec_match_dst_port(); // normal TCP pkts
            (0, _) : exec_match_client_port(); // all TCPMig packets
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationRequest32b0(
    in index_t index,
    in value32b_t value,
    in my_ingress_metadata_t meta,
    out value32b_t return_value) {

    Register< value32b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value32b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value32b_t register_value, out bit<1> null) {
            register_value = value;
        }
    };
    action exec_write_value() {
        write_value.execute(index);
    }

    RegisterAction< value32b_t, index_t, value32b_t >(reg) read_value = {
        void apply(inout value32b_t register_value, out value32b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_value() {
        return_value = read_value.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
            meta.result00           : ternary;
            meta.result01           : ternary;
        }
        actions = {
            exec_read_value;
            exec_write_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, 0, 1, 1) : exec_write_value(); // TCP SYN
            (0, 0b00110001, 1, 1) : exec_write_value(); // PREPARE_MIG_ACK
            (0, 0, 1, 1) : exec_read_value(); // normal TCP pkts
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationRequest16b0(
    in index_t index,
    in value16b_t value,
    in my_ingress_metadata_t meta,
    out value16b_t return_value) {

    Register< value16b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value16b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value16b_t register_value, out bit<1> null) {
            register_value = value;
        }
    };
    action exec_write_value() {
        write_value.execute(index);
    }

    RegisterAction< value16b_t, index_t, value16b_t >(reg) read_value = {
        void apply(inout value16b_t register_value, out value16b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_value() {
        return_value = read_value.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
            meta.result00           : ternary;
            meta.result01           : ternary;
        }
        actions = {
            exec_read_value;
            exec_write_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, 0, 1, 1) : exec_write_value(); // TCP SYN
            (0, 0b00110001, 1, 1) : exec_write_value(); // PREPARE_MIG_ACK
            (0, 0, 1, 1) : exec_read_value(); // normal TCP pkts
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationRequest32b1(
    in index_t index,
    in value32b_t value,
    in my_ingress_metadata_t meta,
    out value32b_t return_value) {

    Register< value32b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value32b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value32b_t register_value, out bit<1> null) {
            register_value = value;
        }
    };
    action exec_write_value() {
        write_value.execute(index);
    }

    RegisterAction< value32b_t, index_t, value32b_t >(reg) read_value = {
        void apply(inout value32b_t register_value, out value32b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_value() {
        return_value = read_value.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
            meta.result10           : ternary;
            meta.result11           : ternary;
        }
        actions = {
            exec_read_value;
            exec_write_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, 0, 1, 1) : exec_write_value(); // TCP SYN
            (0, 0b00110001, 1, 1) : exec_write_value(); // PREPARE_MIG_ACK
            (0, 0, 1, 1) : exec_read_value(); // normal TCP pkts
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MigrationRequest16b1(
    in index_t index,
    in value16b_t value,
    in my_ingress_metadata_t meta,
    out value16b_t return_value) {

    Register< value16b_t, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< value16b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value16b_t register_value, out bit<1> null) {
            register_value = value;
        }
    };
    action exec_write_value() {
        write_value.execute(index);
    }

    RegisterAction< value16b_t, index_t, value16b_t >(reg) read_value = {
        void apply(inout value16b_t register_value, out value16b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_value() {
        return_value = read_value.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag          : ternary;
            meta.result10           : ternary;
            meta.result11           : ternary;
        }
        actions = {
            exec_read_value;
            exec_write_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (1, 0, 1, 1) : exec_write_value(); // TCP SYN
            (0, 0b00110001, 1, 1) : exec_write_value(); // PREPARE_MIG_ACK
            (0, 0, 1, 1) : exec_read_value(); // normal TCP pkts
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

/* FOR HEARTBEAT HANDLING */
control MinimumWorkload(
    in index_t index,
    in my_ingress_headers_t hdr,
    in my_ingress_metadata_t meta,
    out bit<1> discriminator_out) {

    Register< value32b_t, index_t >(1) reg;
    RegisterAction< value32b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value32b_t register_value, out bit<1> is_written) {
            if(register_value >= hdr.heartbeat.queue_len){
                register_value = hdr.heartbeat.queue_len;
                is_written = 1;
            }else{
                is_written = 0;
            }
        }
    };
    action exec_write_value() {
        discriminator_out = write_value.execute(index);
    }
    RegisterAction< value32b_t, index_t, bit<1> >(reg) init_value = {
        void apply(inout value32b_t register_value, out bit<1> is_written) {
            register_value = 9999999;
            is_written = 1;
        }
    };
    action exec_init_value() {
        discriminator_out = init_value.execute(index);
    }

    // RegisterAction< value32b_t, index_t, bit<1> >(reg) check_value = {
    //     void apply(inout value32b_t register_value, out bit<1> is_matched) {
    //         if(register_value == hdr.ipv4.src_ip){
    //             is_matched = 1;
    //         }else{
    //             is_matched = 0;
    //         }
    //     }
    // };
    // action exec_check_value() {
    //     discriminator_out = check_value.execute(index);
    // }


    table tbl_action_selection {
        key = {
            hdr.heartbeat.isValid() : exact;
        }
        actions = {
            exec_write_value;
            exec_init_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (true) : exec_write_value();
            (false): exec_init_value();
            // (0, _) : exec_check_value();
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}


control MinimumWorkload32b(
    in index_t index,
    in value32b_t value,
    in my_ingress_metadata_t meta,
    out value32b_t return_value) {

    Register< value32b_t, index_t >(1) reg;
    RegisterAction< value32b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value32b_t register_value, out bit<1> null) {
            register_value = value;
        }
    };
    action exec_write_value() {
        write_value.execute(index);
    }

    RegisterAction< value32b_t, index_t, value32b_t >(reg) read_value = {
        void apply(inout value32b_t register_value, out value32b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_value() {
        return_value = read_value.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution        : ternary;
            meta.result00               : ternary;
        }
        actions = {
            exec_read_value;
            exec_write_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (0, 1) : exec_write_value();
            (1, _) : exec_read_value();
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}

control MinimumWorkload16b(
    in index_t index,
    in value16b_t value,
    in my_ingress_metadata_t meta,
    out value16b_t return_value) {

    Register< value16b_t, index_t >(1) reg;
    RegisterAction< value16b_t, index_t, bit<1> >(reg) write_value = {
        void apply(inout value16b_t register_value, out bit<1> null) {
            register_value = value;
        }
    };
    action exec_write_value() {
        write_value.execute(index);
    }

    RegisterAction< value16b_t, index_t, value16b_t >(reg) read_value = {
        void apply(inout value16b_t register_value, out value16b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_value() {
        return_value = read_value.execute(index);
    }


    table tbl_action_selection {
        key = {
            meta.initial_distribution        : ternary;
            meta.result00               : ternary;
        }
        actions = {
            exec_read_value;
            exec_write_value;
            NoAction;
        }
        size = 16;
        const entries = {
            (0, 1) : exec_write_value();
            (1, _) : exec_read_value();
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}


#endif /* _MIGRATION_HELPER_ */

// ============================     BLOCKING     ============================== //
control Blocker0(
    in index_t index,
    in my_ingress_metadata_t meta,
    out bit<1> return_value) {

    Register< bit<1>, index_t >(TWO_POWER_SIXTEEN) reg;
    RegisterAction< bit<1>, index_t, bit<1> >(reg) block = {
        void apply(inout bit<1> register_value, out bit<1> null) {
            register_value = 1;
        }
    };
    action exec_block() {
        block.execute(index);
    }
    RegisterAction< bit<1>, index_t, bit<1> >(reg) unblock = {
        void apply(inout bit<1> register_value, out bit<1> null) {
            register_value = 0;
        }
    };
    action exec_unblock() {
        unblock.execute(index);
    }

    RegisterAction< bit<1>, index_t, bit<1> >(reg) check_block = {
        void apply(inout bit<1> register_value, out bit<1> is_blocked) {
            is_blocked = register_value;
        }
    };
    action exec_check_block() {
        return_value = check_block.execute(index);
    }

    table tbl_action_selection {
        key = {
            meta.flag           : ternary;
            meta.result00       : ternary;
            meta.result01       : ternary;
        }
        actions = {
            exec_block;
            exec_unblock;
            exec_check_block;
            NoAction;
        }
        size = 16;
        const entries = {
            (0b00100000, 1, 1) : exec_block();      // PREPARE_MIG
            (0b01010000, 1, 1) : exec_unblock();    // CONN_STATE_ACK
            (0, 1, 1)          : exec_check_block();
        }
        const default_action = NoAction();
    }

    apply {
        tbl_action_selection.apply();
    }
}