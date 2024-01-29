// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod accept;
mod args;
mod async_close;
mod bind;
mod close;
mod connect;
mod listen;
mod socket;
mod wait;

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use args::ProgramArguments;
use demikernel::{
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
    LibOS,
    LibOSName,
};

//======================================================================================================================
// Macros
//======================================================================================================================

/// Runs a test and prints if it passed or failed on the standard output.
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

/// Updates the error variable `err_var` with `anyhow::anyhow!(msg)` if `err_var` is `Ok`.
/// Otherwise it prints `msg` on the standard output.
#[macro_export]
macro_rules! update_error {
    ($err_var:ident, $msg:expr) => {
        if $err_var.is_ok() {
            $err_var = Err(anyhow::anyhow!($msg));
        } else {
            println!("{}", $msg);
        }
    };
}

/// Collects the result of a test and appends it to a vector.
#[macro_export]
macro_rules! collect {
    ($vec:ident, $expr:expr) => {
        $vec.append(&mut $expr);
    };
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

fn main() -> Result<()> {
    let mut nfailed: usize = 0;
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    let args: ProgramArguments = ProgramArguments::new(
        "tcp",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for TCP queues.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    crate::collect!(result, socket::run(&mut libos));
    crate::collect!(result, bind::run(&mut libos, &args.local().ip()));
    crate::collect!(result, listen::run(&mut libos, &args.local(), &args.remote()));
    crate::collect!(result, accept::run(&mut libos, &args.local()));
    crate::collect!(result, connect::run(&mut libos, &args.local(), &args.remote()));
    crate::collect!(result, close::run(&mut libos, &args.local()));
    crate::collect!(result, wait::run(&mut libos, &args.local()));
    crate::collect!(result, async_close::run(&mut libos, &args.local()));

    // Dump results.
    for (test_name, test_status, test_result) in result {
        println!("[{}] {}", test_status, test_name);
        if let Err(e) = test_result {
            nfailed += 1;
            println!("    {}", e);
        }
    }

    if nfailed > 0 {
        anyhow::bail!("{} tests failed", nfailed);
    } else {
        println!("all tests passed");
        Ok(())
    }
}

pub fn check_for_network_error(qr: &demi_qresult_t) -> bool {
    qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED
        && (qr.qr_ret == (libc::EBADF as i64)
            || qr.qr_ret == (libc::ECANCELED as i64)
            || qr.qr_ret == (libc::ECONNREFUSED as i64)
            || qr.qr_ret == (libc::ECONNABORTED as i64))
}
