// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use demikernel_xdp_bindings::{
    XDP_IP_PORT_SET, XDP_MATCH_PATTERN, XDP_MATCH_TYPE, XDP_PORT_SET, XDP_REDIRECT_PARAMS, XDP_RULE,
    _XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT,
};

use crate::{
    catpowder::win::{ring::rule::params::XdpRedirectParams, socket::XdpSocket},
    runtime::libxdp,
};
use ::std::mem;
use std::{
    fmt::{Debug, Formatter},
    net::Ipv4Addr,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP rule.
#[repr(C)]
pub struct XdpRule(libxdp::XDP_RULE);

/// Helper structure to print a port set struct.
struct IpPortSetWrapper<'a>(&'a XDP_IP_PORT_SET);
struct RedirectWrapper<'a>(&'a XDP_REDIRECT_PARAMS);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpRule {
    pub const XDP_PORT_SET_BUFFER_SIZE: usize = (u16::MAX as usize + 1) / 8;

    /// Creates a new XDP rule for the target socket which filters all traffic.
    #[allow(dead_code)]
    pub fn new(socket: &XdpSocket, match_type: XDP_MATCH_TYPE, pattern: XDP_MATCH_PATTERN) -> Self {
        let redirect: XdpRedirectParams = XdpRedirectParams::new(socket);
        let rule: XDP_RULE = XDP_RULE {
            Match: match_type,
            Action: _XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT,
            __bindgen_anon_1: unsafe {
                mem::transmute_copy::<XDP_REDIRECT_PARAMS, libxdp::_XDP_RULE__bindgen_ty_1>(redirect.as_ref())
            },
            Pattern: pattern,
        };
        Self(rule)
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

fn emit_port_ranges<F: FnMut(u16, u16) -> ()>(port_set: &XDP_PORT_SET, mut cb: F) {
    let port_set: &[u8; XdpRule::XDP_PORT_SET_BUFFER_SIZE] =
        unsafe { (port_set.PortSet as *const [u8; XdpRule::XDP_PORT_SET_BUFFER_SIZE]).as_ref() }.unwrap();

    let test_bit = |idx: u16| -> bool {
        let byte: usize = idx as usize / 8;
        let mask: u8 = 1u8 << (idx as usize % 8);
        (port_set[byte] & mask) != 0
    };

    let mut i: usize = 0;
    while i < u16::MAX as usize {
        let loop_start: usize = i;
        while i < u16::MAX as usize && test_bit((i as u16).to_be()) {
            i += 1;
        }

        if i > loop_start {
            cb(loop_start as u16, (i - 1) as u16);
        }

        i += 1;
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<'a> Debug for IpPortSetWrapper<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut sb: String = String::new();
        emit_port_ranges(&self.0.PortSet, |start: u16, end: u16| {
            if !sb.is_empty() {
                sb.push_str(", ");
            }
            sb.push_str(format!("{}-{}", start, end).as_str());
        });

        f.debug_struct("XDP_IP_PORT_SET")
            .field("Address", &Ipv4Addr::from(unsafe { self.0.Address.Ipv4 }))
            .field("PortSet", &sb)
            .finish()
    }
}

impl<'a> Debug for RedirectWrapper<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XDP_REDIRECT_PARAMS")
            .field("TargetType", &self.0.TargetType)
            .field("Target", &self.0.Target)
            .finish()
    }
}

impl Debug for XdpRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("XdpRule");

        match self.0.Match {
            libxdp::_XDP_MATCH_TYPE_XDP_MATCH_ALL => {
                dbg.field("Match", &"XDP_MATCH_ALL");
                dbg.field("Pattern", &"()");
            },
            libxdp::_XDP_MATCH_TYPE_XDP_MATCH_UDP_DST => {
                dbg.field("Match", &"XDP_MATCH_UDP_DST");
                dbg.field("Pattern", unsafe { self.0.Pattern.Port.as_ref() });
            },
            libxdp::_XDP_MATCH_TYPE_XDP_MATCH_TCP_DST => {
                dbg.field("Match", &"XDP_MATCH_TCP_DST");
                dbg.field("Pattern", unsafe { self.0.Pattern.Port.as_ref() });
            },
            libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV4_UDP_PORT_SET => {
                dbg.field("Match", &"XDP_MATCH_IPV4_UDP_PORT_SET");
                let ip_port_set: &XDP_IP_PORT_SET = unsafe { self.0.Pattern.IpPortSet.as_ref() };
                dbg.field("Pattern", &IpPortSetWrapper(ip_port_set));
            },
            libxdp::_XDP_MATCH_TYPE_XDP_MATCH_IPV4_TCP_PORT_SET => {
                dbg.field("Match", &"XDP_MATCH_IPV4_TCP_PORT_SET");
                let ip_port_set: &XDP_IP_PORT_SET = unsafe { self.0.Pattern.IpPortSet.as_ref() };
                dbg.field("Pattern", &IpPortSetWrapper(ip_port_set));
            },
            _ => {
                dbg.field("Match", &"Unknown");
            },
        }

        match self.0.Action {
            libxdp::_XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT => {
                dbg.field("Action", &"XDP_PROGRAM_ACTION_REDIRECT");
                dbg.field(
                    "Redirect",
                    &RedirectWrapper(unsafe { self.0.__bindgen_anon_1.Redirect.as_ref() }),
                );
            },
            _ => {
                dbg.field("Action", &"Unknown");
            },
        }

        dbg.finish()
    }
}
