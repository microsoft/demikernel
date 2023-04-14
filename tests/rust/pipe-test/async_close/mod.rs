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

    crate::collect!(result, crate::test!(async_close_invalid_pipe(libos)));

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
