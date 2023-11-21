// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
mod error;
mod overlapped;
mod socket;
mod winsock;

//==============================================================================
// Imports
//==============================================================================

use std::{
    net::{
        SocketAddr,
        SocketAddrV4,
    },
    time::Duration,
};

use libc::ENOTSUP;
use windows::Win32::Networking::WinSock::{
    tcp_keepalive,
    IPPROTO,
    IPPROTO_TCP,
    IPPROTO_UDP,
};

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        scheduler::Yielder,
        SharedDemiRuntime,
        SharedObject,
    },
};

use self::{
    overlapped::IoCompletionPort,
    socket::Socket,
    winsock::WinsockRuntime,
};

//======================================================================================================================
// Types
//======================================================================================================================

pub type SocketFd = Socket;

//======================================================================================================================
// Structures
//======================================================================================================================

pub(super) struct WinConfig {
    keepalive_params: tcp_keepalive,
    linger_time: Option<Duration>,
}

/// Underlying network transport.
pub struct CatnapTransport {
    winsock: WinsockRuntime,
    iocp: IoCompletionPort,
    config: WinConfig,
}

#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedCatnapTransport {
    pub fn new(config: &Config, _runtime: SharedDemiRuntime) -> Self {
        let config: WinConfig = WinConfig {
            keepalive_params: config.tcp_keepalive().expect("failed to load TCP settings"),
            linger_time: config.linger_time().expect("failed to load linger settings"),
        };

        Self(SharedObject::new(CatnapTransport {
            winsock: WinsockRuntime::new().expect("failed to initialize WinSock"),
            iocp: IoCompletionPort::new().expect("failed to setup I/O completion port"),
            config,
        }))
    }

    pub async fn epoll(&mut self) {
        let yielder: Yielder = Yielder::new();
        loop {
            if let Err(err) = self.0.iocp.process_events() {
                error!("Completion port error: {}", err);
            }

            match yielder.yield_once().await {
                Ok(()) => continue,
                Err(_) => break,
            }
        }
    }

    pub fn socket(&mut self, domain: socket2::Domain, typ: socket2::Type) -> Result<Socket, Fail> {
        // Select protocol.
        let protocol: IPPROTO = match typ {
            socket2::Type::STREAM => IPPROTO_TCP,
            socket2::Type::DGRAM => IPPROTO_UDP,
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        self.0
            .winsock
            .socket(domain.into(), typ.into(), protocol.0, &self.0.config)
    }

    pub async fn accept(&mut self, socket: &mut Socket, yielder: Yielder) -> Result<(Socket, SocketAddrV4), Fail> {
        socket.accept(yielder, &mut self.0.iocp).await.and_then(
            |(socket, addr)| -> Result<(Socket, SocketAddrV4), Fail> {
                match addr {
                    SocketAddr::V4(addr) => Ok((socket, addr)),
                    _ => Err(Fail::new(libc::EAFNOSUPPORT, "bad address family on result of accept")),
                }
            },
        )
    }
}
