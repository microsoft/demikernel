// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    runtime::types::demi_opcode_t,
    LibOS,
    QDesc,
    QToken,
};
use std::{
    net::SocketAddrV4,
    time::Duration,
};

//======================================================================================================================
// Constants
//======================================================================================================================

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Runs standalone tests.
pub fn run(libos: &mut LibOS, addr: &SocketAddrV4) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    crate::collect!(result, crate::test!(accept_invalid_queue_descriptor(libos)));
    crate::collect!(result, crate::test!(accept_unbound_socket(libos)));
    crate::collect!(result, crate::test!(accept_active_socket(libos, addr)));
    crate::collect!(result, crate::test!(accept_listening_socket(libos, addr)));
    crate::collect!(result, crate::test!(accept_connecting_socket(libos, addr)));
    crate::collect!(result, crate::test!(accept_accepting_socket(libos, addr)));
    crate::collect!(result, crate::test!(accept_closed_socket(libos, addr)));

    result
}

/// Attempts to accept connections on an invalid queue descriptor.
fn accept_invalid_queue_descriptor(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(accept_invalid_socket));

    // Fail to accept() connections.
    match libos.accept(QDesc::from(0)) {
        Err(e) if e.errno == libc::EBADF => (),
        Err(e) => anyhow::bail!("accept() failed with {}", e),
        Ok(_) => anyhow::bail!("accept() connections on an invalid socket should fail"),
    };

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is not bound.
fn accept_unbound_socket(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(accept_unbound_socket));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Fail to accept() connections.
    match libos.accept(sockqd) {
        Err(e) if e.errno == libc::EINVAL => (),
        Err(e) => anyhow::bail!("accept() failed with {}", e),
        Ok(_) => anyhow::bail!("accept() connections on a socket that is not bound should fail"),
    };

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is not listening.
fn accept_active_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_active_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Fail to accept() connections.
    match libos.accept(sockqd) {
        Err(e) if e.errno == libc::EINVAL => (),
        Err(e) => anyhow::bail!("accept() failed with {}", e),
        Ok(_) => anyhow::bail!("accept() connections on a socket that is not listening should fail"),
    };

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is listening.
fn accept_listening_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_listening_socket));

    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;
    libos.listen(sockqd, 16)?;

    // Succeed to accept() connections.
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the accept() returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should succeed with an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }
    Ok(())
}

/// Attempts to accept connections on a TCP socket that is connecting.
fn accept_connecting_socket(libos: &mut LibOS, remote: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_connecting_socket));

    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, remote.to_owned())?;
    let mut connect_finished: bool = false;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // Can only complete with ECONNREFUSED because remote does not exist.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNREFUSED => {
            connect_finished = true
        },
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should be cancelled"),
    }

    // Fail to accept() connections.
    match libos.accept(sockqd) {
        Err(e) if e.errno == libc::EINVAL => (),
        Err(e) => anyhow::bail!("accept() failed with {}", e),
        Ok(_) => anyhow::bail!("accept() connections on a socket that is closed should fail"),
    };

    // Succeed to close socket.
    libos.close(sockqd)?;

    if !connect_finished {
        // Poll again to check that the connect() returns an err.
        match libos.wait(qt, Some(Duration::from_micros(0))) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNREFUSED => {},
            // If connect() completes successfully, something has gone wrong.
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
                anyhow::bail!("connect() should not succeed because remote does not exist")
            },
            Ok(_) => anyhow::bail!("wait() should succeed with an error on connect() after close()"),
            Err(_) => anyhow::bail!("wait() should not time out"),
        }
    }

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is already accepting connections.
/// TODO: Check if this is actually not allowed.
fn accept_accepting_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_accepting_socket));

    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Fail to accept() connections.
    match libos.accept(sockqd) {
        Err(e) if e.errno == libc::EINPROGRESS => (),
        Err(e) => anyhow::bail!("accept() failed with {}", e),
        Ok(_) => anyhow::bail!("accept() connections on a socket that is accepting connections should fail"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the accept() returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should succeed with an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is closed.
fn accept_closed_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_closed_socket));

    // Create a closed socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;
    libos.listen(sockqd, 16)?;
    libos.close(sockqd)?;

    // Fail to accept() connections.
    match libos.accept(sockqd) {
        Err(e) if e.errno == libc::EBADF => Ok(()),
        Err(e) => anyhow::bail!("accept() failed with {}", e),
        Ok(_) => anyhow::bail!("accept() connections on a socket that is closed should fail"),
    }
}
