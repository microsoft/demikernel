// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(maybe_uninit_uninit_array, new_uninit)]
#![feature(try_blocks)]

#[macro_use]
extern crate log;

#[cfg(feature = "catnip-libos")]
mod catnip;

#[cfg(feature = "catpowder-libos")]
mod catpowder;

#[cfg(feature = "catcollar-libos")]
mod catcollar;

#[cfg(feature = "catnap-libos")]
mod catnap;

#[cfg(feature = "catnip-libos")]
#[path = ""]
mod libos_export {
    pub(crate) use crate::catnip::CatnipLibOS as NetworkLibOS;
    pub use ::inetstack::operations::OperationResult;
}

#[cfg(feature = "catpowder-libos")]
#[path = ""]
mod libos_export {
    pub(crate) use crate::catpowder::CatpowderLibOS as NetworkLibOS;
    pub use ::inetstack::operations::OperationResult;
}
#[cfg(feature = "catcollar-libos")]
#[path = ""]
mod libos_export {
    pub(crate) use crate::catcollar::CatcollarLibOS as NetworkLibOS;
    pub use crate::catcollar::OperationResult;
}

#[cfg(feature = "catnap-libos")]
#[path = ""]
mod libos_export {
    pub(crate) use crate::catnap::CatnapLibOS as NetworkLibOS;
    pub use crate::catnap::OperationResult;
}

pub use libos_export::*;

pub use self::demikernel::libos::LibOS;
pub use ::runtime::{
    network::types::{
        Ipv4Addr,
        MacAddress,
        Port16,
    },
    types::{
        demi_sgarray_t,
        demi_sgaseg_t,
    },
    QDesc,
    QResult,
    QToken,
    QType,
};

pub mod demikernel;
