// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    collections::HashMap,
    mem::MaybeUninit,
    rc::{
        Rc,
        Weak,
    },
};

use crate::{
    catnap::transport::{
        error::expect_last_wsa_error,
        overlapped::IoCompletionPort,
        socket::Socket,
        WinConfig,
    },
    runtime::fail::Fail,
};
use windows::{
    core::{
        GUID,
        PSTR,
    },
    Win32::Networking::WinSock::{
        closesocket,
        getsockopt,
        setsockopt,
        WSACleanup,
        WSAIoctl,
        WSASocketW,
        WSAStartup,
        INVALID_SOCKET,
        LPFN_ACCEPTEX,
        LPFN_CONNECTEX,
        LPFN_DISCONNECTEX,
        LPFN_GETACCEPTEXSOCKADDRS,
        RIO_EXTENSION_FUNCTION_TABLE,
        SIO_GET_EXTENSION_FUNCTION_POINTER,
        SIO_GET_MULTIPLE_EXTENSION_FUNCTION_POINTER,
        SOCKET,
        SOL_SOCKET,
        SO_PROTOCOL_INFOW,
        WSADATA,
        WSAID_ACCEPTEX,
        WSAID_CONNECTEX,
        WSAID_DISCONNECTEX,
        WSAID_GETACCEPTEXSOCKADDRS,
        WSAPROTOCOL_INFOW,
        WSA_FLAG_OVERLAPPED,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

// TODO: update to use value from windows crate once exposed.
const WSAID_MULTIPLE_RIO: ::windows::core::GUID =
    ::windows::core::GUID::from_u128(0x8509e081_96dd_4005_b165_9e2ee8c79e3f);

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure stores Winsock extension functions.
#[derive(Default, Clone, Copy)]
pub(super) struct SocketExtensions {
    /// AcceptEx function pointer.
    pub acceptex: LPFN_ACCEPTEX,

    /// GetAcceptExSockaddrs function pointer.
    pub get_acceptex_sockaddrs: LPFN_GETACCEPTEXSOCKADDRS,

    /// ConnectEx function pointer.
    pub connectex: LPFN_CONNECTEX,

    /// DisconnectEx function pointer.
    pub disconnectex: LPFN_DISCONNECTEX,

    /// Registered I/O function table.
    // Note: RIO extensions are preserved for future use.
    #[allow(unused)]
    pub rio_fns: RIO_EXTENSION_FUNCTION_TABLE,
}

/// This structure manages the state of the Winsock runtime. Winsock state is managed internally by the Winsock runtime.
/// While this struct holds limited Winsock runtime data, a valid instance of this struct predicates initialization of
/// the Winsock runtime.
pub struct WinsockRuntime {
    /// A map of `SocketExtensions` keyed by Winsock provider GUID. Providers will be shared by multiple sockets;
    /// generally we expect a socket with the same creation parameters to be served by the same provider, although this
    /// is not required. System configuration changes can trigger new providers for new sockets.
    extensions_by_provider: HashMap<GUID, Weak<SocketExtensions>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SocketExtensions {
    /// Create an instance of SocketExtensions, with all extensions resolved for the socket provider.
    pub fn new(s: SOCKET) -> Result<Rc<SocketExtensions>, Fail> {
        Ok(Rc::new(SocketExtensions {
            acceptex: Self::lookup_single_fn(s, &WSAID_ACCEPTEX)?,
            get_acceptex_sockaddrs: Self::lookup_single_fn(s, &WSAID_GETACCEPTEXSOCKADDRS)?,
            connectex: Self::lookup_single_fn(s, &WSAID_CONNECTEX)?,
            disconnectex: Self::lookup_single_fn(s, &WSAID_DISCONNECTEX)?,
            rio_fns: Self::resolve_rio_fn_table(s)?,
        }))
    }

    /// Resolve the RIO function table, which uses a different I/O control code than individual functions.
    fn resolve_rio_fn_table(s: SOCKET) -> Result<RIO_EXTENSION_FUNCTION_TABLE, Fail> {
        let mut result: RIO_EXTENSION_FUNCTION_TABLE = RIO_EXTENSION_FUNCTION_TABLE::default();
        result.cbSize = std::mem::size_of::<RIO_EXTENSION_FUNCTION_TABLE>() as u32;

        // Safety: SIO_GET_MULTIPLE_EXTENSION_FUNCTION_POINTER expects input of type GUID and output of type
        // RIO_EXTENSION_FUNCTION_TABLE.
        unsafe {
            WinsockRuntime::do_ioctl(
                s,
                SIO_GET_MULTIPLE_EXTENSION_FUNCTION_POINTER,
                Some(&WSAID_MULTIPLE_RIO),
                Some(&mut result),
            )
        }?;

        if result.cbSize != std::mem::size_of::<RIO_EXTENSION_FUNCTION_TABLE>() as u32 {
            Err(Fail::new(
                libc::EFAULT,
                "Winsock did not return enough data for RIO_EXTENSION_FUNCTION_TABLE",
            ))
        } else {
            Ok(result)
        }
    }

    /// Lookup a single function pointer using SIO_GET_EXTENSION_FUNCTION_POINTER ioctl. To be sound, T must be a `fn`
    /// type.
    fn lookup_single_fn<T>(s: SOCKET, guid: &GUID) -> Result<Option<T>, Fail> {
        let mut fn_ptr: Option<T> = None;

        // Safety: SIO_GET_EXTENSION_FUNCTION_POINTER expects type GUID for input. Option<fn()> is compatible with C
        // output type for this ioctl.
        unsafe {
            WinsockRuntime::do_ioctl(s, SIO_GET_EXTENSION_FUNCTION_POINTER, Some(guid), Some(&mut fn_ptr)).map_err(
                |err| {
                    let msg: String = format!("{} for function lookup {:?}", err.cause, guid);
                    Fail::new(err.errno, msg.as_str())
                },
            )?;
        }

        if fn_ptr.is_some() {
            Ok(fn_ptr)
        } else {
            Err(Fail::new(libc::ENOTSUP, "Winsock extension not supported"))
        }
    }
}

impl WinsockRuntime {
    /// Start up the Winsock runtime, creating a new instance of WinsockRuntime. While it is not functionally useful,
    /// it is valid to instantiate multiple instances of this struct.
    pub fn new() -> Result<Self, Fail> {
        let mut data: WSADATA = WSADATA::default();
        if unsafe { WSAStartup(0x202u16, &mut data as *mut WSADATA) } != 0 {
            return Err(expect_last_wsa_error());
        }

        Ok(WinsockRuntime {
            extensions_by_provider: HashMap::new(),
        })
    }

    /// Implementation of `ioctl`
    pub(super) unsafe fn do_ioctl<T, U>(
        s: SOCKET,
        control_code: u32,
        input: Option<&T>,
        output: Option<&mut U>,
    ) -> Result<(), Fail> {
        let input: Option<*const libc::c_void> = input.map(|t: &T| -> *const libc::c_void { (t as *const T).cast() });
        let input_size: usize = input.map(|_| std::mem::size_of::<T>()).unwrap_or(0);

        let output: Option<*mut libc::c_void> = output.map(|u: &mut U| -> *mut libc::c_void { (u as *mut U).cast() });
        let output_size: usize = output.map(|_| std::mem::size_of::<U>()).unwrap_or(0);

        if input_size > u32::MAX as usize {
            return Err(Fail::new(
                libc::E2BIG,
                "\"input_size\" parameter to WSAIoctl parameter is too big",
            ));
        }
        if output_size > u32::MAX as usize {
            return Err(Fail::new(
                libc::E2BIG,
                "\"output_size\" parameter to WSAIoctl parameter is too big",
            ));
        }

        let mut bytes_returned: u32 = 0;
        let ret: i32 = unsafe {
            WSAIoctl(
                s,
                control_code,
                input,
                input_size as u32,
                output,
                output_size as u32,
                &mut bytes_returned,
                None,
                None,
            )
        };

        if ret == 0 {
            if bytes_returned == output_size as u32 {
                Ok(())
            } else {
                let s: String = format!("WSAIoctl returned {} bytes; expected {}", bytes_returned, output_size);
                Err(Fail::new(libc::EFAULT, s.as_str()))
            }
        } else {
            Err(expect_last_wsa_error())
        }
    }

    /// Execute an I/O control syscall on the socket.
    /// Safety: the I/O control code must match the expected input/output parameter types. If `output` is provided, the
    /// I/O control operation must write a valid value of type U on success, or nothing on failure.
    #[allow(unused)]
    pub unsafe fn ioctl<T, U>(
        &self,
        s: SOCKET,
        control_code: u32,
        input: Option<&T>,
        output: Option<&mut U>,
    ) -> Result<(), Fail> {
        Self::do_ioctl(s, control_code, input, output)
    }

    /// Implementation of setsockopt.
    pub(super) unsafe fn do_setsockopt<'a, T>(s: SOCKET, level: i32, opt: i32, val: Option<&'a T>) -> Result<(), Fail> {
        let val: Option<&'a [u8]> = match val {
            Some(val) => {
                Some(unsafe { std::slice::from_raw_parts((val as *const T).cast(), std::mem::size_of::<T>()) })
            },
            None => None,
        };

        if unsafe { setsockopt(s, level, opt, val) } == 0 {
            Ok(())
        } else {
            Err(expect_last_wsa_error())
        }
    }

    /// Set a socket option (via setsockopt) from a structured value `val`.
    /// Safety: the requested socket option must agree with the ABI of T.
    #[allow(unused)]
    pub unsafe fn setsockopt<'a, T>(&self, s: SOCKET, level: i32, opt: i32, val: Option<&'a T>) -> Result<(), Fail> {
        Self::do_setsockopt(s, level, opt, val)
    }

    /// Implementation of getsockopt.
    pub(super) unsafe fn do_getsockopt<T>(s: SOCKET, level: i32, optname: i32) -> Result<T, Fail> {
        let mut out: MaybeUninit<T> = MaybeUninit::zeroed();
        let optval: PSTR = PSTR::from_raw(out.as_mut_ptr().cast());
        let mut optlen: i32 =
            i32::try_from(std::mem::size_of::<T>()).map_err(|_| Fail::new(libc::E2BIG, "option type too large"))?;
        if unsafe { getsockopt(s, level, optname, optval, &mut optlen) } == 0 {
            Ok(unsafe { out.assume_init() })
        } else {
            Err(expect_last_wsa_error())
        }
    }

    /// Get a socket option (via getsockopt) and return the option as a structure type.
    /// Safety: the requested socket option must initialize a value of type T.
    pub unsafe fn getsockopt<T>(&self, s: SOCKET, level: i32, optname: i32) -> Result<T, Fail> {
        Self::do_getsockopt(s, level, optname)
    }

    /// Get or initialize a new `SocketExtensions` instance for a  socket. Extensions are stored by socket provider,
    /// which may be shared by multiple sockets.
    fn get_or_init_extensions(&mut self, s: SOCKET) -> Result<Rc<SocketExtensions>, Fail> {
        let protocol: WSAPROTOCOL_INFOW = unsafe { self.getsockopt(s, SOL_SOCKET, SO_PROTOCOL_INFOW) }?;

        let extensions: &mut Weak<SocketExtensions> = self
            .extensions_by_provider
            .entry(protocol.ProviderId)
            .or_insert(Weak::default());
        if let Some(extensions) = extensions.upgrade() {
            return Ok(extensions);
        }

        let new_extensions: Rc<SocketExtensions> = SocketExtensions::new(s)?;
        *extensions = Rc::downgrade(&new_extensions);

        Ok(new_extensions)
    }

    /// Create a raw Winsock socket.
    pub(super) unsafe fn raw_socket(
        domain: libc::c_int,
        typ: libc::c_int,
        protocol: libc::c_int,
        protocol_info: Option<&WSAPROTOCOL_INFOW>,
        flags: u32,
    ) -> Result<SOCKET, Fail> {
        let protocol_info: Option<*const WSAPROTOCOL_INFOW> = protocol_info.map(|i| i as *const WSAPROTOCOL_INFOW);
        match unsafe { WSASocketW(domain, typ, protocol, protocol_info, 0, flags) } {
            INVALID_SOCKET => Err(expect_last_wsa_error()),
            socket => Ok(socket),
        }
    }

    /// Create a new socket.
    pub fn socket(
        &mut self,
        domain: libc::c_int,
        typ: libc::c_int,
        protocol: libc::c_int,
        config: &WinConfig,
        iocp: &IoCompletionPort,
    ) -> Result<Socket, Fail> {
        // Safety: SOCKET is a loose handle; it must be closed with `closesocket` to clean up resources. Socket struct
        // will take ownership by end of method; failures after this call need to be cause a `closesocket` call.
        let s: SOCKET = unsafe { Self::raw_socket(domain, typ, protocol, None, WSA_FLAG_OVERLAPPED) }?;

        self.get_or_init_extensions(s)
            .and_then(|extensions: Rc<SocketExtensions>| Socket::new(s, protocol, config, extensions, iocp))
            .or_else(|err: Fail| {
                unsafe { closesocket(s) };
                Err(err)
            })
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

impl Drop for WinsockRuntime {
    fn drop(&mut self) {
        self.extensions_by_provider.clear();
        unsafe { WSACleanup() };
    }
}
