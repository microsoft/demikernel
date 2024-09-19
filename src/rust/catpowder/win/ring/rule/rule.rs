// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        ring::rule::params::XdpRedirectParams,
        socket::XdpSocket,
    },
    runtime::libxdp,
};
use ::std::mem;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP rule.
#[repr(C)]
pub struct XdpRule(libxdp::XDP_RULE);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpRule {
    /// Creates a new XDP rule for the target socket.
    pub fn new(socket: &XdpSocket) -> Self {
        let redirect: XdpRedirectParams = XdpRedirectParams::new(socket);
        let rule: libxdp::XDP_RULE = unsafe {
            let mut rule: libxdp::XDP_RULE = std::mem::zeroed();
            rule.Match = libxdp::_XDP_MATCH_TYPE_XDP_MATCH_ALL;
            rule.Action = libxdp::_XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT;
            // Perform bitwise copy from redirect to rule, as this is a union field.
            rule.__bindgen_anon_1 =
                mem::transmute_copy::<libxdp::XDP_REDIRECT_PARAMS, libxdp::_XDP_RULE__bindgen_ty_1>(redirect.as_ref());

            rule
        };
        Self(rule)
    }
}
