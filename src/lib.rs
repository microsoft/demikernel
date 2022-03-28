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
        pub use self::catnip::DPDKBuf;
        pub use ::catnip::operations::OperationResult as OperationResult;
    } else if  #[cfg(feature = "catpowder-libos")] {
        mod catpowder;
        pub use ::catnip::operations::OperationResult;
    } else {
        mod catnap;
        pub use catnap::OperationResult;
    }
}

pub use self::demikernel::libos::LibOS;
pub use ::catnip::protocols::ipv4::Ipv4Endpoint;
pub use ::runtime::{
    network::types::{
        Ipv4Addr,
        MacAddress,
        Port16,
    },
    types::{
        dmtr_sgarray_t,
        dmtr_sgaseg_t,
    },
    QDesc,
    QResult,
    QToken,
    QType,
};

pub mod demikernel;
