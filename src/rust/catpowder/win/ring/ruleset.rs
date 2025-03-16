// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use demikernel_xdp_bindings::{XDP_HOOK_ID, XDP_IP_PORT_SET, XDP_MATCH_PATTERN, XDP_MATCH_TYPE};
use windows::Win32::Networking::WinSock::IN_ADDR;

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::rule::{XdpProgram, XdpRule},
        socket::XdpSocket,
    },
    inetstack::protocols::Protocol,
    runtime::{fail::Fail, libxdp},
};
use ::std::mem;
use std::rc::Rc;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP rule.
struct MatchPattern {
    pub match_type: XDP_MATCH_TYPE,
    pub pattern: XDP_MATCH_PATTERN,
}

pub struct RuleSet {
    rules: Vec<MatchPattern>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl MatchPattern {
    /// Creates a new XDP rule for the target socket which filters all traffic.
    fn match_all() -> Self {
        Self {
            match_type: libxdp::_XDP_MATCH_TYPE_XDP_MATCH_ALL,
            pattern: unsafe { mem::zeroed() },
        }
    }

    fn match_port_set(protocol: Protocol, local_ip: IN_ADDR, mut port_set: Vec<u8>) -> Self {
        port_set.shrink_to_fit();
        assert!(port_set.capacity() == XdpRule::XDP_PORT_SET_BUFFER_SIZE);
        let match_type: XDP_MATCH_TYPE = match protocol {
            Protocol::Udp => libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV4_UDP_PORT_SET,
            Protocol::Tcp => libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV4_TCP_PORT_SET,
        };

        let mut pattern: XDP_MATCH_PATTERN = unsafe { mem::zeroed() };
        let ip_port_set: &mut XDP_IP_PORT_SET = unsafe { pattern.IpPortSet.as_mut() };
        ip_port_set.Address.Ipv4 = local_ip;
        ip_port_set.PortSet.PortSet = port_set.leak().as_ptr() as *const u8;

        Self { match_type, pattern }
    }

    /// Creates a new XDP rule for the target socket which filters for a specific (protocol, port) combination.
    fn match_dest(protocol: Protocol, port: u16) -> Self {
        let match_type: XDP_MATCH_TYPE = match protocol {
            Protocol::Udp => libxdp::_XDP_MATCH_TYPE_XDP_MATCH_UDP_DST,
            Protocol::Tcp => libxdp::_XDP_MATCH_TYPE_XDP_MATCH_TCP_DST,
        };

        let mut pattern: XDP_MATCH_PATTERN = unsafe { mem::zeroed() };
        *unsafe { pattern.Port.as_mut() } = port.to_be();

        Self { match_type, pattern }
    }
}

impl RuleSet {
    pub fn new_cohost(local_ip: IN_ADDR, tcp_ports: &[u16], udp_ports: &[u16]) -> Rc<Self> {
        let mut rules: Vec<MatchPattern> = Vec::new();
        if !tcp_ports.is_empty() {
            rules.extend(make_cohost_rules(local_ip, tcp_ports, Protocol::Tcp));
        }

        if !udp_ports.is_empty() {
            rules.extend(make_cohost_rules(local_ip, udp_ports, Protocol::Udp));
        }

        Rc::new(Self { rules })
    }

    pub fn new_redirect_all() -> Rc<Self> {
        let rules: Vec<MatchPattern> = vec![MatchPattern::match_all()];
        Rc::new(Self { rules })
    }

    pub fn reprogram(
        self: &Rc<Self>,
        api: &mut XdpApi,
        socket: &XdpSocket,
        ifindex: u32,
        queueid: u32,
    ) -> Result<XdpProgram, Fail> {
        const XDP_INSPECT_RX: XDP_HOOK_ID = XDP_HOOK_ID {
            Layer: libxdp::_XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: libxdp::_XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: libxdp::_XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };

        let rules: Vec<XdpRule> = Vec::from_iter(self.rules.iter().map(|rule: &MatchPattern| {
            XdpRule::new(socket, rule.match_type, unsafe { std::ptr::read(&rule.pattern) })
        }));

        trace!(
            "MatchPattern::reprogram(): interface {}, queue {}, rules: {:?}",
            ifindex,
            queueid,
            rules
        );

        let program: XdpProgram = XdpProgram::new(api, rules.as_slice(), ifindex, &XDP_INSPECT_RX, queueid, 0)?;
        debug!(
            "xdp program created for interface {}, queue {} with {} rules",
            ifindex,
            queueid,
            rules.len()
        );

        Ok(program)
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

fn make_cohost_rules(local_ip: IN_ADDR, ports: &[u16], protocol: Protocol) -> Vec<MatchPattern> {
    if ports.len() > 20 {
        trace!("using port sets for XDP rules");
        let mut bitmap: Vec<u8> = Vec::new();
        bitmap.resize(XdpRule::XDP_PORT_SET_BUFFER_SIZE, 0);
        for port in ports.iter() {
            let port: u16 = port.to_be();
            let idx: usize = port as usize / 8;
            let bit: u8 = 1 << (port as usize % 8);
            bitmap[idx] |= bit;
        }

        vec![MatchPattern::match_port_set(protocol, local_ip, bitmap)]
    } else {
        trace!("using per-port XDP rules");
        Vec::from_iter(ports.iter().map(|port: &u16| MatchPattern::match_dest(protocol, *port)))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for MatchPattern {
    fn drop(&mut self) {
        if self.match_type == libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV4_TCP_PORT_SET
            || self.match_type == libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV4_UDP_PORT_SET
            || self.match_type == libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV6_UDP_PORT_SET
            || self.match_type == libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV6_TCP_PORT_SET
            || self.match_type == libxdp::_XDP_MATCH_TYPE_XDP_MATCH_UDP_PORT_SET
        {
            mem::drop(unsafe {
                Vec::from_raw_parts(
                    self.pattern.IpPortSet.as_mut().PortSet.PortSet as *mut u8,
                    XdpRule::XDP_PORT_SET_BUFFER_SIZE,
                    XdpRule::XDP_PORT_SET_BUFFER_SIZE,
                )
            });
        }
    }
}
