// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::rc::Rc;

use windows::Win32::Networking::WinSock::{
    closesocket, socket, WSACleanup, WSAIoctl, WSAStartup, AF_INET, INET_PORT_RANGE, INET_PORT_RESERVATION_INSTANCE,
    INVALID_SOCKET, IN_ADDR, IPPROTO_TCP, IPPROTO_UDP, SIO_ACQUIRE_PORT_RESERVATION, SOCKET, SOCK_DGRAM, SOCK_STREAM,
    WSADATA,
};

use crate::{
    catnap::transport::error::expect_last_wsa_error,
    catpowder::win::ring::RuleSet,
    demikernel::config::Config,
    inetstack::protocols::{layer4::ephemeral::EphemeralPorts, Protocol},
    runtime::fail::Fail,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// State to track port sharing state with the kernel for cohosting.
pub struct PortSharingState {
    /// The local IP address of the host, from the config.
    local_ip: IN_ADDR,
    /// All TCP ports to be redirected.
    tcp_ports: Vec<u16>,
    /// All UDP ports to be redirected.
    udp_ports: Vec<u16>,
    /// The Winsock socket used to reserve ephemeral ports with the kernel.
    reserved_socket: SOCKET,
    /// The list of reserved ephemeral ports.
    reserved_ports: Vec<u16>,
}

/// The state of cohosting.
pub enum CohostingMode {
    /// No cohosting mode is enabled.
    None,
    /// Port sharing mode is enabled.
    PortSharing(PortSharingState),
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl CohostingMode {
    /// Creates a new instance of `CohostingMode` based on the provided configuration.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        if config.xdp_cohost_mode()? == false {
            return Ok(CohostingMode::None);
        }

        let local_ip: IN_ADDR = IN_ADDR::from(config.local_ipv4_addr()?);

        let (mut tcp_ports, mut udp_ports) = config.xdp_cohost_ports()?;

        let reserved_protocol: Option<Protocol> = config.xdp_reserved_port_protocol()?;
        let reserved_port_count: Option<u16> = config.xdp_reserved_port_count()?;

        let (reserved_socket, reserved_ports): (SOCKET, Vec<u16>) =
            if reserved_protocol.is_some() && reserved_port_count.is_some() {
                let protocol: Protocol = reserved_protocol.unwrap();
                let port_count: u16 = reserved_port_count.unwrap();

                let mut data: WSADATA = WSADATA::default();
                if unsafe { WSAStartup(0x202u16, &mut data as *mut WSADATA) } != 0 {
                    return Err(expect_last_wsa_error());
                }

                trace!("reserving {} ports with protocol {:?}", port_count, protocol);

                let (socket, ports) = reserve_port_blocks(port_count, protocol).or_else(|f: Fail| {
                    let _ = unsafe { WSACleanup() };
                    Err(f)
                })?;

                match protocol {
                    Protocol::Tcp => tcp_ports.extend(ports.iter().cloned()),
                    Protocol::Udp => udp_ports.extend(ports.iter().cloned()),
                }

                (socket, ports)
            } else {
                trace!("reserved port options not set; no ports reserved");
                (INVALID_SOCKET, vec![])
            };

        trace!(
            "XDP cohost mode enabled. TCP ports: {:?}, UDP ports: {:?}",
            tcp_ports,
            udp_ports
        );

        Ok(CohostingMode::PortSharing(PortSharingState {
            local_ip,
            tcp_ports,
            udp_ports,
            reserved_socket,
            reserved_ports,
        }))
    }

    pub fn create_ruleset(&self) -> Rc<RuleSet> {
        match self {
            CohostingMode::None => return RuleSet::new_redirect_all(),
            CohostingMode::PortSharing(state) => {
                RuleSet::new_cohost(state.local_ip, state.tcp_ports.as_slice(), state.udp_ports.as_slice())
            },
        }
    }

    pub fn ephemeral_ports(&self) -> EphemeralPorts {
        match self {
            CohostingMode::None => return EphemeralPorts::default(),
            CohostingMode::PortSharing(state) => {
                if state.reserved_ports.is_empty() {
                    EphemeralPorts::default()
                } else {
                    EphemeralPorts::new(state.reserved_ports.as_slice()).unwrap()
                }
            },
        }
    }
}

fn reserve_port_blocks(port_count: u16, protocol: Protocol) -> Result<(SOCKET, Vec<u16>), Fail> {
    const MAX_HALVINGS: usize = 5;
    let mut ports: Vec<u16> = Vec::with_capacity(port_count as usize);

    let mut reservation_len: u16 = port_count;
    let mut halvings: usize = 0;

    let (sock_type, protocol) = match protocol {
        Protocol::Tcp => (SOCK_STREAM, IPPROTO_TCP.0),
        Protocol::Udp => (SOCK_DGRAM, IPPROTO_UDP.0),
    };

    let s: SOCKET = unsafe { socket(AF_INET.0.into(), sock_type, protocol) };
    if s == INVALID_SOCKET {
        return Err(expect_last_wsa_error());
    }

    while ports.len() < port_count as usize {
        trace!("reserve_port_blocks(): trying reservation length: {}", reservation_len);
        match reserve_ports(reservation_len, s) {
            Ok((start, count, _)) if count > 0 => {
                let end: u16 = start + (count - 1);
                trace!("reserve_port_blocks(): reserved ports: {}-{}", start, end);
                ports.extend(start..=end);
            },
            Ok(_) => {
                panic!("reserve_port_blocks(): reserved zero ports");
            },
            Err(e) => {
                halvings += 1;
                if halvings >= MAX_HALVINGS || reservation_len == 1 {
                    error!("reserve_port_blocks(): failed to reserve ports; giving up: {:?}", e);
                    let _ = unsafe { closesocket(s) };
                    return Err(e);
                } else {
                    trace!(
                        "reserve_port_blocks(): failed to reserve ports; halving reservation size: {:?}",
                        e
                    );
                    reservation_len /= 2;
                }
            },
        }
    }

    Ok((s, ports))
}

fn reserve_ports(port_count: u16, s: SOCKET) -> Result<(u16, u16, u64), Fail> {
    let port_range: INET_PORT_RANGE = INET_PORT_RANGE {
        StartPort: 0,
        NumberOfPorts: port_count,
    };

    let mut reservation: INET_PORT_RESERVATION_INSTANCE = INET_PORT_RESERVATION_INSTANCE::default();
    let mut bytes_out: u32 = 0;

    let result: i32 = unsafe {
        WSAIoctl(
            s,
            SIO_ACQUIRE_PORT_RESERVATION,
            Some(&port_range as *const INET_PORT_RANGE as *mut libc::c_void),
            std::mem::size_of::<INET_PORT_RANGE>() as u32,
            Some(&mut reservation as *mut INET_PORT_RESERVATION_INSTANCE as *mut libc::c_void),
            std::mem::size_of::<INET_PORT_RESERVATION_INSTANCE>() as u32,
            &mut bytes_out,
            None,
            None,
        )
    };

    if result != 0 {
        return Err(expect_last_wsa_error());
    }

    Ok((
        u16::from_be(reservation.Reservation.StartPort),
        reservation.Reservation.NumberOfPorts,
        reservation.Token.Token,
    ))
}

impl Drop for PortSharingState {
    fn drop(&mut self) {
        if self.reserved_socket != INVALID_SOCKET {
            let _ = unsafe { closesocket(self.reserved_socket) };
            let _ = unsafe { WSACleanup() };
        }
    }
}
