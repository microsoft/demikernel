#![allow(unused)]

pub mod constants;
mod peer;
mod passive_open;
mod established;
mod active_open;

#[cfg(test)]
mod tests;

use std::num::Wrapping;

pub type SeqNumber = Wrapping<u32>;
