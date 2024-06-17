// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::catpowder::win::socket::XdpSocket;
use ::std::mem;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct XdpRedirectParams {
    redirect: xdp_rs::XDP_REDIRECT_PARAMS,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

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
