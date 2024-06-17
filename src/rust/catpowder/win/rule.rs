// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::catpowder::win::{
    params::XdpRedirectParams,
    socket::XdpSocket,
};
use ::std::mem;

//======================================================================================================================
// Structures
//======================================================================================================================

#[repr(C)]
pub struct XdpRule(xdp_rs::XDP_RULE);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpRule {
    pub fn new(socket: &XdpSocket) -> Self {
        let redirect: XdpRedirectParams = XdpRedirectParams::new(socket);
        let rule: xdp_rs::XDP_RULE = unsafe {
            let mut rule: xdp_rs::XDP_RULE = std::mem::zeroed();
            rule.Match = xdp_rs::_XDP_MATCH_TYPE_XDP_MATCH_ALL;
            rule.Action = xdp_rs::_XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT;
            // TODO: Set pattern
            // Perform bitwise copy from redirect to rule.
            rule.__bindgen_anon_1 =
                mem::transmute_copy::<xdp_rs::XDP_REDIRECT_PARAMS, xdp_rs::_XDP_RULE__bindgen_ty_1>(redirect.as_ptr());

            rule
        };
        Self(rule)
    }

    pub fn as_ptr(&self) -> *const xdp_rs::XDP_RULE {
        &self.0
    }
}
