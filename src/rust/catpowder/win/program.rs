// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::mem;

use super::socket::XdpSocket;

pub struct XdpRule {
    rule: xdp_rs::XDP_RULE,
}

pub struct XdpRedirectParams {
    redirect: xdp_rs::XDP_REDIRECT_PARAMS,
}

impl XdpRedirectParams {
    pub fn new(socket: &XdpSocket) -> Self {
        let redirect: xdp_rs::XDP_REDIRECT_PARAMS = {
            let mut redirect: xdp_rs::_XDP_REDIRECT_PARAMS = unsafe { mem::zeroed() };
            redirect.TargetType = xdp_rs::_XDP_REDIRECT_TARGET_TYPE_XDP_REDIRECT_TARGET_TYPE_XSK;
            redirect.Target = socket.socket;
            redirect
        };
        Self { redirect }
    }

    pub fn as_ptr(&self) -> &xdp_rs::XDP_REDIRECT_PARAMS {
        &self.redirect
    }
}

impl XdpRule {
    pub fn new(socket: &XdpSocket) -> Self {
        let redirect: XdpRedirectParams = XdpRedirectParams::new(socket);
        let rule: xdp_rs::XDP_RULE = unsafe {
            let mut rule: xdp_rs::XDP_RULE = std::mem::zeroed();
            rule.Match = xdp_rs::_XDP_MATCH_TYPE_XDP_MATCH_ALL;
            rule.Action = xdp_rs::_XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT;
            // TODO: Set redirect.
            // TODO: Set pattern
            // Perform bitwise copu from redirect to rule.
            rule.__bindgen_anon_1 =
                mem::transmute_copy::<xdp_rs::XDP_REDIRECT_PARAMS, xdp_rs::_XDP_RULE__bindgen_ty_1>(redirect.as_ptr());

            rule
        };
        Self { rule }
    }

    pub fn as_ptr(&self) -> *const xdp_rs::XDP_RULE {
        &self.rule
    }
}
