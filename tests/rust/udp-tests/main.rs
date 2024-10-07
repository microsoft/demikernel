// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![deny(clippy::all)]

mod args;
mod bind;
mod close;

use anyhow::Result;
use args::ProgramArguments;
use demikernel::{LibOS, LibOSName};

/// Runs a test and prints the result to standard output.
#[macro_export]
macro_rules! test {
    ($fn_name:ident($($arg:expr),*)) => {{
        match $fn_name($($arg),*) {
            Ok(ok) =>
                vec![(stringify!($fn_name).to_string(), "passed".to_string(), Ok(ok))],
            Err(err) =>
                vec![(stringify!($fn_name).to_string(), "failed".to_string(), Err(err))],
        }
    }};
}

/// Appends the test result to the vector.
#[macro_export]
macro_rules! append_test_result {
    ($vec:ident, $expr:expr) => {
        $vec.append(&mut $expr);
    };
}

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new()?;
    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name, None)?
    };
    let mut num_failed_tests: usize = 0;
    let mut test_results: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    append_test_result!(
        test_results,
        bind::run_tests(&mut libos, &args.get_local_socket_addr().ip())
    );

    append_test_result!(
        test_results,
        close::run_tests(&mut libos, &args.get_local_socket_addr().ip())
    );

    for (test_name, test_status, test_result) in test_results {
        println!("[{}] {}", test_status, test_name);
        if let Err(e) = test_result {
            num_failed_tests += 1;
            println!("    {}", e);
        }
    }

    if num_failed_tests > 0 {
        anyhow::bail!("{} tests failed", num_failed_tests);
    }

    println!("all tests passed");
    Ok(())
}
