use windows::{
    core::{
        HRESULT,
        HSTRING,
    },
    Win32::{
        Foundation::{
            ERROR_ABANDONED_WAIT_0,
            ERROR_ACCESS_DENIED,
            ERROR_ALREADY_EXISTS,
            ERROR_INSUFFICIENT_BUFFER,
            ERROR_INVALID_HANDLE,
            ERROR_INVALID_PARAMETER,
            ERROR_IO_INCOMPLETE,
            ERROR_IO_PENDING,
            ERROR_MORE_DATA,
            ERROR_NOT_ENOUGH_MEMORY,
            ERROR_OPERATION_ABORTED,
            NTSTATUS,
            WIN32_ERROR,
        },
        Networking::WinSock::{
            self,
            WSAGetLastError,
            WSA_ERROR,
        },
        System::IO::OVERLAPPED,
    },
};

use crate::runtime::fail::Fail;

pub fn translate_wsa_error(err: WSA_ERROR) -> libc::errno_t {
    match err {
        // Direct mappings
        WinSock::WSAEINTR => libc::EINTR,
        WinSock::WSAEBADF => libc::EBADF,
        WinSock::WSAEACCES => libc::EACCES,
        WinSock::WSAEFAULT => libc::EFAULT,
        WinSock::WSAEINVAL => libc::EINVAL,
        WinSock::WSAEMFILE => libc::EMFILE,
        WinSock::WSAEWOULDBLOCK => libc::EWOULDBLOCK,
        WinSock::WSAEINPROGRESS => libc::EINPROGRESS,
        WinSock::WSAEALREADY => libc::EALREADY,
        WinSock::WSAENOTSOCK => libc::ENOTSOCK,
        WinSock::WSAEDESTADDRREQ => libc::EDESTADDRREQ,
        WinSock::WSAEMSGSIZE => libc::EMSGSIZE,
        WinSock::WSAEPROTOTYPE => libc::EPROTOTYPE,
        WinSock::WSAENOPROTOOPT => libc::ENOPROTOOPT,
        WinSock::WSAEPROTONOSUPPORT => libc::EPROTONOSUPPORT,
        WinSock::WSAEOPNOTSUPP => libc::EOPNOTSUPP,
        WinSock::WSAEAFNOSUPPORT => libc::EAFNOSUPPORT,
        WinSock::WSAEADDRINUSE => libc::EADDRINUSE,
        WinSock::WSAEADDRNOTAVAIL => libc::EADDRNOTAVAIL,
        WinSock::WSAENETDOWN => libc::ENETDOWN,
        WinSock::WSAENETUNREACH => libc::ENETUNREACH,
        WinSock::WSAENETRESET => libc::ENETRESET,
        WinSock::WSAECONNABORTED => libc::ECONNABORTED,
        WinSock::WSAECONNRESET => libc::ECONNRESET,
        WinSock::WSAENOBUFS => libc::ENOBUFS,
        WinSock::WSAEISCONN => libc::EISCONN,
        WinSock::WSAENOTCONN => libc::ENOTCONN,
        WinSock::WSAETIMEDOUT => libc::ETIMEDOUT,
        WinSock::WSAECONNREFUSED => libc::ECONNREFUSED,
        WinSock::WSAELOOP => libc::ELOOP,
        WinSock::WSAENAMETOOLONG => libc::ENAMETOOLONG,
        WinSock::WSAEHOSTUNREACH => libc::EHOSTUNREACH,
        WinSock::WSAENOTEMPTY => libc::ENOTEMPTY,
        WinSock::WSAESOCKTNOSUPPORT => libc::EPROTONOSUPPORT,
        WinSock::WSAEPFNOSUPPORT => libc::EPROTONOSUPPORT,

        // The following are missing from Rust libc
        WinSock::WSAESHUTDOWN => libc::EINVAL,       /*libc::ESHUTDOWN*/
        WinSock::WSAETOOMANYREFS => libc::EINVAL,    /*libc::ETOOMANYREFS*/
        WinSock::WSAEHOSTDOWN => libc::EHOSTUNREACH, /*libc::EHOSTDOWN*/
        WinSock::WSAEPROCLIM => libc::ENOMEM,        /*libc::EPROCLIM*/
        WinSock::WSAEUSERS => libc::ENOMEM,          /*libc::EUSERS*/
        WinSock::WSAEDQUOT => libc::ENOSPC,          /*libc::EDQUOT*/
        WinSock::WSAESTALE => libc::EINVAL,          /*libc::ESTALE*/
        WinSock::WSAEREMOTE => libc::EINVAL,         /*libc::EREMOTE*/

        // WSA-specific
        WinSock::WSANOTINITIALISED => libc::ENODEV,

        // Everything else.
        _ => libc::EINVAL,
    }
}

fn wsa_error_to_win_error(err: WSA_ERROR) -> windows::core::Error {
    let hresult: HRESULT = WIN32_ERROR(err.0 as u32).into();
    windows::core::Error::new(hresult, HSTRING::new())
}

pub fn last_overlapped_wsa_error() -> Result<(), Fail> {
    let wsa_error: WSA_ERROR = unsafe { WSAGetLastError() };
    if wsa_error.0 == ERROR_IO_PENDING.0 as i32 {
        Ok(())
    } else {
        let error: windows::core::Error = wsa_error_to_win_error(wsa_error);
        if error.code().is_ok() {
            Ok(())
        } else {
            Err(error.into())
        }
    }
}

pub fn get_overlapped_api_result(api_success: bool) -> Result<(), Fail> {
    if !api_success {
        last_overlapped_wsa_error()
    } else {
        Ok(())
    }
}

pub fn expect_last_wsa_error() -> Fail {
    // Safety: FFI; no major considerations.
    wsa_error_to_win_error(unsafe { WSAGetLastError() }).into()
}

pub fn get_last_wsa_error() -> Result<(), Fail> {
    let error: windows::core::Error = wsa_error_to_win_error(unsafe { WSAGetLastError() });
    if error.code().is_ok() {
        Ok(())
    } else {
        Err(error.into())
    }
}

/// Translate a small subset of Win32 error codes which we may be interested in distinguishing to errno_t.
pub fn translate_win32_error(error: WIN32_ERROR) -> libc::errno_t {
    match error {
        ERROR_ACCESS_DENIED => libc::EACCES,
        ERROR_NOT_ENOUGH_MEMORY => libc::ENOMEM,
        ERROR_ALREADY_EXISTS => libc::EEXIST,
        ERROR_INVALID_HANDLE => libc::EINVAL,
        ERROR_INVALID_PARAMETER => libc::EINVAL,
        ERROR_IO_INCOMPLETE => libc::EIO,
        ERROR_IO_PENDING => libc::EINPROGRESS,
        ERROR_OPERATION_ABORTED => libc::ECANCELED,
        ERROR_ABANDONED_WAIT_0 => libc::ECANCELED,
        ERROR_INSUFFICIENT_BUFFER => libc::EOVERFLOW,
        ERROR_MORE_DATA => libc::EOVERFLOW,
        _ => libc::EFAULT,
    }
}

pub fn get_result_from_overlapped(overlapped: &OVERLAPPED) -> Result<(), Fail> {
    let status: NTSTATUS = NTSTATUS(overlapped.Internal as i32);
    if status.is_ok() {
        Ok(())
    } else {
        Err(windows::core::Error::new(status.into(), HSTRING::new()).into())
    }
}

impl From<WSA_ERROR> for Fail {
    fn from(value: WSA_ERROR) -> Self {
        match get_last_wsa_error() {
            Ok(()) => Fail::new(0 as libc::errno_t, "operation completed successfully"),
            Err(err) => err.into(),
        }
    }
}

impl From<&windows::core::Error> for Fail {
    fn from(value: &windows::core::Error) -> Self {
        let cause: String = format!("{}", value);
        let errno: libc::errno_t = match WIN32_ERROR::from_error(value) {
            Some(error) => translate_win32_error(error),
            None => libc::EFAULT,
        };
        Fail::new(errno, cause.as_str())
    }
}

impl From<windows::core::Error> for Fail {
    fn from(value: windows::core::Error) -> Self {
        Self::from(&value)
    }
}
