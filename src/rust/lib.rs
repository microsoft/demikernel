// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(clippy:all))]
#![recursion_limit = "512"]
#![feature(maybe_uninit_uninit_array)]
#![feature(new_uninit)]
#![feature(try_blocks)]
#![feature(atomic_from_mut)]
#![feature(never_type)]
#![feature(test)]
#![feature(type_alias_impl_trait)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(ptr_sub_ptr)]
#![feature(strict_provenance)]
#![feature(ptr_as_uninit)]

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

pub use crate::demikernel::libos::network::OperationResult;

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
    QDesc,
    QResult,
    QToken,
    QType,
};

pub mod demikernel;
