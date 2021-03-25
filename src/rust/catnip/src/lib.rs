// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(const_fn, const_panic, const_alloc_layout)]
#![feature(const_mut_refs, const_type_name)]
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
#![feature(test)]
extern crate test;

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

#[macro_use]
extern crate derive_more;

pub mod collections;
pub mod engine;
pub mod fail;
pub mod file_table;
pub mod interop;
pub mod libos;
pub mod logging;
pub mod operations;
pub mod options;
pub mod protocols;
pub mod runtime;
pub mod scheduler;
pub mod sync;
pub mod test_helpers;
pub mod timer;
