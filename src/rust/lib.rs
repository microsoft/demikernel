// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(clippy:all))]
#![recursion_limit = "512"]
#![feature(test)]
#![feature(allocator_api)]
#![feature(strict_provenance)]
#![cfg_attr(target_os = "windows", feature(maybe_uninit_uninit_array))]
#![feature(noop_waker)]
#![feature(hash_extract_if)]

mod collections;
mod pal;

#[cfg(feature = "profiler")]
pub mod perftools;

pub mod runtime;

#[cfg(any(
    feature = "catnap-libos",
    feature = "catnip-libos",
    feature = "catpowder-libos",
    feature = "catloop-libos"
))]
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

#[cfg(all(feature = "catnap-libos"))]
mod catnap;

#[cfg(feature = "catmem-libos")]
mod catmem;

#[cfg(feature = "catloop-libos")]
mod catloop;

pub use self::demikernel::libos::{
    name::LibOSName,
    LibOS,
};
pub use crate::runtime::{
    network::{
        socket::option::SocketOption,
        types::{
            MacAddress,
            Port16,
        },
    },
    types::{
        demi_sgarray_t,
        demi_sgaseg_t,
    },
    OperationResult,
    QDesc,
    QToken,
    QType,
};

pub mod demikernel;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

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
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`, right: `{:?}` at file: {}, line: {}"#, left_val, right_val, file!(), line!())
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
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`, right: `{:?}`: {} at file: {}, line: {}"#, left_val, right_val, format_args!($($arg)+), file!(), line!())
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
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`, right: `{:?}` at file: {}, line: {}"#, left_val, right_val, file!(), line!())
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
                    anyhow::bail!(r#"ensure failed: `(left == right)` left: `{:?}`, right: `{:?}`: {} at file: {}, line: {}"#, left_val, right_val, format_args!($($arg)+), file!(), line!())
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

#[macro_export]
macro_rules! expect_some {
    ( $var:expr, $ex:expr $( , $arg:expr )* ) => {
        $var.unwrap_or_else(|| panic!( $ex $(, $arg )* ))
    };
}

#[macro_export]
macro_rules! expect_ok {
    ( $var:expr, $ex:expr $( , $arg:expr )* ) => {
        $var.unwrap_or_else(|_| panic!( $ex $(, $arg )* ))
    };
}

/// Use this macro to add the current scope to profiling. In effect, the time
/// taken from entering to leaving the scope will be measured.
///
/// Internally, the scope is inserted in the scope tree of the global
/// thread-local [`PROFILER`](constant.PROFILER.html).
///
/// # Example
///
/// The following example will profile the scope `"foo"`, which has the scope
/// `"bar"` as a child.
///
/// ```
/// use inetstack::timer;
///
/// {
///     timer!("foo");
///
///     {
///         timer!("bar");
///         // ... do something ...
///     }
///
///     // ... do some more ...
/// }
/// ```

#[macro_export]
macro_rules! timer {
    ($name:expr) => {
        #[cfg(feature = "profiler")]
        let _guard = $crate::perftools::profiler::PROFILER.with(|p| p.borrow_mut().sync_scope($name));
    };
}

#[cfg(feature = "profiler")]
#[macro_export]
macro_rules! async_timer {
    ($name:expr, $future:expr) => {
        async {
            std::pin::pin!($crate::perftools::profiler::AsyncScope::new(
                $crate::perftools::profiler::PROFILER.with(|p| p.borrow_mut().get_scope($name)),
                std::pin::pin!($future).as_mut()
            ))
            .await
        }
    };
}

#[cfg(not(feature = "profiler"))]
#[macro_export]
macro_rules! async_timer {
    ($name:expr, $future:expr) => {
        $future
    };
}

#[cfg(feature = "profiler")]
#[macro_export]
macro_rules! coroutine_timer {
    ($name:expr, $future:expr) => {
        Box::pin($crate::perftools::profiler::Profiler::coroutine_scope($name, $future).fuse())
    };
}

#[test]
fn test_ensure() -> Result<(), anyhow::Error> {
    ensure_eq!(1, 1);
    ensure_eq!(1, 1, "test");
    ensure_neq!(1, 2);
    ensure_neq!(1, 2, "test");
    Ok(())
}
