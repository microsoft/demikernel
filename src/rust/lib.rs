// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(clippy:all))]
#![recursion_limit = "512"]
#![feature(atomic_from_mut)]
#![feature(never_type)]
#![feature(test)]
#![feature(type_alias_impl_trait)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(strict_provenance)]
#![cfg_attr(target_os = "windows", feature(maybe_uninit_uninit_array))]

mod collections;
mod pal;

#[cfg(feature = "profiler")]
pub mod perftools;

pub mod scheduler;

pub mod runtime;

pub mod inetstack;

extern crate test;

#[macro_use]
extern crate log;

#[macro_use]
extern crate cfg_if;

#[cfg(feature = "catnip-libos")]
mod catnip;

#[cfg(feature = "catpowder-libos")]
mod catpowder;

#[cfg(feature = "catcollar-libos")]
mod catcollar;

#[cfg(all(feature = "catnap-libos", target_os = "linux"))]
mod catnap;

#[cfg(all(feature = "catnapw-libos", target_os = "windows"))]
mod catnapw;

#[cfg(feature = "catmem-libos")]
mod catmem;

#[cfg(feature = "catloop-libos")]
mod catloop;

pub use self::demikernel::libos::{
    name::LibOSName,
    LibOS,
};
pub use crate::runtime::{
    network::types::{
        MacAddress,
        Port16,
    },
    types::{
        demi_sgarray_t,
        demi_sgaseg_t,
    },
    OperationResult,
    QDesc,
    QResult,
    QToken,
    QType,
};

pub mod demikernel;

//======================================================================================================================
// Macros
//======================================================================================================================

/// Ensures that two expressions are equivalent or return an Error.
#[macro_export]
macro_rules! ensure_eq {
    ($left:expr, $right:expr) => ({
        match (&$left, &$right) {
            (left_val, right_val) => {
                if !(*left_val == *right_val) {
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`,right: `{:?}`"#, left_val, right_val)
                }
            }
        }
    });
    ($left:expr, $right:expr,) => ({
        crate::ensure_eq!($left, $right)
    });
    ($left:expr, $right:expr, $($arg:tt)+) => ({
        match (&($left), &($right)) {
            (left_val, right_val) => {
                if !(*left_val == *right_val) {
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`, right: `{:?}`: {}"#, left_val, right_val, format_args!($($arg)+))
                }
            }
        }
    });
}

/// Ensure that two expressions are not equal or returns an Error.
#[macro_export]
macro_rules! ensure_neq {
    ($left:expr, $right:expr) => ({
        match (&$left, &$right) {
            (left_val, right_val) => {
                if (*left_val == *right_val) {
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`,right: `{:?}`"#, left_val, right_val)
                }
            }
        }
    });
    ($left:expr, $right:expr,) => ({
        crate::ensure_neq!($left, $right)
    });
    ($left:expr, $right:expr, $($arg:tt)+) => ({
        match (&($left), &($right)) {
            (left_val, right_val) => {
                if (*left_val == *right_val) {
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`, right: `{:?}`: {}"#, left_val, right_val, format_args!($($arg)+))
                }
            }
        }
    });
}

/// Runs a test and prints if it passed or failed on the standard output.
#[macro_export]
macro_rules! run_test {
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
macro_rules! update_test_error {
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
macro_rules! collect_test {
    ($vec:ident, $expr:expr) => {
        $vec.append(&mut $expr);
    };
}

/// Dumps results of tests.
#[macro_export]
macro_rules! dump_test {
    ($vec:ident) => {{
        let mut nfailed: usize = 0;
        // Dump results.
        for (test_name, test_status, test_result) in $vec {
            std::println!("[{}] {}", test_status, test_name);
            if let Err(e) = test_result {
                nfailed += 1;
                std::println!("{}", e);
            }
        }

        if nfailed > 0 {
            anyhow::bail!("{} tests failed", nfailed);
        } else {
            std::println!("all tests passed");
            Ok(())
        }
    }};
}

#[test]
fn test_ensure() -> Result<(), anyhow::Error> {
    ensure_eq!(1, 1);
    ensure_eq!(1, 1, "test");
    ensure_neq!(1, 2);
    ensure_neq!(1, 2, "test");
    Ok(())
}
