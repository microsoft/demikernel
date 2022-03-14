// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(maybe_uninit_uninit_array, new_uninit)]
#![feature(try_blocks)]

#[macro_use]
extern crate log;

cfg_if::cfg_if! {
    if #[cfg(feature = "catnip-libos")] {
        mod catnip;
    } else if  #[cfg(feature = "catpowder-libos")] {
        mod catpowder;
    } else {
        mod catnap;
    }
}

pub mod demikernel;
