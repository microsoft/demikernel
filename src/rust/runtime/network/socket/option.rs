// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! SO_* options for sockets. We do not support all options, but the ones that we do support are listed here.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    pal::data_structures::KeepAlive,
    runtime::fail::Fail,
};
use ::std::time::Duration;
#[cfg(target_os = "windows")]
use ::windows::Win32::Networking::WinSock::tcp_keepalive;

//======================================================================================================================
// Constants
//======================================================================================================================

const DEFAULT_LINGER: Option<Duration> = None;
#[cfg(target_os = "linux")]
const DEFAULT_KEEP_ALIVE: bool = false;
#[cfg(target_os = "windows")]
const DEFAULT_KEEP_ALIVE: tcp_keepalive = tcp_keepalive {
    onoff: 0,
    keepalivetime: 7200000,
    keepaliveinterval: 1000,
};
const DEFAULT_NO_DELAY: bool = true;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A listing of the SO_* socket options.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy)]
pub enum SocketOption {
    Linger(Option<Duration>),
    KeepAlive(KeepAlive),
    NoDelay(bool),
}

/// A structure to store the values of the SO_* socket options.
#[derive(Debug, Clone, Copy)]
pub struct TcpSocketOptions {
    linger: Option<Duration>,
    keep_alive: KeepAlive,
    no_delay: bool,
}

impl TcpSocketOptions {
    pub fn new(config: &Config) -> Result<Self, Fail> {
        Ok(Self {
            linger: config.linger().unwrap_or(DEFAULT_LINGER),
            keep_alive: config.tcp_keepalive().unwrap_or(DEFAULT_KEEP_ALIVE),
            no_delay: config.no_delay().unwrap_or(DEFAULT_NO_DELAY),
        })
    }

    pub fn get_linger(&self) -> Option<Duration> {
        self.linger
    }

    pub fn set_linger(&mut self, linger: Option<Duration>) {
        self.linger = linger;
    }

    pub fn get_keepalive(&self) -> KeepAlive {
        self.keep_alive
    }

    pub fn set_keepalive(&mut self, keep_alive: KeepAlive) {
        self.keep_alive = keep_alive;
    }

    pub fn get_nodelay(&self) -> bool {
        self.no_delay
    }

    pub fn set_nodelay(&mut self, nodelay: bool) {
        self.no_delay = nodelay;
    }
}

impl Default for TcpSocketOptions {
    fn default() -> Self {
        Self {
            linger: DEFAULT_LINGER,
            keep_alive: DEFAULT_KEEP_ALIVE,
            no_delay: DEFAULT_NO_DELAY,
        }
    }
}
