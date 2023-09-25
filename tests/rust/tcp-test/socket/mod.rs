// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::LibOS;

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

//======================================================================================================================
// Constants
//======================================================================================================================

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for socket() on TCP sockets.
pub fn run(libos: &mut LibOS) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    crate::collect!(result, crate::test!(create_socket_using_unsupported_domain(libos)));
    crate::collect!(result, crate::test!(create_socket_using_unsupported_type(libos)));

    result
}

/// Attempts to create a TCP socket using an unsupported domain.
fn create_socket_using_unsupported_domain(libos: &mut LibOS) -> Result<()> {
    // Unsupported domains in Linux.
    #[cfg(target_os = "linux")]
    let domains: Vec<libc::c_int> = vec![
        libc::AF_ALG,
        libc::AF_APPLETALK,
        libc::AF_ASH,
        libc::AF_ATMPVC,
        libc::AF_ATMSVC,
        libc::AF_AX25,
        libc::AF_BLUETOOTH,
        libc::AF_BRIDGE,
        libc::AF_CAIF,
        libc::AF_CAN,
        libc::AF_DECnet,
        libc::AF_ECONET,
        libc::AF_IB,
        libc::AF_IEEE802154,
        // libc::AF_INET,
        libc::AF_INET6,
        libc::AF_IPX,
        libc::AF_IRDA,
        libc::AF_ISDN,
        libc::AF_IUCV,
        libc::AF_KEY,
        libc::AF_LLC,
        libc::AF_LOCAL,
        libc::AF_MPLS,
        libc::AF_NETBEUI,
        libc::AF_NETLINK,
        libc::AF_NETROM,
        libc::AF_NFC,
        libc::AF_PACKET,
        libc::AF_PHONET,
        libc::AF_PPPOX,
        libc::AF_RDS,
        libc::AF_ROSE,
        libc::AF_ROUTE,
        libc::AF_RXRPC,
        libc::AF_SECURITY,
        libc::AF_SNA,
        libc::AF_TIPC,
        libc::AF_UNIX,
        libc::AF_UNSPEC,
        libc::AF_VSOCK,
        libc::AF_WANPIPE,
        libc::AF_X25,
        libc::AF_XDP,
    ];

    // Unsupported domains in Windows.
    #[cfg(target_os = "windows")]
    let domains: Vec<libc::c_int> = vec![
        WinSock::AF_APPLETALK as i32,
        WinSock::AF_DECnet as i32,
        // WinSock::AF_INET as i32,
        WinSock::AF_INET6.0 as i32,
        WinSock::AF_IPX as i32,
        WinSock::AF_IRDA as i32,
        WinSock::AF_SNA as i32,
        WinSock::AF_UNIX as i32,
        WinSock::AF_UNSPEC.0 as i32,
    ];

    // Attempt to create a TCP socket with all unsupported domains.
    for domain in domains {
        // Fail to create socket.
        match libos.socket(domain, SOCK_STREAM, 0) {
            Err(e) if e.errno == libc::ENOTSUP => (),
            Err(e) => anyhow::bail!("sockeet() failed with {}", e),
            Ok(_) => anyhow::bail!("create a TCP socket with an unsupported domain should fail"),
        }
    }

    Ok(())
}

/// Attempts to create a TCP socket using an unsupported socket type.
fn create_socket_using_unsupported_type(libos: &mut LibOS) -> Result<()> {
    // Invalid socket types in Linux.
    #[cfg(target_os = "linux")]
    let socket_types: Vec<libc::c_int> = vec![
        libc::SOCK_DCCP,
        // libc::SOCK_DGRAM,
        libc::SOCK_PACKET,
        libc::SOCK_RAW,
        libc::SOCK_RDM,
        libc::SOCK_SEQPACKET,
        // libc::SOCK_STREAM,
    ];

    // Invalid socket types in Windows.
    #[cfg(target_os = "windows")]
    let socket_types: Vec<libc::c_int> = vec![
        WinSock::SOCK_RAW.0 as i32,
        WinSock::SOCK_RDM.0 as i32,
        WinSock::SOCK_SEQPACKET.0 as i32,
    ];

    // Attempt to create a TCP socket with all invalid socket types.
    for socket_type in socket_types {
        // Fail to create socket.
        match libos.socket(AF_INET, socket_type, 0) {
            Err(e) if e.errno == libc::ENOTSUP => (),
            Err(e) => anyhow::bail!("sockeet() failed with {}", e),
            Ok(_) => anyhow::bail!("create a TCP socket with invalid socket type should fail"),
        }
    }

    Ok(())
}
