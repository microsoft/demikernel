#![feature(generators, generator_trait)]
#![warn(clippy::all)]
// todo: remove once project is more mature.
#![allow(dead_code)]

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

#[macro_use]
extern crate nom;

#[macro_use]
extern crate derive_more;

#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[cfg(test)]
#[macro_use]
extern crate maplit;

#[macro_use]
pub mod r#async;

mod bindings;
mod event;
mod options;
mod prelude;
mod protocols;
mod rand;

pub mod collections;
pub mod engine;
pub mod fail;
pub mod io;
pub mod result;
pub mod runtime;

#[cfg(test)]
pub mod test;

pub use engine::Engine;
pub use event::Event;
pub use io::IoVec;
pub use options::Options;
pub use runtime::Runtime;
