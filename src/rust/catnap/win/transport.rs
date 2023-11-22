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
    pin::{
        pin,
        Pin,
    },
    time::Duration,
};

use libc::ENOTSUP;
use windows::Win32::{
    Networking::WinSock::{
        tcp_keepalive,
        IPPROTO,
        IPPROTO_TCP,
        IPPROTO_UDP,
    },
    System::IO::OVERLAPPED,
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
    socket::{
        AcceptResult,
        Socket,
    },
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

    pub fn bind(&self, socket: &Socket, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        socket.bind(local.into())
    }

    pub fn listen(&mut self, socket: &mut Socket, backlog: usize) -> Result<(), Fail> {
        socket.listen(backlog)
    }

    pub async fn accept(&mut self, socket: &Socket, yielder: Yielder) -> Result<(Socket, SocketAddrV4), Fail> {
        let start = |accept_result: Pin<&mut AcceptResult>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
            socket.start_accept(accept_result, overlapped)
        };
        let cancel = |_: Pin<&mut AcceptResult>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
            socket.cancel_io(overlapped)
        };
        let finish = |accept_result: Pin<&mut AcceptResult>,
                      overlapped: &OVERLAPPED,
                      completion_key: usize|
         -> Result<(Socket, SocketAddr, SocketAddr), Fail> {
            socket.finish_accept(accept_result, overlapped, completion_key)
        };

        let (socket, _local_addr, remote_addr) = unsafe {
            self.0
                .iocp
                .do_io_with(AcceptResult::new(), yielder, start, cancel, finish)
        }
        .await?;

        match remote_addr {
            SocketAddr::V4(addr) => Ok((socket, addr)),
            _ => Err(Fail::new(libc::EAFNOSUPPORT, "bad address family on result of accept")),
        }
    }

    pub async fn connect(&mut self, socket: &Socket, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        unsafe {
            self.0.iocp.do_io(
                yielder,
                |overlapped: *mut OVERLAPPED| -> Result<(), Fail> { socket.start_conect(remote.into(), overlapped) },
                |overlapped: *mut OVERLAPPED| -> Result<(), Fail> { socket.cancel_io(overlapped) },
                |overlapped: &OVERLAPPED, completion_key: usize| -> Result<(), Fail> {
                    socket.finish_connect(overlapped, completion_key)
                },
            )
        }
        .await
    }
}
