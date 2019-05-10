#![warn(clippy::all)]

#[macro_use]
extern crate lazy_static;

pub mod fail;
pub mod result;

mod prelude;
mod protocols;
mod rand;
mod state;
mod sync;
