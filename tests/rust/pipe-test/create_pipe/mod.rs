// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    LibOSName,
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
    demikernel::collect_test!(
        result,
        demikernel::run_test!(create_pipe_with_same_name_in_two_liboses(pipe_name))
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
    let pipeqd: QDesc = create_pipe(libos, pipe_name)?;

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
    };

    ret
}

/// Creates a pipe in one LibOS, destroys the LibOS without closing the pipe and then creates a pipe with the same name
/// again.
fn create_pipe_with_same_name_in_two_liboses(pipe_name: &str) -> Result<()> {
    {
        let libos: &mut LibOS = &mut {
            // Ok to use expect here because we should have parsed the LibOSName previously.
            let libos_name: LibOSName = LibOSName::from_env().expect("Should have a valid LibOS name").into();
            LibOS::new(libos_name, None).expect("Should be able to create another libOS")
        };

        create_pipe(libos, pipe_name)?;
    }
    {
        let libos: &mut LibOS = &mut {
            // Ok to use expect here because we should have parsed the LibOSName previously.
            let libos_name: LibOSName = LibOSName::from_env().expect("Should have a valid LibOS name").into();
            LibOS::new(libos_name, None).expect("Should be able to create another libOS")
        };

        create_pipe_and_close(libos, pipe_name)?;
    }

    Ok(())
}

/// Creates a pipe with a valid name and does not close it.
fn create_pipe(libos: &mut LibOS, pipe_name: &str) -> Result<QDesc> {
    match libos.create_pipe(pipe_name) {
        Ok(pipeqd) => Ok(pipeqd),
        Err(e) => anyhow::bail!("create_pipe() failed ({})", e),
    }
}

/// Creates a pipe with a valid name and closes it.
fn create_pipe_and_close(libos: &mut LibOS, pipe_name: &str) -> Result<()> {
    let pipeqd: QDesc = create_pipe(libos, pipe_name)?;
    libos.close(pipeqd)?;
    Ok(())
}
