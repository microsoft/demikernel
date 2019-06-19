#![feature(generators, generator_trait)]
#![warn(clippy::all)]

#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

mod effect;
mod options;
mod prelude;
mod protocols;
mod rand;

pub mod r#async;
pub mod collections;
pub mod fail;
pub mod io;
pub mod result;
pub mod runtime;
pub mod station;

#[cfg(test)]
pub mod test;

pub use effect::Effect;
pub use io::IoVec;
pub use options::Options;
pub use runtime::Runtime;
pub use station::Station;
