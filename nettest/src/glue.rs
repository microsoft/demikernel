// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::{
    fmt::Debug,
    net::SocketAddr,
    time::Duration,
};

//======================================================================================================================

#[derive(Clone, Debug)]
pub struct Event {
    pub time: Duration,
    pub action: Action,
}

#[derive(Clone, Debug)]
pub enum Action {
    SyscallEvent(SyscallEvent),
    PacketEvent(PacketEvent),
}

#[derive(Clone, Debug)]
pub struct SyscallEvent {
    pub end_time: Option<Duration>,
    pub syscall: DemikernelSyscall,
}

#[derive(Clone)]
#[allow(dead_code)]
pub enum DemikernelSyscall {
    Socket(SocketArgs, i32),
    Bind(BindArgs, i32),
    Listen(ListenArgs, i32),
    Accept(AcceptArgs, i32),
    Connect(ConnectArgs, i32),
    Push(PushArgs, i32),
    Pop(i32),
    Close(CloseArgs, i32),
    Wait(WaitArgs, i32),
    Unsupported,
}

impl Debug for DemikernelSyscall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DemikernelSyscall::Socket(args, _qd) => write!(f, "demi_socket({:?})", args),
            DemikernelSyscall::Bind(args, _ok) => write!(f, "demi_bind({:?})", args),
            DemikernelSyscall::Listen(args, _ok) => write!(f, "demi_listen({:?})", args),
            DemikernelSyscall::Accept(args, _qd) => write!(f, "demi_accept({:?})", args),
            DemikernelSyscall::Connect(args, _ok) => write!(f, "demi_connect({:?})", args),
            DemikernelSyscall::Push(args, _ret) => write!(f, "demi_push({:?})", args),
            DemikernelSyscall::Pop(_ret) => write!(f, "demi_pop"),
            DemikernelSyscall::Close(args, _ret) => write!(f, "demi_close({:?})", args),
            DemikernelSyscall::Wait(args, _ret) => write!(f, "demi_wait({:?})", args),
            DemikernelSyscall::Unsupported => write!(f, "Unsupported"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub enum SocketDomain {
    AF_INET,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub enum SocketType {
    SOCK_STREAM,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub enum SocketProtocol {
    IPPROTO_TCP,
}

#[derive(Clone, Debug)]
pub struct SocketArgs {
    pub domain: SocketDomain,
    pub typ: SocketType,
    pub protocol: SocketProtocol,
}

#[derive(Clone, Debug)]
pub struct BindArgs {
    pub qd: Option<u32>,
    pub addr: Option<SocketAddr>,
}

#[derive(Clone, Debug)]
pub struct ListenArgs {
    pub qd: Option<u32>,
    pub backlog: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct AcceptArgs {
    pub qd: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct ConnectArgs {
    pub qd: Option<u32>,
    pub addr: Option<SocketAddr>,
}

#[derive(Clone, Debug)]
pub struct PushArgs {
    pub qd: Option<u32>,
    pub buf: Option<Vec<u8>>,
    pub len: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct WaitArgs {
    pub qd: Option<u32>,
    pub timeout: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct CloseArgs {
    pub qd: u32,
}

#[derive(Clone, Debug)]
pub enum PacketEvent {
    Tcp(PacketDirection, TcpPacket),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PacketDirection {
    Incoming,
    Outgoing,
}

#[derive(Clone, Debug)]
pub struct TcpPacket {
    pub flags: TcpFlags,
    pub seqnum: TcpSequenceNumber,
    pub ack: Option<u32>,
    pub win: Option<u32>,
    pub options: Vec<TcpOption>,
}
#[derive(Clone)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub fin: bool,
    pub rst: bool,
    pub psh: bool,
    pub urg: bool,
    pub ece: bool,
    pub cwr: bool,
}

#[derive(Clone, Debug)]
pub struct TcpSequenceNumber {
    pub seq: u32,
    pub win: u32,
}

#[derive(Clone, Debug)]
pub enum TcpOption {
    Noop,
    Mss(u16),
    WindowScale(u8),
    SackOk,
    Timestamp(u32, u32),
    EndOfOptions,
}

impl Debug for TcpFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut flags = String::new();
        if self.syn {
            flags.push_str("SYN ");
        }
        if self.ack {
            flags.push_str("ACK ");
        }
        if self.fin {
            flags.push_str("FIN ");
        }
        if self.rst {
            flags.push_str("RST ");
        }
        if self.psh {
            flags.push_str("PSH ");
        }
        if self.urg {
            flags.push_str("URG ");
        }
        if self.ece {
            flags.push_str("ECE ");
        }
        if self.cwr {
            flags.push_str("CWR ");
        }
        write!(f, "{}", flags)
    }
}

pub fn parse_int(s: &str) -> Result<u32, ()> {
    match s.parse::<u32>() {
        Ok(val) => Ok(val),
        Err(_) => {
            eprintln!("{} cannot be represented as a u32", s);
            Err(())
        },
    }
}

pub fn parse_ret_code(s: &str) -> Result<i32, ()> {
    if let Ok(val) = s.parse::<i32>() {
        Ok(val)
    } else {
        // Try to convert to an errno value.
        match s {
            "EPERM" => Ok(-libc::EPERM),
            "ENOENT" => Ok(-libc::ENOENT),
            "ESRCH" => Ok(-libc::ESRCH),
            "EINTR" => Ok(-libc::EINTR),
            "EIO" => Ok(-libc::EIO),
            "ENXIO" => Ok(-libc::ENXIO),
            "E2BIG" => Ok(-libc::E2BIG),
            "ENOEXEC" => Ok(-libc::ENOEXEC),
            "EBADF" => Ok(-libc::EBADF),
            "ECHILD" => Ok(-libc::ECHILD),
            "EAGAIN" => Ok(-libc::EAGAIN),
            "ENOMEM" => Ok(-libc::ENOMEM),
            "EACCES" => Ok(-libc::EACCES),
            "EFAULT" => Ok(-libc::EFAULT),
            "EBUSY" => Ok(-libc::EBUSY),
            "EEXIST" => Ok(-libc::EEXIST),
            "EXDEV" => Ok(-libc::EXDEV),
            "ENODEV" => Ok(-libc::ENODEV),
            "ENOTDIR" => Ok(-libc::ENOTDIR),
            "EISDIR" => Ok(-libc::EISDIR),
            "EINVAL" => Ok(-libc::EINVAL),
            "ENFILE" => Ok(-libc::ENFILE),
            "EMFILE" => Ok(-libc::EMFILE),
            "ENOTTY" => Ok(-libc::ENOTTY),
            "EFBIG" => Ok(-libc::EFBIG),
            "ENOSPC" => Ok(-libc::ENOSPC),
            "ESPIPE" => Ok(-libc::ESPIPE),
            "EROFS" => Ok(-libc::EROFS),
            "EMLINK" => Ok(-libc::EMLINK),
            "EPIPE" => Ok(-libc::EPIPE),
            "EDOM" => Ok(-libc::EDOM),
            "ERANGE" => Ok(-libc::ERANGE),
            "EDEADLK" => Ok(-libc::EDEADLK),
            "EDEADLOCK" => Ok(-libc::EDEADLOCK),
            "ENAMETOOLONG" => Ok(-libc::ENAMETOOLONG),
            "ENOLCK" => Ok(-libc::ENOLCK),
            "ENOSYS" => Ok(-libc::ENOSYS),
            "ENOTEMPTY" => Ok(-libc::ENOTEMPTY),
            "EILSEQ" => Ok(-libc::EILSEQ),
            "EADDRINUSE" => Ok(-libc::EADDRINUSE),
            "EADDRNOTAVAIL" => Ok(-libc::EADDRNOTAVAIL),
            "EAFNOSUPPORT" => Ok(-libc::EAFNOSUPPORT),
            "EALREADY" => Ok(-libc::EALREADY),
            "EBADMSG" => Ok(-libc::EBADMSG),
            "ECANCELED" => Ok(-libc::ECANCELED),
            "ECONNABORTED" => Ok(-libc::ECONNABORTED),
            "ECONNREFUSED" => Ok(-libc::ECONNREFUSED),
            "ECONNRESET" => Ok(-libc::ECONNRESET),
            "EDESTADDRREQ" => Ok(-libc::EDESTADDRREQ),
            "EHOSTUNREACH" => Ok(-libc::EHOSTUNREACH),
            "EIDRM" => Ok(-libc::EIDRM),
            "EINPROGRESS" => Ok(-libc::EINPROGRESS),
            "EISCONN" => Ok(-libc::EISCONN),
            "ELOOP" => Ok(-libc::ELOOP),
            "EMSGSIZE" => Ok(-libc::EMSGSIZE),
            "ENETDOWN" => Ok(-libc::ENETDOWN),
            "ENETRESET" => Ok(-libc::ENETRESET),
            "ENETUNREACH" => Ok(-libc::ENETUNREACH),
            "ENOBUFS" => Ok(-libc::ENOBUFS),
            "ENODATA" => Ok(-libc::ENODATA),
            "ENOLINK" => Ok(-libc::ENOLINK),
            "ENOMSG" => Ok(-libc::ENOMSG),
            "ENOPROTOOPT" => Ok(-libc::ENOPROTOOPT),
            "ENOSR" => Ok(-libc::ENOSR),
            "ENOSTR" => Ok(-libc::ENOSTR),
            "ENOTCONN" => Ok(-libc::ENOTCONN),
            "ENOTRECOVERABLE" => Ok(-libc::ENOTRECOVERABLE),
            "ENOTSOCK" => Ok(-libc::ENOTSOCK),
            "ENOTSUP" => Ok(-libc::ENOTSUP),
            "EOPNOTSUPP" => Ok(-libc::EOPNOTSUPP),
            "EOVERFLOW" => Ok(-libc::EOVERFLOW),
            "EOWNERDEAD" => Ok(-libc::EOWNERDEAD),
            "EPROTO" => Ok(-libc::EPROTO),
            "EPROTONOSUPPORT" => Ok(-libc::EPROTONOSUPPORT),
            "EPROTOTYPE" => Ok(-libc::EPROTOTYPE),
            "ETIME" => Ok(-libc::ETIME),
            "ETIMEDOUT" => Ok(-libc::ETIMEDOUT),
            "ETXTBSY" => Ok(-libc::ETXTBSY),
            "EWOULDBLOCK" => Ok(-libc::EWOULDBLOCK),
            _ => panic!("Unknown error code"),
        }
    }
}
