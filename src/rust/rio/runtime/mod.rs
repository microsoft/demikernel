// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod error;
mod memory;
mod overlapped;

//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::HashMap,
    mem::MaybeUninit,
    net::SocketAddr,
    ops::Deref,
    rc::Rc,
    time::Duration,
};

use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    rio::runtime::memory::BufferRing,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        SharedObject,
    },
};

use windows::Win32::Networking::WinSock::{
    bind,
    closesocket,
    listen,
    tcp_keepalive,
    WSAAccept,
    WSAConnect,
    INVALID_SOCKET,
    SOCKET,
    WSADATA,
};

use self::{
    error::last_wsa_error,
    winsock::WinsockRuntime,
};

mod socket;
mod winsock;
pub use socket::Socket;

//==============================================================================
// Structures
//==============================================================================

#[derive(Default, Clone, Copy)]
pub(super) struct RioConfig {
    keepalive_params: tcp_keepalive,
    linger_time: Option<Duration>,
}

struct WinsockState {
    wsadata: WSADATA,
}

/// DPDK Runtime
pub struct RioRuntime {
    recv_ring: Rc<BufferRing>,
    send_ring: Rc<BufferRing>,
    runtime_config: RioConfig,
    winsock: WinsockRuntime,
}

#[derive(Clone)]
pub struct SharedRioRuntime(SharedObject<RioRuntime>);

//==============================================================================
// Associated Functions
//==============================================================================

impl SharedRioRuntime {
    pub fn new(config: Config) -> Result<SharedRioRuntime, Fail> {
        let (send_ring, recv_ring) = Self::init_buffer_rings(&config)?;

        let runtime_config: RioConfig = RioConfig {
            keepalive_params: config.tcp_keepalive()?,
            linger_time: config.linger_time()?,
        };

        Ok(Self(SharedObject::new(RioRuntime {
            recv_ring,
            send_ring,
            runtime_config,
            winsock: WinsockRuntime::new()?,
        })))
    }

    fn init_buffer_rings(config: &Config) -> Result<(Rc<BufferRing>, Rc<BufferRing>), Fail> {
        let recv_ring: Rc<BufferRing> = Rc::<BufferRing>::new(BufferRing::new(
            config.recv_buffer_count()?,
            config.recv_buffer_size()?,
        )?);

        let send_ring: Rc<BufferRing> = if config.share_send_recv_bufs()? {
            recv_ring
        } else {
            Rc::new(BufferRing::new(
                config.send_buffer_count()?,
                config.send_buffer_size()?,
            )?)
        };

        Ok((send_ring, recv_ring))
    }

    pub fn bind(&mut self, s: SOCKET, local: SocketAddr) -> Result<(), Fail> {
        let local: socket2::SockAddr = local.into();

        if unsafe { bind(s, local.as_ptr().cast(), local.len()) } == 0 {
            Ok(())
        } else {
            Err(Fail::new(last_wsa_error(), "bind failed"))
        }
    }

    pub fn listen(&mut self, s: SOCKET, backlog: usize) -> Result<(), Fail> {
        let backlog: i32 = i32::try_from(backlog).map_err(|_| Fail::new(libc::ERANGE, "backlog too large"))?;
        if unsafe { listen(s, backlog) } == 0 {
            Ok(())
        } else {
            Err(Fail::new(last_wsa_error(), "listen failed"))
        }
    }

    pub fn accept(&mut self, s: SOCKET) -> Result<(SOCKET, SocketAddr), Fail> {
        // NB socket2 uses the windows-sys crate, so type names are qualified here to prevent confusion with Windows
        // crate.
        let mut store: MaybeUninit<windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE> = MaybeUninit::zeroed();
        let mut len: i32 = std::mem::size_of::<windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE>() as i32;

        match unsafe {
            WSAAccept(
                s,
                Some(store.as_mut_ptr().cast()),
                Some((&mut len) as *mut i32),
                None,
                0,
            )
        } {
            newsock if newsock != INVALID_SOCKET => {
                let addr: socket2::SockAddr = unsafe { socket2::SockAddr::new(store.assume_init(), len) };
                match addr.as_socket() {
                    Some(socketaddr) => Ok((newsock, socketaddr)),
                    None => {
                        unsafe { closesocket(newsock) };
                        Err(Fail::new(
                            libc::EAFNOSUPPORT,
                            "bad address family for newly accepted socket",
                        ))
                    },
                }
            },

            // NB caller will distinguish retryable errors.
            _ => Err(Fail::new(last_wsa_error(), "WSAAccept failed")),
        }
    }

    pub fn try_connect(&mut self, s: SOCKET, addr: SocketAddr) -> Result<(), Fail> {
        let addr: socket2::SockAddr = addr.into();

        let res: i32 = unsafe { WSAConnect(s, addr.as_ptr().cast(), addr.len(), None, None, None, None) };
    }

    pub fn push(&mut self) {}

    pub fn sgaalloc_sendbuf(&mut self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.0.send_ring.sgaalloc(self.0.deref(), size)
    }

    pub fn sgafree_sendbuf(&mut self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.0.send_ring.sgafree(self.0.deref(), sga)
    }

    fn sgaalloc_recvbuf(&mut self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.0.recv_ring.sgaalloc(self.0.deref(), size)
    }

    fn sgafree_recvbuf(&mut self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.0.recv_ring.sgafree(self.0.deref(), sga)
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl MemoryRuntime for RioRuntime {}
