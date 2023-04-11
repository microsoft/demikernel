// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    runtime::fail::Fail,
    LibOS,
};

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for pipe queues.
pub fn run(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    let mut ret: Result<(), anyhow::Error> = Ok(());

    match create_pipe_with_invalid_name(libos) {
        Ok(_) => println!("[passed] {}", stringify!(create_pipe_with_invalid_name)),
        Err(e) => {
            // Don't overwrite previous error.
            if ret.is_ok() {
                ret = Err(e);
            }
            println!("[FAILED] {}", stringify!(create_pipe_with_invalid_name))
        },
    };

    ret
}

/// Attempts to create a pipe with an invalid name.
fn create_pipe_with_invalid_name(libos: &mut LibOS) -> Result<()> {
    // Fail to create pipe with an invalid name.
    match libos.create_pipe(&format!("")) {
        Err(e) if e.errno == libc::EINVAL => Ok(()),
        Ok(_) => anyhow::bail!("create_pipe() with invalid name should fail"),
        Err(e) => anyhow::bail!("create_pipe() failed with {}", e.cause),
    }
}
