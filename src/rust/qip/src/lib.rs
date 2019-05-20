#![warn(clippy::all)]

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate derive_more;

mod effect;
mod options;
mod packet;
mod prelude;
mod protocols;
mod rand;
mod sync;

pub mod collections;
pub mod fail;
pub mod result;
pub mod station;

pub use effect::Effect;
pub use options::Options;
pub use packet::Packet;
