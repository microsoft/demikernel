// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{
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
    Socket(SocketArgs, u32),
    Bind(BindArgs, u32),
    Listen(ListenArgs, u32),
    Accept(AcceptArgs, u32),
    Connect(ConnectArgs, u32),
    Push(PushArgs, u32),
    Pop(u32),
    Close,
    Wait(WaitArgs, u32),
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
            DemikernelSyscall::Close => write!(f, "demi_close"),
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
