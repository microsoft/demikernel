#![allow(unused)]

mod peer;
mod passive_open;
mod established;
mod active_open;

use std::num::Wrapping;

pub type SeqNumber = Wrapping<u32>;
