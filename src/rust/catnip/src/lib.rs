// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(generators, generator_trait)]
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

mod event;
mod interop;
mod logging;
mod options;
mod prelude;
mod protocols;
mod rand;
mod retry;

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
