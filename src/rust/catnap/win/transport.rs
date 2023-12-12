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
            IoWaitMode,
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
        scheduler::Yielder,
        DemiRuntime,
        SharedDemiRuntime,
        SharedObject,
    },
};

//======================================================================================================================
// Types
//======================================================================================================================

pub type SocketDescriptor = Socket;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Structured configuration values loaded from `Config`.
pub(super) struct WinConfig {
    /// TCP keepalive parameters.
    keepalive_params: tcp_keepalive,

    /// Linger socket options.
    linger_time: Option<Duration>,
}

/// Underlying network transport.
pub struct CatnapTransport {
    /// Winsock runtime instance.
    winsock: WinsockRuntime,

    /// I/O completion port for overlapped I/O.
    iocp: IoCompletionPort,

    /// Configuration values.
    config: WinConfig,
}

/// A network transport built on top of Windows overlapped I/O.
#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedCatnapTransport {
    /// Create a new transport instance.
    pub fn new(config: &Config, mut runtime: SharedDemiRuntime) -> Self {
        let config: WinConfig = WinConfig {
            keepalive_params: config.tcp_keepalive().expect("failed to load TCP settings"),
            linger_time: config.linger_time().expect("failed to load linger settings"),
        };

        let me: Self = Self(SharedObject::new(CatnapTransport {
            winsock: WinsockRuntime::new().expect("failed to initialize WinSock"),
            iocp: IoCompletionPort::new().expect("failed to setup I/O completion port"),
            config,
        }));

        runtime
            .insert_background_coroutine(
                "catnap::transport::epoll",
                Box::pin({
                    let mut me: Self = me.clone();
                    async move { me.run_event_processor().await }
                }),
            )
            .expect("should be able to insert background coroutine");

        me
    }

    /// Run a coroutine which pulls the I/O completion port for events.
    async fn run_event_processor(&mut self) {
        let yielder: Yielder = Yielder::new();
        loop {
            if let Err(err) = self.0.iocp.process_events(IoWaitMode::PollOnly) {
                error!("Completion port error: {}", err);
            }

            match yielder.yield_once().await {
                Ok(()) => continue,
                Err(_) => break,
            }
        }
    }

    /// Create a new socket for the specified domain and type.
    pub fn socket(&mut self, domain: socket2::Domain, typ: socket2::Type) -> Result<Socket, Fail> {
        // Select protocol.
        let protocol: IPPROTO = match typ {
            socket2::Type::STREAM => IPPROTO_TCP,
            socket2::Type::DGRAM => IPPROTO_UDP,
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
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
    pub fn close(&mut self, socket: &Socket) -> Result<(), Fail> {
        socket.shutdown()
    }

    /// Asynchronously disconnect and shut down a socket.
    pub async fn async_close(&mut self, socket: &Socket, yielder: Yielder) -> Result<(), Fail> {
        match unsafe {
            self.0.iocp.do_io(
                &yielder,
                |overlapped: *mut OVERLAPPED| socket.start_disconnect(overlapped),
                |_| Err(Fail::new(libc::EFAULT, "cannot cancel a disconnect")),
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
    pub fn bind(&self, socket: &Socket, local: SocketAddr) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        socket.bind(local)
    }

    /// Listen on the specified socket.
    pub fn listen(&mut self, socket: &mut Socket, backlog: usize) -> Result<(), Fail> {
        socket.listen(backlog)
    }

    /// Accept a connection on the specified socket. The coroutine will not finish until a connection is successfully
    /// accepted or `yielder` is cancelled.
    pub async fn accept(&mut self, socket: &Socket, yielder: Yielder) -> Result<(Socket, SocketAddr), Fail> {
        let start = |accept_result: Pin<&mut AcceptState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
            socket.start_accept(accept_result, overlapped)
        };
        let cancel = |_: Pin<&mut AcceptState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
            socket.cancel_io(overlapped)
        };
        let me_finish: Self = self.clone();
        let finish = |accept_result: Pin<&mut AcceptState>,
                      result: OverlappedResult|
         -> Result<(Socket, SocketAddr, SocketAddr), Fail> {
            socket.finish_accept(accept_result, &me_finish.0.iocp, result)
        };

        let (socket, _local_addr, remote_addr) = unsafe {
            self.0
                .iocp
                .do_io_with(AcceptState::new(), &yielder, start, cancel, finish)
        }
        .await?;

        Ok((socket, remote_addr))
    }

    /// Connect a socket to a remote address.
    pub async fn connect(&mut self, socket: &Socket, remote: SocketAddr, yielder: Yielder) -> Result<(), Fail> {
        unsafe {
            self.0.iocp.do_io(
                &yielder,
                |overlapped: *mut OVERLAPPED| -> Result<(), Fail> { socket.start_connect(remote, overlapped) },
                |overlapped: *mut OVERLAPPED| -> Result<(), Fail> { socket.cancel_io(overlapped) },
                |result: OverlappedResult| -> Result<(), Fail> { socket.finish_connect(result) },
            )
        }
        .await
    }

    /// Pop data from the socket into `buf`. This method will return the remote address iff the socket is not connected.
    pub async fn pop(
        &mut self,
        socket: &Socket,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: Yielder,
    ) -> Result<Option<SocketAddr>, Fail> {
        unsafe {
            self.0.iocp.do_io_with(
                PopState::new(buf.clone()),
                &yielder,
                |pop_state: Pin<&mut PopState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                    socket.start_pop(pop_state, overlapped)
                },
                |_pop_state: Pin<&mut PopState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                    socket.cancel_io(overlapped)
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
    pub async fn push(
        &mut self,
        socket: &Socket,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        loop {
            let result: Result<usize, Fail> = unsafe {
                self.0.iocp.do_io_with(
                    buf.clone(),
                    &yielder,
                    |buffer: Pin<&mut DemiBuffer>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                        socket.start_push(buffer, addr, overlapped)
                    },
                    |_buffer: Pin<&mut DemiBuffer>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                        socket.cancel_io(overlapped)
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
}
