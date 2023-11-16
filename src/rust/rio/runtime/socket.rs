use std::{
    mem::MaybeUninit,
    net::SocketAddr,
    rc::Rc,
    time::Duration,
};

use windows::Win32::{
    Foundation::{
        GetLastError,
        BOOL,
        ERROR_IO_PENDING,
        FALSE,
        HANDLE,
        TRUE,
    },
    Networking::WinSock::{
        closesocket,
        tcp_keepalive,
        GetAcceptExSockaddrs,
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
        WSAPROTOCOL_INFOW,
        WSA_FLAG_OVERLAPPED,
    },
    System::IO::{
        GetOverlappedResult,
        OVERLAPPED,
    },
};

use crate::runtime::fail::Fail;

use super::{
    winsock::{
        SocketExtensions,
        WinsockRuntime,
    },
    RioConfig,
};

pub struct Socket {
    s: SOCKET,
    extensions: Rc<SocketExtensions>,
}

impl Socket {
    pub(super) fn new(
        s: SOCKET,
        protocol: libc::c_int,
        config: &RioConfig,
        extensions: Rc<SocketExtensions>,
    ) -> Result<Socket, Fail> {
        let s: Socket = Socket { s, extensions };
        s.setup_socket(protocol, config)?;
        Ok(s)
    }

    fn setup_socket(&self, protocol: libc::c_int, config: &RioConfig) -> Result<(), Fail> {
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

    fn dup_socket_kind(&self, config: &RioConfig) -> Result<Socket, Fail> {
        // Safety: SO_PROTOCOL_INFOW fills out a WSAPROTOCOL_INFOW structure.
        let protocol: WSAPROTOCOL_INFOW =
            unsafe { WinsockRuntime::do_getsockopt(self.s, SOL_SOCKET, SO_PROTOCOL_INFOW) }?;

        let s: SOCKET = WinsockRuntime::raw_socket(
            FROM_PROTOCOL_INFO,
            FROM_PROTOCOL_INFO,
            FROM_PROTOCOL_INFO,
            Some(&protocol),
            WSA_FLAG_OVERLAPPED,
        )?;

        Ok(Socket {
            s,
            extensions: self.extensions.clone(),
        })
    }

    pub(super) fn accept(&mut self, config: &RioConfig) -> Result<(Socket, SocketAddr), Fail> {
        let dup: Socket = self.dup_socket_kind(config)?;

        // Prevent any completion port notifications by setting the low-order bit of hEvent;
        // see https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getqueuedcompletionstatus
        let mut overlapped: OVERLAPPED = OVERLAPPED::default();
        overlapped.hEvent = HANDLE(1);

        // Per AcceptEx documentation, address storage must contain 16 bytes of buffer.
        const SOCKADDR_BUF_SIZE: usize = std::mem::size_of::<SOCKADDR_STORAGE>() + 16;
        const BUFFER_LEN: usize = (SOCKADDR_BUF_SIZE) * 2;
        let mut buffer: [u8; BUFFER_LEN] = [0u8; BUFFER_LEN];
        let mut bytes_out: u32 = 0;

        let result: BOOL = unsafe {
            self.extensions.acceptex.unwrap()(
                self.s,
                dup.s,
                buffer.as_mut_ptr() as *mut libc::c_void,
                0,
                SOCKADDR_BUF_SIZE as u32,
                SOCKADDR_BUF_SIZE as u32,
                &mut bytes_out,
                &mut overlapped,
            )
        };

        if result == FALSE {
            match unsafe { GetLastError() } {
                // Wait for I/O to complete.
                Err(ref err) if err.code() == ERROR_IO_PENDING.into() => {
                    let mut bytes_transferred: u32 = 0;
                    while let Err(ref err) =
                        unsafe { GetOverlappedResult(None, &overlapped, &mut bytes_transferred, TRUE) }
                    {
                        if err.code() != ERROR_IO_PENDING.into() {
                            return Err(Fail::from(err));
                        };
                    }
                },

                Err(ref err) => {
                    return Err(Fail::from(err));
                },

                Ok(_) => {
                    return Err(Fail::new(libc::EFAULT, "GetOverlappedResult returned unexpectedly"));
                },
            }
        }

        // Required to update user mode attributes of the socket after AcceptEx completes. This will propagate socket
        // options from listening socket to accepted socket, as if accept(...) was called.
        // TODO: can this be called while I/O is pending?
        unsafe { WinsockRuntime::do_setsockopt(dup.s, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT, Some(&self.s)) }?;

        let remote_addr: socket2::SockAddr = unsafe {
            // NB socket2 uses the windows-sys crate, so type names are qualified here to prevent confusion with Windows
            // crate.
            let mut localsockaddr: MaybeUninit<*mut windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> =
                MaybeUninit::zeroed();
            let mut localsockaddrlength: i32 = 0;
            let mut remotesockaddr: MaybeUninit<*mut windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> =
                MaybeUninit::zeroed();
            let mut remotesockaddrlength: i32 = 0;
            GetAcceptExSockaddrs(
                buffer.as_ptr().cast(),
                0,
                SOCKADDR_BUF_SIZE as u32,
                SOCKADDR_BUF_SIZE as u32,
                localsockaddr.as_mut_ptr().cast(),
                &mut localsockaddrlength,
                remotesockaddr.as_mut_ptr().cast(),
                &mut remotesockaddrlength,
            );
            socket2::SockAddr::new(*remotesockaddr.assume_init(), remotesockaddrlength)
        };

        let remote_addr: SocketAddr = remote_addr
            .as_socket()
            .ok_or_else(|| Fail::new(libc::EAFNOSUPPORT, "bad remote socket address from accept"))?;

        Ok((dup, remote_addr))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        unsafe { closesocket(self.s) };
        self.s = INVALID_SOCKET;
    }
}
