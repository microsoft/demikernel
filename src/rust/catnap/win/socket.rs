// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    fmt::Debug,
    marker::PhantomPinned,
    mem::MaybeUninit,
    net::{
        Ipv4Addr,
        Ipv6Addr,
        SocketAddr,
        SocketAddrV4,
        SocketAddrV6,
    },
    pin::Pin,
    rc::Rc,
    time::Duration,
};

use windows::{
    core::PSTR,
    Win32::{
        Foundation::{
            BOOL,
            ERROR_NOT_FOUND,
            FALSE,
            HANDLE,
            TRUE,
        },
        Networking::WinSock::{
            bind,
            closesocket,
            listen,
            shutdown,
            tcp_keepalive,
            WSAGetLastError,
            WSARecvFrom,
            WSASendTo,
            FROM_PROTOCOL_INFO,
            INVALID_SOCKET,
            IPPROTO_TCP,
            LINGER,
            SD_BOTH,
            SIO_KEEPALIVE_VALS,
            SOCKADDR,
            SOCKADDR_IN,
            SOCKADDR_IN6,
            SOCKADDR_INET,
            SOCKADDR_STORAGE,
            SOCKET,
            SOCKET_ERROR,
            SOL_SOCKET,
            SO_KEEPALIVE,
            SO_LINGER,
            SO_PROTOCOL_INFOW,
            SO_UPDATE_ACCEPT_CONTEXT,
            SO_UPDATE_CONNECT_CONTEXT,
            TCP_NODELAY,
            WSABUF,
            WSAEINVAL,
            WSAPROTOCOL_INFOW,
            WSA_FLAG_OVERLAPPED,
        },
        System::IO::{
            CancelIoEx,
            OVERLAPPED,
        },
    },
};

use crate::{
    catnap::transport::{
        error::{
            expect_last_wsa_error,
            get_overlapped_api_result,
        },
        overlapped::{
            IoCompletionPort,
            OverlappedResult,
        },
        winsock::{
            SocketExtensions,
            WinsockRuntime,
        },
        WinConfig,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

// Per AcceptEx documentation, the accept address storage buffer must be at least the size of the relevant socket
// address type plus 16 bytes.
const SOCKADDR_BUF_SIZE: usize = std::mem::size_of::<SOCKADDR_STORAGE>() + 16;

// AcceptEx buffer returns two addresses.
const ACCEPT_BUFFER_LEN: usize = (SOCKADDR_BUF_SIZE) * 2;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Socket {
    s: SOCKET,
    extensions: Rc<SocketExtensions>,
}

/// State type used by `Socket::start_accept` and `Socket::finish_accept`.
pub struct AcceptState {
    new_socket: Option<Socket>,
    buffer: [u8; ACCEPT_BUFFER_LEN],
    _marker: PhantomPinned,
}

/// State type used by `Socket::start_pop` and `Socket::finish_pop`.
pub struct PopState {
    buffer: DemiBuffer,
    address: MaybeUninit<SOCKADDR_STORAGE>,
    addr_len: i32,
    _marker: PhantomPinned,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl AcceptState {
    /// Create a new, empty `AcceptState``.
    pub fn new() -> Self {
        Self {
            new_socket: None,
            buffer: [0u8; ACCEPT_BUFFER_LEN],
            _marker: PhantomPinned,
        }
    }
}

impl PopState {
    /// Create a new, empty `PopState` using the specified receive buffer.
    pub fn new(buffer: DemiBuffer) -> PopState {
        Self {
            buffer,
            address: MaybeUninit::zeroed(),
            addr_len: 0,
            _marker: PhantomPinned,
        }
    }
}

impl Socket {
    /// Create a new socket, wrapping the underlying OS `SOCKET` handle.
    pub(super) fn new(
        s: SOCKET,
        protocol: libc::c_int,
        config: &WinConfig,
        extensions: Rc<SocketExtensions>,
        iocp: &IoCompletionPort,
    ) -> Result<Socket, Fail> {
        let s: Socket = Socket { s, extensions };
        s.setup_socket(protocol, config)?;
        iocp.associate_socket(s.s, 0)?;
        Ok(s)
    }

    /// Translate a SocketAddr to SOCKADDR_INET and byte length.
    fn translate_address(addr: SocketAddr) -> (SOCKADDR_INET, i32) {
        match addr {
            SocketAddr::V4(addr) => (addr.into(), std::mem::size_of::<SOCKADDR_IN>() as i32),
            SocketAddr::V6(addr) => (addr.into(), std::mem::size_of::<SOCKADDR_IN6>() as i32),
        }
    }

    /// Setup relevant socket options according to the configuration.
    fn setup_socket(&self, protocol: libc::c_int, config: &WinConfig) -> Result<(), Fail> {
        self.set_linger(config.linger_time)?;
        if protocol == IPPROTO_TCP.0 {
            self.set_tcp_keepalive(&config.keepalive_params)?;

            if let Some(nagle) = config.nagle {
                self.set_nagle(nagle)?;
            }
        }
        Ok(())
    }

    /// Set linger socket options.
    fn set_linger(&self, linger_time: Option<Duration>) -> Result<(), Fail> {
        let l: LINGER = LINGER {
            l_onoff: if linger_time.is_some() { 1 } else { 0 },
            l_linger: linger_time.unwrap_or(Duration::ZERO).as_secs() as u16,
        };

        unsafe { WinsockRuntime::do_setsockopt(self.s, SOL_SOCKET, SO_LINGER, Some(&l)) }?;
        Ok(())
    }

    /// Set TCP keepalive socket options.
    fn set_tcp_keepalive(&self, keepalive_params: &tcp_keepalive) -> Result<(), Fail> {
        unsafe { WinsockRuntime::do_setsockopt(self.s, SOL_SOCKET, SO_KEEPALIVE, Some(&keepalive_params.onoff)) }?;

        if keepalive_params.onoff != 0 {
            // Safety: SIO_KEEPALIVE_VALS uses tcp_keepalive structure as ABI.
            unsafe {
                WinsockRuntime::do_ioctl::<tcp_keepalive, ()>(self.s, SIO_KEEPALIVE_VALS, Some(&keepalive_params), None)
            }?;
        }

        Ok(())
    }

    /// Enable or disable the use of Nagle's algorithm for TCP.
    fn set_nagle(&self, enabled: bool) -> Result<(), Fail> {
        // Note the inverted condition here: TCP_NODELAY is a disabler for Nagle's algorithm.
        let value: BOOL = if enabled { FALSE } else { TRUE };
        unsafe { WinsockRuntime::do_setsockopt(self.s, IPPROTO_TCP.0, TCP_NODELAY, Some(&value)) }?;
        Ok(())
    }

    /// Make a new socket like some template socket.
    pub fn new_like(template: &Socket) -> Result<Socket, Fail> {
        // Safety: SO_PROTOCOL_INFOW fills out a WSAPROTOCOL_INFOW structure.
        let protocol: WSAPROTOCOL_INFOW =
            unsafe { WinsockRuntime::do_getsockopt(template.s, SOL_SOCKET, SO_PROTOCOL_INFOW) }?;

        let extensions: Rc<SocketExtensions> = template.extensions.clone();

        // Safety: SOCKET handle is transferred to a Socket instance, which will safely close the handle on drop.
        let s: SOCKET = unsafe {
            WinsockRuntime::raw_socket(
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                Some(&protocol),
                WSA_FLAG_OVERLAPPED,
            )
        }?;

        Ok(Socket { s, extensions })
    }

    /// Begin disconnecting a connection-oriented socket. If called on a non-stream-based socket or an unconnected
    /// stream-based socket, this method will return an `ENOTCONN` failure.
    pub fn start_disconnect(&self, overlapped: *mut OVERLAPPED) -> Result<(), Fail> {
        let result: bool = unsafe { self.extensions.disconnectex.unwrap()(self.s, overlapped, 0, 0).as_bool() };

        get_overlapped_api_result(result)
    }

    /// Call once the overlapped operation started by `start_disconnect` has completed to finish disconnecting and
    /// shutdown the socket.
    pub fn finish_disconnect(&self, result: OverlappedResult) -> Result<(), Fail> {
        self.shutdown().and(result.ok())
    }

    /// Shutdown communication on the socket. For better asynchronous behavior on connection-oriented sockets,
    /// `start_disconnect` will start an asynchronous disconnect operation. If the socket is not disconnected prior to
    /// this call, this call may block for socket teardown, depending on the linger settings.
    pub fn shutdown(&self) -> Result<(), Fail> {
        if unsafe { shutdown(self.s, SD_BOTH) } == 0 {
            Ok(())
        } else {
            Err(expect_last_wsa_error().into())
        }
    }

    /// Call `bind` winsock API on self.
    pub fn bind(&self, local: SocketAddr) -> Result<(), Fail> {
        let sockaddr: socket2::SockAddr = local.into();
        let result: i32 = unsafe { bind(self.s, sockaddr.as_ptr().cast(), sockaddr.len()) };

        if result == 0 {
            Ok(())
        } else {
            Err(expect_last_wsa_error().into())
        }
    }

    /// Call `listen` winsock API on self.
    pub fn listen(&self, backlog: usize) -> Result<(), Fail> {
        let backlog: i32 = i32::try_from(backlog).unwrap_or(i32::MAX);
        if unsafe { listen(self.s, backlog) } == 0 {
            Ok(())
        } else {
            Err(expect_last_wsa_error().into())
        }
    }

    /// Cancel an overlapped I/O operation. If the operation completed prior to the cancellation attempt, or if the
    /// OVERLAPPED structure is not associated with a known operation, this method will fail with EINPROGRESS. If the
    /// method succeeds, the OVERLAPPED will never be dequed from the completion port associated with the operation.
    /// Other failures indicate that the operation might still complete.
    pub fn cancel_io(&self, overlapped: *mut OVERLAPPED) -> Result<(), Fail> {
        unsafe { CancelIoEx(HANDLE(self.s.0 as isize), Some(overlapped)) }.map_err(|win_err| {
            if win_err.code() == ERROR_NOT_FOUND.into() {
                Fail::new(libc::EINPROGRESS, "cannot cancel this operation")
            } else {
                win_err.into()
            }
        })
    }

    /// Start an overlapped accept operation; this must be called from inside IoCompletionPort::do_io/do_socket_io.
    /// Once the operation completes, the AcceptState can be given to `finish_accept` to finish the operation.
    pub fn start_accept(
        &self,
        mut accept_result: Pin<&mut AcceptState>,
        overlapped: *mut OVERLAPPED,
    ) -> Result<(), Fail> {
        let new_socket: Socket = Socket::new_like(self)?;

        // Safety: getting the buffer pointer does not violate pinning invariants.
        let buf_ptr: *mut u8 = unsafe { accept_result.as_mut().get_unchecked_mut() }
            .buffer
            .as_mut_ptr();
        let mut bytes_out: u32 = 0;

        // Safety: buffer pointers refer to valid, live locations for the duration of the call iff accept_result stays
        // alive until the completion.
        let success: bool = unsafe {
            self.extensions.acceptex.unwrap()(
                self.s,
                new_socket.s,
                buf_ptr.cast(),
                0,
                SOCKADDR_BUF_SIZE as u32,
                SOCKADDR_BUF_SIZE as u32,
                &mut bytes_out,
                overlapped,
            )
        }
        .as_bool();

        get_overlapped_api_result(success).and_then(|_| {
            // Safety: the socket does not require structural pinning.
            unsafe { accept_result.as_mut().get_unchecked_mut() }.new_socket = Some(new_socket);
            Ok(())
        })
    }

    /// Finish an accept operation, once the overlapped accept call has completed. Calling this method before the I/O
    /// operation is complete is unsound and may result in undefined behavior. The method returns the new socket along
    /// with the (local, remote) address pair.
    pub fn finish_accept(
        &self,
        mut accept_result: Pin<&mut AcceptState>,
        iocp: &IoCompletionPort,
        result: OverlappedResult,
    ) -> Result<(Socket, SocketAddr, SocketAddr), Fail> {
        if let Err(err) = result.ok() {
            return Err(err);
        }

        // NB Windows docs are unclear whether the "bytes transferred" overlapped result should be 0 for AcceptEx with
        // no receive, or whether it is equal to the local+remote address buffer length. It is safe to assume addresses
        // were provisioned correctly by the API.
        // Safety: the socket does not require structural pinning.
        let new_socket = unsafe { accept_result.as_mut().get_unchecked_mut() }
            .new_socket
            .take()
            .ok_or_else(|| Fail::new(libc::EINVAL, "invalid state"))?;

        // Required to update user mode attributes of the socket after AcceptEx completes. This will propagate socket
        // options from listening socket to accepted socket, as if accept(...) was called.
        // Safety: FFI call to Windows API; no specific considerations.
        unsafe { WinsockRuntime::do_setsockopt(new_socket.s, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT, Some(&self.s)) }?;

        iocp.associate_socket(new_socket.s, 0)?;

        let (local_addr, remote_addr) = unsafe {
            // NB socket2 uses the windows-sys crate, so type names are qualified here to prevent confusion with Windows
            // crate.
            let mut localsockaddr: MaybeUninit<*mut windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> =
                MaybeUninit::zeroed();
            let mut localsockaddrlength: i32 = 0;
            let mut remotesockaddr: MaybeUninit<*mut windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> =
                MaybeUninit::zeroed();
            let mut remotesockaddrlength: i32 = 0;

            self.extensions.get_acceptex_sockaddrs.unwrap()(
                accept_result.buffer.as_ptr().cast(),
                0,
                SOCKADDR_BUF_SIZE as u32,
                SOCKADDR_BUF_SIZE as u32,
                localsockaddr.as_mut_ptr().cast(),
                &mut localsockaddrlength,
                remotesockaddr.as_mut_ptr().cast(),
                &mut remotesockaddrlength,
            );

            // Postcondition: if a buffer of valid size is passed to GetAcceptExSockaddrs, the returned pointers will
            // be non-null.
            assert!(!localsockaddr.assume_init_ref().is_null() && !remotesockaddr.assume_init_ref().is_null());

            (
                socket2::SockAddr::new(*localsockaddr.assume_init(), localsockaddrlength),
                socket2::SockAddr::new(*remotesockaddr.assume_init(), remotesockaddrlength),
            )
        };

        let local_addr: SocketAddr = local_addr
            .as_socket()
            .ok_or_else(|| Fail::new(libc::EAFNOSUPPORT, "bad local socket address from accept"))?;

        let remote_addr: SocketAddr = remote_addr
            .as_socket()
            .ok_or_else(|| Fail::new(libc::EAFNOSUPPORT, "bad remote socket address from accept"))?;

        Ok((new_socket, local_addr, remote_addr))
    }

    /// Start an overlapped connect operation.
    pub fn start_connect(&self, remote: SocketAddr, overlapped: *mut OVERLAPPED) -> Result<(), Fail> {
        // Constants missing from windows crate.
        const IN6ADDR_ANY: SocketAddrV6 = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0);
        const INADDR_ANY: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);

        let (remote_addr, remote_len): (SOCKADDR_INET, i32) = Self::translate_address(remote);
        let success: bool = unsafe {
            // NB ConnectEx requires the socket to be explicitly bound.
            let (localaddr, locallen): (SOCKADDR_INET, i32) = match remote {
                SocketAddr::V4(_) => Self::translate_address(INADDR_ANY.into()),
                SocketAddr::V6(_) => Self::translate_address(IN6ADDR_ANY.into()),
            };
            if bind(self.s, (&localaddr as *const SOCKADDR_INET).cast(), locallen) != 0 {
                // WSAEINVAL indicates the socket is already bound. Try to connect.
                if WSAGetLastError() != WSAEINVAL {
                    return Err(expect_last_wsa_error());
                }
            }

            self.extensions.connectex.unwrap()(
                self.s,
                (&remote_addr as *const SOCKADDR_INET).cast(),
                remote_len,
                std::ptr::null(),     // No send data
                0,                    // Ignored
                std::ptr::null_mut(), // Ignored
                overlapped,
            )
            .as_bool()
        };

        get_overlapped_api_result(success)
    }

    /// Finish a connect operation started by start_connect.
    pub fn finish_connect(&self, result: OverlappedResult) -> Result<(), Fail> {
        if let Err(err) = result.ok() {
            return Err(err);
        }

        // Required to update user mode attributes of the socket after ConnectEx completes.
        unsafe { WinsockRuntime::do_setsockopt::<()>(self.s, SOL_SOCKET, SO_UPDATE_CONNECT_CONTEXT, None) }
    }

    /// Start a pop operation, as intended for use with `IoCompletionPort::do_io_with`. The operation may complete
    /// immediately, in which case an overlapped completion is not scheduled. To indicate this case, this method will
    /// return EAGAIN and update `buffer` according to the number of bytes received.
    pub fn start_pop(&self, mut pop_state: Pin<&mut PopState>, overlapped: *mut OVERLAPPED) -> Result<(), Fail> {
        let mut bytes_transferred: u32 = 0;
        let mut flags: u32 = 0;
        let success: bool = unsafe {
            let wsa_buffer: WSABUF = WSABUF {
                len: pop_state.buffer.len() as u32,
                // Safety: loading the buffer pointer won't violate pinning invariants.
                buf: PSTR::from_raw(pop_state.as_mut().get_unchecked_mut().buffer.as_mut_ptr()),
            };

            // NB winsock service providers are required to capture the entire WSABUF array inline with the call, so
            // wsa_buffer and the derivative slice can safely drop after the call.
            let result: i32 = WSARecvFrom(
                self.s,
                std::slice::from_ref(&wsa_buffer),
                Some(&mut bytes_transferred),
                &mut flags,
                Some(pop_state.as_mut().get_unchecked_mut().address.as_mut_ptr() as *mut SOCKADDR),
                Some(&mut pop_state.as_mut().get_unchecked_mut().addr_len),
                Some(overlapped),
                None,
            );

            result != SOCKET_ERROR
        };

        get_overlapped_api_result(success)
    }

    /// Finish an overlapped pop operation started with start_pop.
    pub fn finish_pop(
        &self,
        mut pop_state: Pin<&mut PopState>,
        result: OverlappedResult,
    ) -> Result<(usize, Option<SocketAddr>), Fail> {
        // Note: the `flags` output of WSARecvFrom is not immediately avialable as written. These can be rehydrated by
        // calling WSAGetOverlappedResult on the OVERLAPPED; however, the overlapped is not immediately available. The
        // flags seem (determined experimentally) to be derived from the NTSTATUS of the resulting overlapped. The
        // WSA/GetOverlappedResult API should be avoided, as it seems to incur some synchronization overhead which is
        // not useful for this implementation.
        if let Err(err) = result.ok() {
            return Err(err);
        }

        let addr: Option<SocketAddr> = if pop_state.addr_len > 0 {
            // Safety: since we are done with the overlapped API, pinning is no longer required for pop_state. The
            // returned address and addr_len values come from the OS, so they will be valid to pass to socket2.
            unsafe {
                socket2::SockAddr::new(
                    std::mem::transmute(std::mem::take(
                        pop_state.as_mut().get_unchecked_mut().address.assume_init_mut(),
                    )),
                    pop_state.addr_len,
                )
            }
            .as_socket()
        } else {
            None
        };

        Ok((result.bytes_transferred as usize, addr))
    }

    /// Start a push operation, as intended for use with `IoCompletionPort::do_io_with`.
    pub fn start_push(
        &self,
        buffer: Pin<&mut DemiBuffer>,
        addr: Option<SocketAddr>,
        overlapped: *mut OVERLAPPED,
    ) -> Result<(), Fail> {
        let mut bytes_transferred: u32 = 0;
        let success: bool = unsafe {
            let wsa_buffer: WSABUF = WSABUF {
                len: buffer.len() as u32,
                // Safety: loading the buffer pointer won't violate pinning invariants.
                buf: PSTR::from_raw(buffer.get_unchecked_mut().as_mut_ptr()),
            };

            let addr: Option<socket2::SockAddr> = addr.map(socket2::SockAddr::from);

            // NB winsock service providers are required to capture the entire WSABUF array inline with the call, so
            // wsa_buffer and the derivative slice can safely drop after the call.
            // Per Windows documentation, WSASendTo ignores the destination address for connection oriented sockets and
            // functions equivalently to WSASend.
            let result: i32 = WSASendTo(
                self.s,
                std::slice::from_ref(&wsa_buffer),
                Some(&mut bytes_transferred),
                0,
                addr.as_ref()
                    .map(|addr: &socket2::SockAddr| -> *const SOCKADDR { addr.as_ptr().cast() }),
                addr.as_ref()
                    .map(|addr: &socket2::SockAddr| -> i32 { addr.len() })
                    .unwrap_or(0),
                Some(overlapped),
                None,
            );

            result != SOCKET_ERROR
        };

        get_overlapped_api_result(success)
    }

    /// Finish a push operation started with start_push.
    pub fn finish_push(&self, _buffer: Pin<&mut DemiBuffer>, result: OverlappedResult) -> Result<usize, Fail> {
        result.ok().and(Ok(result.bytes_transferred as usize))
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

impl Drop for Socket {
    /// Close socket handle.
    fn drop(&mut self) {
        unsafe { closesocket(self.s) };
        self.s = INVALID_SOCKET;
    }
}

impl Debug for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Socket").field("s", &self.s).finish()
    }
}
