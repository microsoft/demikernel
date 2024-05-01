// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! SO_* options for sockets. We do not support all options, but the ones that we do support are listed here.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::time::Duration;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A listing of the SO_* socket options.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy)]
pub enum SocketOption {
    SO_LINGER(Option<Duration>),
}

/// A structure to store the values of the SO_* socket options.
#[derive(Debug, Clone, Copy)]
pub struct TcpSocketOptions {
    linger: Option<Duration>,
}

impl TcpSocketOptions {
    pub fn get_linger(&self) -> Option<Duration> {
        self.linger
    }

    pub fn set_linger(&mut self, linger: Option<Duration>) {
        self.linger = linger;
    }
}

impl Default for TcpSocketOptions {
    fn default() -> Self {
        Self { linger: None }
    }
}
