// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(const_fn, const_panic, const_alloc_layout)]
#![feature(generators, generator_trait)]
#![feature(min_const_generics)]
#![feature(new_uninit)]
#![feature(maybe_uninit_uninit_array)]
#![feature(never_type)]
#![feature(raw)]
#![feature(try_blocks)]
#![feature(type_alias_impl_trait)]
#![warn(clippy::all)]
#![recursion_limit="512"]

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

#[macro_use]
extern crate nom;

#[macro_use]
extern crate derive_more;

#[macro_use]
extern crate lazy_static;

pub mod event;
mod interop;
mod logging;
pub mod options;
mod prelude;
pub mod protocols;
pub mod rand;

pub mod collections;
pub mod engine;
pub mod fail;
pub mod result;
pub mod runtime;
pub mod shims;

#[cfg(test)]
pub mod test;

pub use engine::Engine;
pub use event::Event;
pub use options::Options;
pub use runtime::Runtime;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
