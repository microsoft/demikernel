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
    net::SocketAddr,
    pin::Pin,
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
    catnap::transport::{
        overlapped::{
            IoCompletionPort,
            OverlappedResult,
        },
        socket::{
            AcceptState,
            PopState,
            Socket,
        },
        winsock::WinsockRuntime,
    },
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::transport::NetworkTransport,
        poll_yield,
        DemiRuntime,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::FutureExt;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Structured configuration values loaded from `Config`.
pub(super) struct WinConfig {
    /// TCP keepalive parameters.
    keepalive_params: tcp_keepalive,

    /// Linger socket options.
    linger_time: Option<Duration>,

    /// Whether Nagle's algorithm is enabled or disabled.
    nagle: Option<bool>,
}

/// Underlying network transport.
pub struct CatnapTransport {
    /// Winsock runtime instance.
    winsock: WinsockRuntime,

    /// I/O completion port for overlapped I/O.
    iocp: IoCompletionPort,

    /// Configuration values.
    config: WinConfig,

    /// Shared Demikernel runtime.
    runtime: SharedDemiRuntime,
}

/// A network transport built on top of Windows overlapped I/O.
#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedCatnapTransport {
    /// Create a new transport instance.
    pub fn new(config: &Config, runtime: &mut SharedDemiRuntime) -> Self {
        let config: WinConfig = WinConfig {
            keepalive_params: config.tcp_keepalive().expect("failed to load TCP settings"),
            linger_time: config.linger_time().expect("failed to load linger settings"),
            nagle: config.nagle().expect("failed to load nagle's algorithm settings"),
        };

        let me: Self = Self(SharedObject::new(CatnapTransport {
            winsock: WinsockRuntime::new().expect("failed to initialize WinSock"),
            iocp: IoCompletionPort::new().expect("failed to setup I/O completion port"),
            config,
            runtime: runtime.clone(),
        }));

        runtime
            .insert_background_coroutine(
                "catnap::transport::epoll",
                Box::pin({
                    let mut me: Self = me.clone();
                    async move { me.run_event_processor().await }.fuse()
                }),
            )
            .expect("should be able to insert background coroutine");

        me
    }

    /// Run a coroutine which pulls the I/O completion port for events.
    async fn run_event_processor(&mut self) {
        loop {
            if let Err(err) = self.0.iocp.process_events() {
                error!("Completion port error: {}", err);
            }

            poll_yield().await;
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl NetworkTransport for SharedCatnapTransport {
    type SocketDescriptor = Socket;

    /// Create a new socket for the specified domain and type.
    fn socket(&mut self, domain: socket2::Domain, typ: socket2::Type) -> Result<Socket, Fail> {
        // Select protocol.
        let protocol: IPPROTO = match typ {
            socket2::Type::STREAM => IPPROTO_TCP,
            socket2::Type::DGRAM => IPPROTO_UDP,
            _ => {
                let cause: String = format!("socket type not supported: {}", libc::c_int::from(typ));
                error!("transport::socket(): {}", &cause);
                return Err(Fail::new(ENOTSUP, &cause));
            },
        };

        let me: &mut CatnapTransport = &mut self.0;

        // Create socket.
        let s: Socket = me
            .winsock
            .socket(domain.into(), typ.into(), protocol.0, &me.config, &me.iocp)?;
        Ok(s)
    }

    /// Synchronously shut down the specified socket.
    fn hard_close(&mut self, socket: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        socket.shutdown()
    }

    /// Asynchronously disconnect and shut down a socket.
    async fn close(&mut self, socket: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        match unsafe {
            self.0.iocp.do_io(
                |overlapped: *mut OVERLAPPED| socket.start_disconnect(overlapped),
                |result: OverlappedResult| socket.finish_disconnect(result),
            )
        }
        .await
        {
            Err(err) if err.errno == libc::ENOTCONN => socket.shutdown(),
            r => r,
        }
    }

    /// Bind a socket to a local address.
    fn bind(&mut self, socket: &mut Self::SocketDescriptor, local: SocketAddr) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        socket.bind(local)
    }

    /// Listen on the specified socket.
    fn listen(&mut self, socket: &mut Socket, backlog: usize) -> Result<(), Fail> {
        socket.listen(backlog)
    }

    /// Accept a connection on the specified socket. The coroutine will not finish until a connection is successfully
    /// accepted or `yielder` is cancelled.
    async fn accept(&mut self, socket: &mut Self::SocketDescriptor) -> Result<(Socket, SocketAddr), Fail> {
        let start = |accept_result: Pin<&mut AcceptState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
            socket.start_accept(accept_result, overlapped)
        };
        let me_finish: Self = self.clone();
        let finish = |accept_result: Pin<&mut AcceptState>,
                      result: OverlappedResult|
         -> Result<(Socket, SocketAddr, SocketAddr), Fail> {
            socket.finish_accept(accept_result, &me_finish.0.iocp, result)
        };

        let (socket, _local_addr, remote_addr) =
            unsafe { self.0.iocp.do_io_with(AcceptState::new(), start, finish) }.await?;

        Ok((socket, remote_addr))
    }

    /// Connect a socket to a remote address.
    async fn connect(&mut self, socket: &mut Self::SocketDescriptor, remote: SocketAddr) -> Result<(), Fail> {
        unsafe {
            self.0.iocp.do_io(
                |overlapped: *mut OVERLAPPED| -> Result<(), Fail> { socket.start_connect(remote, overlapped) },
                |result: OverlappedResult| -> Result<(), Fail> { socket.finish_connect(result) },
            )
        }
        .await
    }

    /// Pop data from the socket into `buf`. This method will return the remote address iff the socket is not connected.
    async fn pop(
        &mut self,
        socket: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        size: usize,
    ) -> Result<Option<SocketAddr>, Fail> {
        unsafe {
            self.0.iocp.do_io_with(
                PopState::new(buf.clone()),
                |pop_state: Pin<&mut PopState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                    socket.start_pop(pop_state, overlapped)
                },
                |pop_state: Pin<&mut PopState>,
                 result: OverlappedResult|
                 -> Result<(usize, Option<SocketAddr>), Fail> { socket.finish_pop(pop_state, result) },
            )
        }
        .await
        .and_then(|(nbytes, sockaddr)| {
            buf.trim(buf.len() - nbytes)?;
            if nbytes > 0 {
                trace!("data received ({:?}/{:?} bytes)", nbytes, size);
            } else {
                trace!("not data received");
            }
            Ok(sockaddr)
        })
    }

    /// Push `buf` to the remote endpoint. `addr` is used iff the socket is not connection-oriented. For message-
    /// oriented sockets which were previously `connect`ed, `addr` overrides the previously specified remote address.
    /// For connection-oriented sockets, `addr` is ignored.
    async fn push(
        &mut self,
        socket: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        loop {
            let result: Result<usize, Fail> = unsafe {
                self.0.iocp.do_io_with(
                    buf.clone(),
                    |buffer: Pin<&mut DemiBuffer>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                        socket.start_push(buffer, addr, overlapped)
                    },
                    |buffer: Pin<&mut DemiBuffer>, result: OverlappedResult| -> Result<usize, Fail> {
                        socket.finish_push(buffer, result)
                    },
                )
            }
            .await;

            match result {
                Ok(nbytes) => {
                    trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                    buf.adjust(nbytes)?;
                    if buf.is_empty() {
                        return Ok(());
                    }
                },

                Err(fail) => {
                    if !DemiRuntime::should_retry(fail.errno) {
                        let message: String = format!("push failed: {}", fail.cause);
                        return Err(Fail::new(fail.errno, message.as_str()));
                    }
                },
            }
        }
    }

    fn get_runtime(&self) -> &SharedDemiRuntime {
        &self.0.runtime
    }
}
