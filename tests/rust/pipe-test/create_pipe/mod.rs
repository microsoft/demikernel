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

    demikernel::collect_test!(result, demikernel::run_test!(create_pipe_with_invalid_name(libos)));
    demikernel::collect_test!(
        result,
        demikernel::run_test!(create_pipe_with_same_name(libos, pipe_name))
    );

    result
}

/// Attempts to create a pipe with an invalid name.
fn create_pipe_with_invalid_name(libos: &mut LibOS) -> Result<()> {
    // Fail to create pipe with an invalid name.
    match libos.create_pipe(&format!("")) {
        Err(e) if e.errno == libc::EINVAL => Ok(()),
        Ok(_) => anyhow::bail!("create_pipe() with invalid name should fail"),
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    }
}

/// Attempts to create two pipes with the same name.
fn create_pipe_with_same_name(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    // Succeed to create first pipe.
    let pipeqd: QDesc = match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => pipeqd,
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    };

    // Fail to create pipe with the same name.
    let mut ret: Result<(), anyhow::Error> = match libos.create_pipe(pipe_name) {
        Err(e) if e.errno == libc::EEXIST => Ok(()),
        Ok(_) => Err(anyhow::anyhow!("create_pipe() with same name should fail")),
        Err(e) => Err(anyhow::anyhow!("create_pipe() failed ({})", e)),
    };

    // Close first pipe.
    match libos.close(pipeqd) {
        Ok(_) => (),
        Err(e) => {
            let errmsg: String = format!("close() failed ({})", e);
            demikernel::update_test_error!(ret, errmsg);
            println!("[ERROR] leaking pipeqd={:?}", pipeqd);
        },
    }

    ret
}
