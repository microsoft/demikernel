// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    QDesc,
};

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for pipe queues.
pub fn run(libos: &mut LibOS, pipe_name: &str) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    demikernel::collect_test!(result, demikernel::run_test!(close_invalid_pipe(libos)));
    demikernel::collect_test!(
        result,
        demikernel::run_test!(close_pipe_multiple_times(libos, pipe_name))
    );

    result
}

/// Attempts to close an invalid pipe.
fn close_invalid_pipe(libos: &mut LibOS) -> Result<()> {
    // Fail to close an invalid pipe.
    match libos.close(QDesc::from(0)) {
        Err(e) if e.errno == libc::EBADF => (),
        Ok(_) => anyhow::bail!("close() invalid pipe should fail"),
        Err(e) => anyhow::bail!("close() failed ({})", e),
    };

    // Fail to close an invalid pipe.
    match libos.close(QDesc::from(u32::MAX)) {
        Err(e) if e.errno == libc::EBADF => (),
        Ok(_) => anyhow::bail!("close() invalid pipe should fail"),
        Err(e) => anyhow::bail!("close() failed ({})", e),
    };

    Ok(())
}

/// Attempts to close the same pipe multiple times.
fn close_pipe_multiple_times(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    // Create a pipe.
    let pipeqd: QDesc = match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => pipeqd,
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    };

    // Succeed to close the pipe.
    match libos.close(pipeqd) {
        Ok(_) => (),
        Err(e) => {
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
            anyhow::bail!("close() failed ({})", e);
        },
    };

    // Fail to close the pipe again.
    match libos.close(pipeqd) {
        Err(e) if e.errno == libc::EBADF => Ok(()),
        Ok(_) => anyhow::bail!("close() invalid pipe should fail"),
        Err(e) => anyhow::bail!("close() failed ({})", e),
    }
}
