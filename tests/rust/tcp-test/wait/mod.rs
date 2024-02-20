// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::check_for_network_error;
use ::anyhow::Result;
use ::demikernel::{
    runtime::types::demi_opcode_t,
    LibOS,
    QDesc,
    QToken,
};
use ::std::{
    net::SocketAddr,
    time::Duration,
};

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

/// Drives integration tests for close() on TCP sockets.
pub fn run(libos: &mut LibOS, addr: &SocketAddr) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    crate::collect!(result, crate::test!(wait_after_close_accepting_socket(libos, addr)));
    crate::collect!(result, crate::test!(wait_after_close_connecting_socket(libos, addr)));
    crate::collect!(
        result,
        crate::test!(wait_after_async_close_accepting_socket(libos, addr))
    );
    crate::collect!(
        result,
        crate::test!(wait_after_async_close_connecting_socket(libos, addr))
    );
    crate::collect!(result, crate::test!(wait_on_invalid_queue_token_returns_einval(libos)));
    crate::collect!(
        result,
        crate::test!(wait_for_accept_after_issuing_async_close(libos, addr))
    );
    crate::collect!(
        result,
        crate::test!(wait_for_connect_after_issuing_async_close(libos, addr))
    );

    result
}

// Attempts to close a TCP socket that is accepting and then waits on the qtoken.
fn wait_after_close_accepting_socket(libos: &mut LibOS, local: &SocketAddr) -> Result<()> {
    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should not succeed on accept()"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the accept() coroutine returns an err and was properly canceled.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if check_for_network_error(&qr) => {},
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
            "wait() should succeed with a specified error on accept() after close(), instead returned this unknown \
             error: {:?}",
            qr.qr_ret
        ),
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should return an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to close a TCP socket that is connecting and then waits on the qtoken.
fn wait_after_close_connecting_socket(libos: &mut LibOS, remote: &SocketAddr) -> Result<()> {
    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, *remote)?;
    let mut connect_finished: bool = false;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // Can only complete with ECONNREFUSED because remote does not exist.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && (qr.qr_ret == libc::ECONNREFUSED as i64) => {
            connect_finished = true
        },
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNABORTED as i64 => {
            connect_finished = true
        },
        // If completes successfully, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
            anyhow::bail!("connect() should not succeed because remote does not exist")
        },
        Ok(qr) => {
            anyhow::bail!(
                "wait() should not succeed on connect(): opcode {:?} ret {:?}",
                qr.qr_opcode,
                qr.qr_ret
            )
        },
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    if !connect_finished {
        // Poll again to check that the connect() co-routine returns an err, either canceled or refused.
        match libos.wait(qt, Some(Duration::from_micros(0))) {
            Ok(qr) if check_for_network_error(&qr) => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
                "wait() should succeed with a specified error on connect() after close(), instead returned this \
                 unknown error: {:?}",
                qr.qr_ret
            ),
            // If connect() completes successfully, something has gone wrong.
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
                anyhow::bail!("connect() should not succeed because remote does not exist")
            },
            Ok(_) => anyhow::bail!("wait() should return an error on connect() after close()"),
            Err(_) => anyhow::bail!("wait() should not time out"),
        }
    }

    Ok(())
}

// Attempts to close a TCP socket that is accepting and then waits on the queue token.
fn wait_after_async_close_accepting_socket(libos: &mut LibOS, local: &SocketAddr) -> Result<()> {
    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should not succeed with accept()"),
        Err(_) => anyhow::bail!("wait() should timeout with accept()"),
    }

    // Succeed to close socket.
    let qt_close: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt_close, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }

    // Poll again to check that the accept() co-routine completed with an error and was properly canceled.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if check_for_network_error(&qr) => {},
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
            "wait() should succeed with a specified error on accept() after close(), instead returned this unknown \
             error: {:?}",
            qr.qr_ret
        ),
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(_) => anyhow::bail!("wait() should return an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to close a TCP socket that is connecting and then waits on the queue token.
fn wait_after_async_close_connecting_socket(libos: &mut LibOS, remote: &SocketAddr) -> Result<()> {
    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, *remote)?;
    let mut connect_finished: bool = false;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // Can only complete with ECONNREFUSED or ECONNABORTED because remote does not exist.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNREFUSED as i64 => {
            connect_finished = true
        },
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNABORTED as i64 => {
            connect_finished = true
        },

        // If connect() completes successfully, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
            anyhow::bail!("connect() should not succeed because remote does not exist")
        },
        Ok(qr) => anyhow::bail!(
            "wait() should not succeed with connect(): opcode {:?} error {:?}",
            qr.qr_opcode,
            qr.qr_ret
        ),
        Err(_) => anyhow::bail!("wait() should timeout with connect()"),
    }

    // Succeed to close socket.
    let qt_close: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt_close, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed"),
    }

    if !connect_finished {
        // Poll again to check that the connect() co-routine completed with an error, either canceled or refused.
        match libos.wait(qt, Some(Duration::from_micros(0))) {
            Ok(qr) if check_for_network_error(&qr) => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
                "wait() should succeed with a specified error on connect() after async_close(), instead returned this \
                 unknown error: {:?}",
                qr.qr_ret
            ),
            // If connect() completes successfully, something has gone wrong.
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
                anyhow::bail!("connect() should not succeed because remote does not exist")
            },
            Ok(_) => anyhow::bail!("wait() should return an error on connect() after async_close()"),
            Err(_) => anyhow::bail!("wait() should not time out"),
        }
    }

    Ok(())
}

// Attempt to wait on an invalid queue token.
fn wait_on_invalid_queue_token_returns_einval(libos: &mut LibOS) -> Result<()> {
    // Wait on an invalid queue token made from u64 MAX value.
    match libos.wait(QToken::from(u64::MAX), Some(Duration::from_micros(0))) {
        Ok(_) => anyhow::bail!("wait() should not succeed on invalid token"),
        Err(e) if e.errno == libc::EINVAL => {},
        Err(_) => anyhow::bail!("wait() should not fail with any other reason than invalid token"),
    }

    // Wait on an invalid queue token made from 0 value.
    match libos.wait(QToken::from(0), Some(Duration::from_micros(0))) {
        Ok(_) => anyhow::bail!("wait() should not succeed on invalid token"),
        Err(e) if e.errno == libc::EINVAL => {},
        Err(_) => anyhow::bail!("wait() should not fail with any other reason than invalid token"),
    }

    Ok(())
}

// Attempt to wait for an accept() operation to complete after issuing an asynchronous close on a socket.
fn wait_for_accept_after_issuing_async_close(libos: &mut LibOS, local: &SocketAddr) -> Result<()> {
    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;
    let mut accepted_completed: bool = false;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
        Ok(_) => anyhow::bail!("wait() should not succeed with accept()"),
        Err(_) => anyhow::bail!("wait() should timeout with accept()"),
    }

    let qt_close: QToken = libos.async_close(sockqd)?;

    // Wait again on accept() and ensure that ETIMEDOUT is returned.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // If we found a connection to accept, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
            anyhow::bail!("accept() should not succeed because remote should not be connecting")
        },
        Ok(qr) if check_for_network_error(&qr) => accepted_completed = true,
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
            "accept should fail with a specified error, instead returned this unknown error: {:?}",
            qr.qr_ret
        ),
        Ok(_) => anyhow::bail!("wait() should not succeed with accept()"),
        Err(_) => anyhow::bail!("wait() should timeout with accept()"),
    }

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt_close, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }

    // Wait again on accept() and ensure it fails or gets cancelled.
    if !accepted_completed {
        match libos.wait(qt, Some(Duration::from_micros(0))) {
            Ok(qr) if check_for_network_error(&qr) => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
                "wait() should succeed with a specified error on accept() after async_close(), instead returned this \
                 unknown error: {:?}",
                qr.qr_ret
            ),
            // If we found a connection to accept, something has gone wrong.
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT && qr.qr_ret == 0 => {
                anyhow::bail!("accept() should not succeed because remote should not be connecting")
            },
            Ok(_) => anyhow::bail!("wait() should return an error on accept() after async_close()"),
            Err(e) => anyhow::bail!("wait() should not time out. {:?}", e),
        }
    }

    Ok(())
}

// Attempt to wait for a connect() operation to complete complete after asynchronous close on a socket.
fn wait_for_connect_after_issuing_async_close(libos: &mut LibOS, remote: &SocketAddr) -> Result<()> {
    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, *remote)?;
    let mut connect_finished: bool = false;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        // Can only complete with ECONNREFUSED because remote does not exist.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNREFUSED as i64 => {
            connect_finished = true
        },
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECONNABORTED as i64 => {
            connect_finished = true
        },
        // If connect() completes successfully, something has gone wrong.
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
            anyhow::bail!("connect() should not succeed because remote does not exist")
        },
        Ok(_) => anyhow::bail!("wait() should not succeed with connect()"),
        Err(_) => anyhow::bail!("wait() should timeout with connect()"),
    }

    let qt_close: QToken = libos.async_close(sockqd)?;

    if !connect_finished {
        // Wait again on connect() and ensure it fails or gets cancelled.
        match libos.wait(qt, Some(Duration::from_micros(0))) {
            Ok(qr) if check_for_network_error(&qr) => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!(
                "wait() should succeed with a specified error on connect() after async(), instead returned this \
                 unknown error: {:?}",
                qr.qr_ret
            ),
            // If connect() completes successfully, something has gone wrong.
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT && qr.qr_ret == 0 => {
                anyhow::bail!("connect() should not succeed because remote does not exist")
            },
            Ok(_) => anyhow::bail!("wait() should return an error on connect() after async_close()"),
            Err(_) => anyhow::bail!("wait() should not time out"),
        }
    }

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt_close, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(e) => anyhow::bail!("wait() should succeed. {:?}", e),
    }

    Ok(())
}
