// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(maybe_uninit_uninit_array, new_uninit)]
#![feature(try_blocks)]

#[macro_use]
extern crate log;

#[cfg(feature = "catpowder-libos")]
pub mod catpowder;

#[cfg(feature = "catnip-libos")]
pub mod catnip;

pub mod demikernel;
