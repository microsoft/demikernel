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

/// A wrapper structure for a XDP redirect parameters.
#[repr(C)]
pub struct XdpRedirectParams(xdp_rs::XDP_REDIRECT_PARAMS);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpRedirectParams {
    /// Creates a new XDP redirect parameters for the target socket.
    pub fn new(socket: &XdpSocket) -> Self {
        let redirect: xdp_rs::XDP_REDIRECT_PARAMS = {
            let mut redirect: xdp_rs::_XDP_REDIRECT_PARAMS = unsafe { mem::zeroed() };
            redirect.TargetType = xdp_rs::_XDP_REDIRECT_TARGET_TYPE_XDP_REDIRECT_TARGET_TYPE_XSK;
            redirect.Target = socket.into_raw();
            redirect
        };
        Self(redirect)
    }

    /// Gets a reference to the underlying XDP redirect parameters.
    pub fn as_ref(&self) -> &xdp_rs::XDP_REDIRECT_PARAMS {
        &self.0
    }
}
