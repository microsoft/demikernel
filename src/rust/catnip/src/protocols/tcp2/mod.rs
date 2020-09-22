pub mod constants;
pub mod peer;
mod passive_open;
mod established;
mod active_open;
pub mod runtime;
mod isn_generator;

#[cfg(test)]
mod tests;

use std::num::Wrapping;

pub type SeqNumber = Wrapping<u32>;

pub use self::peer::Peer;
