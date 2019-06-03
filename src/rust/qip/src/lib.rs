#![feature(generators, generator_trait)]
#![feature(impl_trait_in_bindings)]
#![warn(clippy::all)]

#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate derive_more;

#[macro_use]
extern crate num_derive;

mod effect;
mod options;
mod prelude;
mod protocols;
mod rand;
mod sync;

pub mod r#async;
pub mod collections;
pub mod fail;
pub mod result;
pub mod runtime;
pub mod station;

pub use effect::Effect;
pub use options::Options;
pub use station::Station;
