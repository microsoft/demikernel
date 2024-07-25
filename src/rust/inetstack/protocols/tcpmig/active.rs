// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    segment::{
        TcpMigHeader,
        TcpMigSegment,
        TcpMigDefragmenter,
        MigrationStage,
    },
    TcpmigReceiveStatus
};
use crate::{
    capy_profile, capy_profile_merge_previous, capy_time_log, 
    inetstack::protocols::{
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            peer::{state::TcpState},
            segment::TcpHeader, socket::SharedTcpSocket
        },
        tcpmig::{constants::{ORIGIN_PORT, TARGET_PORT}, segment::MAX_FRAGMENT_SIZE},
    }, 
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        SharedDemiRuntime,
    }, 
    QDesc,
};

use crate::{capy_log, capy_log_mig};

use ::std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct ActiveMigration<N: NetworkRuntime> {
    transport: N,

    local_ipv4_addr: Ipv4Addr,
    local_link_addr: MacAddress,
    remote_ipv4_addr: Ipv4Addr,
    remote_link_addr: MacAddress,
    self_udp_port: u16,
    dest_udp_port: u16,

    origin: SocketAddrV4,
    client: SocketAddrV4,

    last_sent_stage: MigrationStage,

    /// QDesc representing the connection, only on the origin side.
    socket: Option<SharedTcpSocket<N>>,

    recv_queue: Vec<(Ipv4Header, TcpHeader, DemiBuffer)>,

    defragmenter: TcpMigDefragmenter,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<N: NetworkRuntime> ActiveMigration<N> {
    pub fn new(
        transport: N,
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        remote_ipv4_addr: Ipv4Addr,
        remote_link_addr: MacAddress,
        self_udp_port: u16,
        dest_udp_port: u16,
        origin: SocketAddrV4,
        client: SocketAddrV4,
        socket: Option<SharedTcpSocket<N>>,
    ) -> Self {
        Self {
            transport,
            local_ipv4_addr,
            local_link_addr,
            remote_ipv4_addr,
            remote_link_addr,
            self_udp_port,
            dest_udp_port, 
            origin,
            client,
            last_sent_stage: MigrationStage::None,
            socket,
            recv_queue: Vec::new(),
            defragmenter: TcpMigDefragmenter::new(),
        }
    }

    pub fn initiate_migration(&mut self) {
        assert_eq!(self.last_sent_stage, MigrationStage::None);

        let tcpmig_hdr = TcpMigHeader::new(
            self.origin,
            self.client, 
            0, 
            MigrationStage::PrepareMigration, 
            ORIGIN_PORT, 
            TARGET_PORT
        );
        self.last_sent_stage = MigrationStage::PrepareMigration;
        capy_log_mig!("\n\n******* START MIGRATION *******\n[TX] PREPARE_MIG ({}, {})", self.origin, self.client);
        capy_time_log!("SEND_PREPARE_MIG,({})", self.client);
        self.send(tcpmig_hdr, DemiBuffer::new(0));
    }

    /// Sends a TCPMig segment from local to remote.
    fn send(
        &mut self,
        tcpmig_hdr: TcpMigHeader,
        buf: DemiBuffer,
    ) {
        debug!("TCPMig send {:?}", tcpmig_hdr);
        // eprintln!("TCPMig {:#?}\n sent to {:?}:{:?}", tcpmig_hdr, self.remote_ipv4_addr, self.remote_link_addr);
        
        // Layer 4 protocol field marked as UDP because DPDK only supports standard Layer 4 protocols.
        let ip_hdr = Ipv4Header::new(self.local_ipv4_addr, self.remote_ipv4_addr, IpProtocol::UDP);

        if buf.len() / MAX_FRAGMENT_SIZE > u16::MAX as usize {
            panic!("TcpState too large")
        }
        let segment = TcpMigSegment::new(
            Ethernet2Header::new(self.remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ip_hdr,
            tcpmig_hdr,
            buf,
        );
        for fragment in segment.fragments() {
            self.transport.transmit(Box::new(fragment));
        }
    }
    pub fn process_packet(&mut self, ipv4_hdr: &Ipv4Header, hdr: TcpMigHeader, buf: DemiBuffer) -> Result<TcpmigReceiveStatus, Fail> {
        #[inline]
        fn next_header(mut hdr: TcpMigHeader, next_stage: MigrationStage) -> TcpMigHeader {
            hdr.stage = next_stage;
            hdr.swap_src_dst_port();
            hdr
        }

        #[inline]
        fn empty_buffer() -> DemiBuffer {
            DemiBuffer::new(0)
        }

        match self.last_sent_stage {
            // Expect PREPARE_MIGRATION and send ACK.
            MigrationStage::None => {
                match hdr.stage {
                    MigrationStage::PrepareMigration => {
                        // capy_profile_merge_previous!("prepare_ack");

                        // Decide if migration should be accepted or not and send corresponding segment.
                        // if ctx.is_under_load {
                        //     let mut hdr = next_header(hdr, MigrationStage::Rejected);
                        //     self.last_sent_stage = MigrationStage::Rejected;
                        //     capy_log_mig!("[TX] MIG_REJECTED ({}, {})", hdr.origin, hdr.client);
                        //     capy_time_log!("SEND_MIG_REJECTED,({})", hdr.client);
                        //     self.send(hdr, empty_buffer());
                        //     return Ok(TcpmigReceiveStatus::SentReject);
                        // } else {
                        let mut hdr = next_header(hdr, MigrationStage::PrepareMigrationAck);
                        hdr.flag_load = true;
                        self.last_sent_stage = MigrationStage::PrepareMigrationAck;
                        capy_log_mig!("[TX] PREPARE_MIG_ACK ({}, {})", hdr.origin, hdr.client);
                        capy_time_log!("SEND_PREPARE_MIG_ACK,({})", hdr.client);
                        self.send(hdr, empty_buffer());
                        return Ok(TcpmigReceiveStatus::Ok);
                        // }
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION"))
                }
            },
            MigrationStage::PrepareMigration => {
                match hdr.stage {
                    MigrationStage::PrepareMigrationAck => {
                        capy_time_log!("RECV_PREPARE_MIG_ACK,({})", hdr.client);
                        // Change target address to actual target address.
                        /*  self.remote_ipv4_addr = ipv4_hdr.get_src_addr(); */
                        // Currently, we are running all backends on a single machine, 
                        // so we send the migration messages to the switch first, 
                        // and let the switch do the addressing to the backend.
                        // Later if backends are on different machines, we need to uncomment this line again.
                        self.dest_udp_port = hdr.get_source_udp_port();

                        capy_log_mig!("PREPARE_MIG_ACK => ({}, {}) is PREPARED", hdr.origin, hdr.client);
                        return Ok(TcpmigReceiveStatus::PrepareMigrationAcked(hdr.origin, hdr.client));
                    },
                    MigrationStage::Rejected => {
                        capy_time_log!("RECV_REJECTED,({})", hdr.client);
                        return Ok(TcpmigReceiveStatus::Rejected(hdr.origin, hdr.client));
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION_ACK or REJECTED"))
                }
            },
            
            MigrationStage::PrepareMigrationAck => {
                match hdr.stage {
                    MigrationStage::ConnectionState => {
                        // capy_profile!("migrate_ack");

                        // Handle fragmentation.
                        let (hdr, buf) = match self.defragmenter.defragment(hdr, buf) {
                            Some((hdr, buf)) => (hdr, buf),
                            None => {
                                capy_log_mig!("Receiving CONN_STATE fragments...");
                                return Ok(TcpmigReceiveStatus::Ok)
                            },
                        };
                        capy_time_log!("RECV_STATE,({})", self.client);
                        capy_log_mig!("RECV_STATE,({})", self.client);
                        let mut state = TcpState::deserialize(buf);
                        capy_log_mig!("TcpState: {:#?}", state.cb);
                        // Overwrite local address.
                        state.set_local(SocketAddrV4::new(self.local_ipv4_addr, self.self_udp_port));

                        // ACK CONNECTION_STATE.
                        let hdr = next_header(hdr, MigrationStage::ConnectionStateAck);
                        self.last_sent_stage = MigrationStage::ConnectionStateAck;
                        capy_log_mig!("[TX] CONN_STATE_ACK ({}, {}) to {}:{}", self.origin, self.client, self.remote_ipv4_addr, self.dest_udp_port);
                        // capy_time_log!("SEND_STATE_ACK,({})", self.client);
                        self.send(hdr, empty_buffer());

                        return Ok(TcpmigReceiveStatus::StateReceived(state));
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE"))
                }
            },

            // Expect ACK and send FIN.
            MigrationStage::ConnectionState => {
            },

            MigrationStage::ConnectionStateAck => {
                // TODO: Close active migration.
                capy_log_mig!("Received someting after sending CONN_STATE_ACK");
            },

            MigrationStage::Rejected => panic!("Target should not receive a packet after rejecting origin."),
        };
        Ok(TcpmigReceiveStatus::Ok)
    }
    pub fn send_connection_state(&mut self, state: TcpState) {
        // capy_time_log!("SERIALIZE_STATE,({})", self.client);
        assert_eq!(self.last_sent_stage, MigrationStage::PrepareMigration);

        capy_log_mig!("[TX] CONNECTION_STATE: ({}, {}) to {}:{}", self.origin, self.client, self.remote_ipv4_addr, self.dest_udp_port);
        
        let buf = state.serialize();
        let tcpmig_hdr = TcpMigHeader::new(
            self.origin,
            self.client, 
            0, 
            MigrationStage::ConnectionState, 
            self.self_udp_port, 
            self.dest_udp_port
        ); // PORT should be the sender of PREPARE_MIGRATION_ACK
        
        self.last_sent_stage = MigrationStage::ConnectionState;
        capy_time_log!("SEND_STATE,({})", self.client);
        self.send(tcpmig_hdr, buf);
    }

    pub fn buffer_packet(&mut self, ip_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        self.recv_queue.push((ip_hdr, tcp_hdr, buf));
    }

    pub fn take_buffered_packets(&mut self) -> Vec<(Ipv4Header, TcpHeader, DemiBuffer)> {
        std::mem::take(&mut self.recv_queue)
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================
