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

    demikernel::collect_test!(result, demikernel::run_test!(open_pipe_with_invalid_name(libos)));
    demikernel::collect_test!(
        result,
        demikernel::run_test!(open_pipe_that_does_not_exist(libos, pipe_name))
    );
    demikernel::collect_test!(
        result,
        demikernel::run_test!(open_pipe_multiple_times(libos, pipe_name))
    );

    result
}

/// Attempts to open a pipe with an invalid name.
fn open_pipe_with_invalid_name(libos: &mut LibOS) -> Result<()> {
    // Fail to create pipe with an invalid name.
    match libos.create_pipe(&format!("")) {
        Err(e) if e.errno == libc::EINVAL => Ok(()),
        Ok(_) => anyhow::bail!("create_pipe() with invalid name should fail"),
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    }
}

/// Attempts to open a pipe that does not exist.
fn open_pipe_that_does_not_exist(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    // Fail to open pipe that does not exist.
    match libos.open_pipe(&format!("{}-does-not-exist", pipe_name)) {
        Err(e) if e.errno == libc::ENOENT => Ok(()),
        Ok(_) => anyhow::bail!("open_pipe() that does not exist should fail"),
        Err(e) => anyhow::bail!("open_pipe() failed ({})", e),
    }
}

/// Attempts to open a pipe multiple times.
fn open_pipe_multiple_times(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    let mut ret: Result<(), anyhow::Error> = Ok(());

    // Create a pipe.
    let pipeqd: QDesc = match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => pipeqd,
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    };

    // Succeed to open pipe multiple times. The number of iterations was set arbitrarily.
    for _ in 0..128 {
        let pipeqd: QDesc = match libos.open_pipe(pipe_name) {
            Ok(pipeqd) => pipeqd,
            Err(e) => {
                let errmsg: String = format!("open_pipe() failed ({})", e);
                demikernel::update_test_error!(ret, errmsg);
                println!("[ERROR] leaking pipeqd={:?}", pipeqd);
                break;
            },
        };

        match libos.close(pipeqd) {
            Ok(_) => (),
            Err(e) => {
                let errmsg: String = format!("close() failed ({})", e);
                demikernel::update_test_error!(ret, errmsg);
                println!("[ERROR] leaking pipeqd={:?}", pipeqd);
                break;
            },
        }
    }

    // Succeed to close pipe.
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
