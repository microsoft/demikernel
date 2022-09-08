// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(maybe_uninit_uninit_array, new_uninit)]
#![feature(try_blocks)]
#![cfg_attr(feature = "strict", deny(clippy:all))]
#![feature(never_type)]
#![feature(test)]
#![feature(type_alias_impl_trait)]
#![recursion_limit = "512"]
#![feature(allocator_api)]

pub mod inetstack;

extern crate test;

#[macro_use]
extern crate derive_more;

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

#[cfg(feature = "catnip-libos")]
mod catnip;

#[cfg(feature = "catpowder-libos")]
mod catpowder;

#[cfg(feature = "catcollar-libos")]
mod catcollar;

#[cfg(feature = "catnap-libos")]
mod catnap;

pub use crate::demikernel::libos::network::OperationResult;

pub use self::demikernel::libos::{
    name::LibOSName,
    LibOS,
};
pub use crate::inetstack::runtime::{
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
