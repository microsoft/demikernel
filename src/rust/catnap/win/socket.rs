use std::{
    fmt::Debug,
    marker::PhantomPinned,
    mem::MaybeUninit,
    net::SocketAddr,
    pin::Pin,
    rc::Rc,
    time::Duration,
};

use windows::Win32::{
    Foundation::{
        ERROR_IO_PENDING,
        ERROR_NOT_FOUND,
        HANDLE,
    },
    Networking::WinSock::{
        bind,
        closesocket,
        listen,
        tcp_keepalive,
        GetAcceptExSockaddrs,
        WSAGetLastError,
        FROM_PROTOCOL_INFO,
        INVALID_SOCKET,
        IPPROTO_TCP,
        LINGER,
        SIO_KEEPALIVE_VALS,
        SOCKADDR_STORAGE,
        SOCKET,
        SOL_SOCKET,
        SO_KEEPALIVE,
        SO_LINGER,
        SO_PROTOCOL_INFOW,
        SO_UPDATE_ACCEPT_CONTEXT,
        SO_UPDATE_CONNECT_CONTEXT,
        WSAPROTOCOL_INFOW,
        WSA_FLAG_OVERLAPPED,
    },
    System::IO::{
        CancelIoEx,
        OVERLAPPED,
    },
};

use crate::{
    catnap::transport::error::translate_wsa_error,
    runtime::fail::Fail,
};

use super::{
    error::last_wsa_error,
    winsock::{
        SocketExtensions,
        WinsockRuntime,
    },
    WinConfig,
};

pub struct Socket {
    s: SOCKET,
    extensions: Rc<SocketExtensions>,
}

const SOCKADDR_BUF_SIZE: usize = std::mem::size_of::<SOCKADDR_STORAGE>() + 16;
const BUFFER_LEN: usize = (SOCKADDR_BUF_SIZE) * 2;

pub struct AcceptResult {
    new_socket: Option<Socket>,
    buffer: [u8; Self::buffer_len()],
    bytes_out: u32,
    _marker: PhantomPinned,
}

impl AcceptResult {
    pub fn new() -> Self {
        Self {
            new_socket: None,
            buffer: [0u8; Self::buffer_len()],
            bytes_out: 0,
            _marker: PhantomPinned,
        }
    }

    fn mut_buffer_ptr(mut self: Pin<&mut Self>) -> *mut u8 {
        self.buffer.as_mut_ptr()
    }

    const fn buffer_len() -> usize {
        BUFFER_LEN
    }

    fn mut_bytes_out_ref(mut self: Pin<&mut Self>) -> &mut u32 {
        &mut self.bytes_out
    }
}

impl Socket {
    pub(super) fn new(
        s: SOCKET,
        protocol: libc::c_int,
        config: &WinConfig,
        extensions: Rc<SocketExtensions>,
    ) -> Result<Socket, Fail> {
        let s: Socket = Socket { s, extensions };
        s.setup_socket(protocol, config)?;
        Ok(s)
    }

    pub fn as_raw_fd(&self) -> SOCKET {
        self.s
    }

    fn setup_socket(&self, protocol: libc::c_int, config: &WinConfig) -> Result<(), Fail> {
        self.set_linger(config.linger_time)?;
        if protocol == IPPROTO_TCP.0 {
            self.set_tcp_keepalive(&config.keepalive_params)?;
        }
        Ok(())
    }

    fn set_linger(&self, linger_time: Option<Duration>) -> Result<(), Fail> {
        let mut l: LINGER = LINGER {
            l_onoff: if linger_time.is_some() { 1 } else { 0 },
            l_linger: linger_time.unwrap_or(Duration::ZERO).as_secs() as u16,
        };

        unsafe { WinsockRuntime::do_setsockopt(self.s, SOL_SOCKET, SO_LINGER, Some(&l)) }?;
        Ok(())
    }

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

    /// Make a new socket like some template socket.
    pub fn new_like(template: &Socket) -> Result<Socket, Fail> {
        // Safety: SO_PROTOCOL_INFOW fills out a WSAPROTOCOL_INFOW structure.
        let protocol: WSAPROTOCOL_INFOW =
            unsafe { WinsockRuntime::do_getsockopt(template.s, SOL_SOCKET, SO_PROTOCOL_INFOW) }?;

        let s: SOCKET = WinsockRuntime::raw_socket(
            FROM_PROTOCOL_INFO,
            FROM_PROTOCOL_INFO,
            FROM_PROTOCOL_INFO,
            Some(&protocol),
            WSA_FLAG_OVERLAPPED,
        )?;

        Ok(Socket {
            s,
            extensions: template.extensions.clone(),
        })
    }

    /// Call `bind` winsock API on self.
    pub fn bind(&self, local: SocketAddr) -> Result<(), Fail> {
        let sockaddr: socket2::SockAddr = local.into();
        if unsafe { bind(self.s, sockaddr.as_ptr().cast(), sockaddr.len()) } == 0 {
            Ok(())
        } else {
            Err(Fail::new(last_wsa_error(), "bind failed"))
        }
    }

    /// Call `listen` winsock API on self.
    pub fn listen(&self, backlog: usize) -> Result<(), Fail> {
        let backlog: i32 = i32::try_from(backlog).unwrap_or(i32::MAX);
        if unsafe { listen(self.s, backlog) } == 0 {
            Ok(())
        } else {
            Err(Fail::new(last_wsa_error(), "listen failed"))
        }
    }

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
        mut accept_result: Pin<&mut AcceptResult>,
        overlapped: *mut OVERLAPPED,
    ) -> Result<(), Fail> {
        let new_socket: Socket = Socket::new_like(self)?;

        let result: bool = unsafe {
            self.extensions.acceptex.unwrap()(
                self.s,
                new_socket.s,
                accept_result.buffer.as_mut_ptr().cast(),
                0,
                SOCKADDR_BUF_SIZE as u32,
                SOCKADDR_BUF_SIZE as u32,
                &mut accept_result.bytes_out,
                overlapped,
            )
        }
        .as_bool();

        match (result, unsafe { WSAGetLastError() }) {
            (false, err) if err.0 != ERROR_IO_PENDING.0 as i32 => {
                Err(Fail::new(translate_wsa_error(err), "accept failed"))
            },
            _ => {
                accept_result.new_socket = Some(new_socket);
                Ok(())
            },
        }
    }

    /// Finish an accept operation, once the overlapped accept call has completed. Calling this method before the I/O
    /// operation is complete is unsound and may result in undefined behavior. The method returns the new socket along
    /// with the (local, remote) address pair.
    pub fn finish_accept(
        &self,
        accept_result: Pin<&mut AcceptResult>,
        _overlapped: *const OVERLAPPED,
        _completion_key: usize,
    ) -> Result<(Socket, SocketAddr, SocketAddr), Fail> {
        let new_socket = accept_result
            .new_socket
            .ok_or_else(|| Fail::new(libc::EINVAL, "invalid state"))?;

        // Required to update user mode attributes of the socket after AcceptEx completes. This will propagate socket
        // options from listening socket to accepted socket, as if accept(...) was called.
        unsafe { WinsockRuntime::do_setsockopt(new_socket.s, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT, Some(&self.s)) }?;

        let (local_addr, remote_addr) = unsafe {
            // NB socket2 uses the windows-sys crate, so type names are qualified here to prevent confusion with Windows
            // crate.
            let mut localsockaddr: MaybeUninit<*mut windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> =
                MaybeUninit::zeroed();
            let mut localsockaddrlength: i32 = 0;
            let mut remotesockaddr: MaybeUninit<*mut windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> =
                MaybeUninit::zeroed();
            let mut remotesockaddrlength: i32 = 0;
            GetAcceptExSockaddrs(
                accept_result.buffer.as_ptr().cast(),
                0,
                SOCKADDR_BUF_SIZE as u32,
                SOCKADDR_BUF_SIZE as u32,
                localsockaddr.as_mut_ptr().cast(),
                &mut localsockaddrlength,
                remotesockaddr.as_mut_ptr().cast(),
                &mut remotesockaddrlength,
            );
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

    pub fn start_conect(&self, remote: SocketAddr, overlapped: *mut OVERLAPPED) -> Result<(), Fail> {
        let sockaddr: socket2::SockAddr = remote.into();
        let result: bool = unsafe {
            self.extensions.connectex.unwrap()(
                self.s,
                sockaddr.as_ptr().cast(),
                sockaddr.len(),
                std::ptr::null(),     // No send data
                0,                    // Ignored
                std::ptr::null_mut(), // Ignored
                overlapped,
            )
        }
        .as_bool();

        match (result, unsafe { WSAGetLastError() }) {
            (false, err) if err.0 != ERROR_IO_PENDING.0 as i32 => {
                Err(Fail::new(translate_wsa_error(err), "connect failed"))
            },
            _ => Ok(()),
        }
    }

    pub fn finish_connect(&self, _overlapped: &OVERLAPPED, _completion_key: usize) -> Result<(), Fail> {
        // Required to update user mode attributes of the socket after ConnectEx completes.
        unsafe { WinsockRuntime::do_setsockopt::<()>(self.s, SOL_SOCKET, SO_UPDATE_CONNECT_CONTEXT, None) }
    }
}

impl Drop for Socket {
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
