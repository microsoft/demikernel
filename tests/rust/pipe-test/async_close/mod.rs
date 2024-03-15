// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    runtime::types::demi_opcode_t,
    LibOS,
    QDesc,
    QToken,
};
use ::std::time::Duration;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for pipe queues.
pub fn run(libos: &mut LibOS, pipe_name: &str) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    demikernel::collect_test!(result, demikernel::run_test!(async_close_invalid_pipe(libos)));
    demikernel::collect_test!(
        result,
        demikernel::run_test!(async_close_pipe_multiple_times_1(libos, pipe_name))
    );
    demikernel::collect_test!(
        result,
        demikernel::run_test!(async_close_pipe_multiple_times_2(libos, pipe_name))
    );
    demikernel::collect_test!(
        result,
        demikernel::run_test!(async_close_pipe_multiple_times_3(libos, pipe_name))
    );

    result
}

/// Attempts to asynchronously close an invalid pipe.
fn async_close_invalid_pipe(libos: &mut LibOS) -> Result<()> {
    // Fail to close an invalid pipe.
    match libos.async_close(QDesc::from(0)) {
        Err(e) if e.errno == libc::EBADF => (),
        Ok(_) => anyhow::bail!("async_close() invalid pipe should fail"),
        Err(e) => anyhow::bail!("async_close() failed ({})", e),
    };

    // Fail to close an invalid pipe.
    match libos.async_close(QDesc::from(u32::MAX)) {
        Err(e) if e.errno == libc::EBADF => (),
        Ok(_) => anyhow::bail!("async_close() invalid pipe should fail"),
        Err(e) => anyhow::bail!("async_close() failed ({})", e),
    };

    Ok(())
}

/// Attempts to asynchronously close the same pipe multiple times.
fn async_close_pipe_multiple_times_1(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    // Create a pipe.
    let pipeqd: QDesc = match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => pipeqd,
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    };

    // Succeed to issue asynchronous close.
    let qt: QToken = match libos.async_close(pipeqd) {
        Ok(qt) => qt,
        Err(e) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("async_close() failed ({})", e);
        },
    };

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Err(e) if e.errno == libc::ETIMEDOUT => {
            anyhow::bail!("wait() async_close() should not timeout")
        },
        Ok(_) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("wait() should succeed with async_close()")
        },
        Err(_) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("wait() should succeed with async_close()")
        },
    }

    // Fail to close the pipe again.
    match libos.async_close(pipeqd) {
        Err(e) if e.errno == libc::EBADF => Ok(()),
        Ok(_) => anyhow::bail!("async_close() a pipe multiple times should fail"),
        Err(e) => anyhow::bail!("async_close() failed ({})", e),
    }
}

/// Attempts to asynchronously close the same pipe multiple times.
fn async_close_pipe_multiple_times_2(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    let mut cancelled: bool = false;
    let mut closed: bool = false;

    // Create a pipe.
    let pipeqd: QDesc = match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => pipeqd,
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    };

    // Succeed to issue asynchronous close.
    let qt1: QToken = match libos.async_close(pipeqd) {
        Ok(qt) => qt,
        Err(e) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("async_close() failed ({})", e);
        },
    };

    // Succeed to issue asynchronous close.
    let qt2: Option<QToken> = match libos.async_close(pipeqd) {
        Ok(qt) => Some(qt),
        Err(e) if e.errno == libc::EBADF => {
            cancelled = true;
            None
        },
        Err(e) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("async_close() failed ({})", e);
        },
    };

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt1, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => closed = true,
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => cancelled = true,
        Ok(_) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("wait() should succeed with async_close()")
        },
        Err(_) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("wait() should succeed with async_close()")
        },
    }

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    if let Some(qt2) = qt2 {
        match libos.wait(qt2, Some(Duration::from_micros(0))) {
            Ok(qr) => {
                if cancelled && qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 {
                    return Ok(());
                }
                if closed && qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 {
                    return Ok(());
                }
                anyhow::bail!("wait() should succeed with async_close()")
            },
            Err(_) => {
                anyhow::bail!("wait() should succeed with async_close()")
            },
        }
    }

    Ok(())
}

/// Attempts to asynchronously close the same pipe multiple times.
fn async_close_pipe_multiple_times_3(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    let mut cancelled: bool = false;
    let mut closed: bool = false;

    // Create a pipe.
    let pipeqd: QDesc = match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => pipeqd,
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    };

    // Succeed to issue asynchronous close.
    let qt1: QToken = match libos.async_close(pipeqd) {
        Ok(qt) => qt,
        Err(e) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("async_close() failed ({})", e);
        },
    };

    // Succeed to issue asynchronous close.
    let qt2: Option<QToken> = match libos.async_close(pipeqd) {
        Ok(qt) => Some(qt),
        Err(e) if e.errno == libc::EBADF => {
            cancelled = true;
            None
        },
        Err(e) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("async_close() failed ({})", e);
        },
    };

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    if let Some(qt2) = qt2 {
        match libos.wait(qt2, Some(Duration::from_micros(0))) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => closed = true,
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {
                cancelled = true
            },
            Ok(_) => {
                println!("[ERROR] leaking pipeqd={:?}", pipeqd);
                anyhow::bail!("wait() should succeed with async_close()")
            },
            Err(_) => {
                println!("[ERROR] leaking pipeqd={:?}", pipeqd);
                anyhow::bail!("wait() should succeed with async_close()")
            },
        }
    }

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt1, Some(Duration::from_micros(0))) {
        Ok(qr) => {
            if cancelled && qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 {
                return Ok(());
            }
            if closed && qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 {
                return Ok(());
            }
            anyhow::bail!("wait() should succeed with async_close()")
        },
        Err(_) => {
            anyhow::bail!("wait() should succeed with async_close()")
        },
    }
}
