// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(maybe_uninit_uninit_array, new_uninit)]
#![feature(try_blocks)]

#[cfg(feature = "catnap-libos")]
pub mod catnap;
#[cfg(feature = "catnip-libos")]
pub mod catnip;
pub mod demikernel;
