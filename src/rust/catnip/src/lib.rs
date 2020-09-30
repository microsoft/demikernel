// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(const_fn, const_panic, const_alloc_layout)]
#![feature(generators, generator_trait)]
#![feature(min_const_generics)]
#![feature(new_uninit)]
#![feature(maybe_uninit_uninit_array, maybe_uninit_extra, maybe_uninit_ref)]
#![feature(never_type)]
#![feature(wake_trait)]
#![feature(raw)]
#![feature(try_blocks)]
#![feature(type_alias_impl_trait)]
#![warn(clippy::all)]
#![recursion_limit = "512"]

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

#[macro_use]
extern crate nom;

#[macro_use]
extern crate derive_more;

pub mod logging;
pub mod options;
pub mod protocols;

pub mod collections;
pub mod engine;
pub mod fail;
pub mod runtime;
pub mod timer;

#[cfg(test)]
pub mod test_helpers;

// #[global_allocator]
// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
