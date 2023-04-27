// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    QToken,
};
use ::std::time::Duration;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for pipe queues.
pub fn run(libos: &mut LibOS) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    demikernel::collect_test!(result, demikernel::run_test!(wait_on_invalid_queue_token(libos)));

    result
}

// Attempts to wait on an invalid queue token.
fn wait_on_invalid_queue_token(libos: &mut LibOS) -> Result<()> {
    // Wait on an invalid queue token made from u64 MAX value.
    match libos.wait(QToken::from(u64::MAX), Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::EINVAL => {},
        Ok(_) => anyhow::bail!("wait() should not succeed on invalid token"),
        Err(e) => anyhow::bail!("wait() should fail with EINVAL (error={:?})", e),
    }

    // Wait on an invalid queue token made from 0 value.
    match libos.wait(QToken::from(0), Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::EINVAL => {},
        Ok(_) => anyhow::bail!("wait() should not succeed on invalid token"),
        Err(e) => anyhow::bail!("wait() should fal with EINVAL (error={:?})", e),
    }

    Ok(())
}
