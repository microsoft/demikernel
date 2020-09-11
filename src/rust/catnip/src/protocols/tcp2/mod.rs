#![allow(unused)]

mod peer;
mod passive;
mod active;

use std::num::Wrapping;

pub type SeqNumber = Wrapping<u32>;
