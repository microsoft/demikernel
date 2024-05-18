// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
    pin::Pin,
};

use windows::Win32::{
    Networking::WinSock::{
        WSAGetLastError,
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
            SocketOpState,
        },
        winsock::WinsockRuntime,
    },
    demikernel::config::Config,
    expect_ok,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            socket::option::{
                SocketOption,
                TcpSocketOptions,
            },
            transport::NetworkTransport,
        },
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

/// Underlying network transport.
pub struct CatnapTransport {
    /// Winsock runtime instance.
    winsock: WinsockRuntime,

    /// I/O completion port for overlapped I/O.
    iocp: IoCompletionPort<SocketOpState>,

    /// Tcp socket options.
    options: TcpSocketOptions,

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
    pub fn new(config: &Config, runtime: &mut SharedDemiRuntime) -> Result<Self, Fail> {
        let me: Self = Self(SharedObject::new(CatnapTransport {
            winsock: expect_ok!(WinsockRuntime::new(), "failed to initialize WinSock"),
            iocp: expect_ok!(IoCompletionPort::new(), "failed to setup I/O completion port"),
            options: TcpSocketOptions::new(config)?,
            runtime: runtime.clone(),
        }));

        runtime.insert_background_coroutine(
            "catnap::transport::epoll",
            Box::pin({
                let mut me: Self = me.clone();
                async move { me.run_event_processor().await }.fuse()
            }),
        )?;

        Ok(me)
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
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
        };

        let me: &mut CatnapTransport = &mut self.0;

        // Create socket.
        let s: Socket = me
            .winsock
            .socket(domain.into(), typ.into(), protocol.0, &me.options, &me.iocp)?;
        Ok(s)
    }

    /// Set an SO_* option on the socket.
    fn set_socket_option(&mut self, socket: &mut Self::SocketDescriptor, option: SocketOption) -> Result<(), Fail> {
        trace!("Set socket option to {:?}", option);
        match option {
            SocketOption::Linger(linger) => socket.set_linger(linger),
            SocketOption::KeepAlive(tcp_keepalive) => socket.set_tcp_keepalive(&tcp_keepalive),
            SocketOption::NoDelay(nagle_enabled) => socket.set_nagle(nagle_enabled),
        }
    }

    /// Gets an SO_* option on the socket. The option should be passed in as [option] and the value returned is either
    /// an error or must match [option] with a value.
    fn get_socket_option(
        &mut self,
        socket: &mut Self::SocketDescriptor,
        option: SocketOption,
    ) -> Result<SocketOption, Fail> {
        trace!("Get socket option: {:?}", option);
        match option {
            SocketOption::Linger(_) => Ok(SocketOption::Linger(socket.get_linger()?)),
            SocketOption::KeepAlive(_) => Ok(SocketOption::KeepAlive(socket.get_tcp_keepalive()?)),
            SocketOption::NoDelay(_) => Ok(SocketOption::NoDelay(socket.get_nagle()?)),
        }
    }

    // Gets address of peer connected to socket
    fn getpeername(&mut self, socket: &mut Self::SocketDescriptor) -> Result<SocketAddrV4, Fail> {
        let addr: Result<SocketAddrV4, Fail> = socket.getpeername();
        match addr {
            Ok(addr) => Ok(addr),
            Err(_) => {
                let cause: String = format!("failed to get peer address (errno={:?})", unsafe { WSAGetLastError() });
                error!("getpeername(): {:?}", cause);
                Err(Fail::new(libc::EINVAL, &cause))
            },
        }
    }

    /// Synchronously shut down the specified socket.
    fn hard_close(&mut self, socket: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        socket.shutdown()
    }

    /// Asynchronously disconnect and shut down a socket.
    async fn close(&mut self, socket: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        match unsafe {
            self.0.iocp.do_io(
                SocketOpState::Close,
                |_: Pin<&mut SocketOpState>, overlapped: *mut OVERLAPPED| socket.start_disconnect(overlapped),
                |_: Pin<&mut SocketOpState>, result: OverlappedResult| socket.finish_disconnect(result),
            )
        }
        .await
        {
            Err(err) if err.errno == libc::ENOTCONN => match socket.shutdown() {
                Err(err) if err.errno == libc::ENOTCONN => Ok(()),
                r => r,
            },
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
        let start = |accept_result: Pin<&mut SocketOpState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
            socket.start_accept(accept_result, overlapped)
        };
        let me_finish: Self = self.clone();
        let finish = |accept_result: Pin<&mut SocketOpState>,
                      result: OverlappedResult|
         -> Result<(Socket, SocketAddr, SocketAddr), Fail> {
            socket.finish_accept(accept_result, &me_finish.0.iocp, result)
        };

        let (socket, _local_addr, remote_addr) = unsafe {
            self.0
                .iocp
                .do_io(SocketOpState::Accept(AcceptState::new()), start, finish)
        }
        .await?;

        Ok((socket, remote_addr))
    }

    /// Connect a socket to a remote address.
    async fn connect(&mut self, socket: &mut Self::SocketDescriptor, remote: SocketAddr) -> Result<(), Fail> {
        unsafe {
            self.0.iocp.do_io(
                SocketOpState::Connect,
                |_: Pin<&mut SocketOpState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                    socket.start_connect(remote, overlapped)
                },
                |_: Pin<&mut SocketOpState>, result: OverlappedResult| -> Result<(), Fail> {
                    socket.finish_connect(result)
                },
            )
        }
        .await
    }

    /// Pop data from the socket into `buf`. This method will return the remote address iff the socket is not connected.
    async fn pop(
        &mut self,
        socket: &mut Self::SocketDescriptor,
        size: usize,
    ) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        unsafe {
            self.0.iocp.do_io(
                SocketOpState::Pop(PopState::new(buf.clone())),
                |state: Pin<&mut SocketOpState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                    socket.start_pop(state, overlapped)
                },
                |state: Pin<&mut SocketOpState>,
                 result: OverlappedResult|
                 -> Result<(usize, Option<SocketAddr>), Fail> { socket.finish_pop(state, result) },
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
            Ok((sockaddr, buf))
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
                self.0.iocp.do_io(
                    SocketOpState::Push(buf.clone()),
                    |state: Pin<&mut SocketOpState>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                        socket.start_push(state, addr, overlapped)
                    },
                    |_: Pin<&mut SocketOpState>, result: OverlappedResult| -> Result<usize, Fail> {
                        socket.finish_push(result)
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

impl MemoryRuntime for SharedCatnapTransport {}
