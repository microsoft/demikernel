// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;

#[cfg(feature = "catnap-libos")]
#[allow(dead_code)]
mod catnap;

#[cfg(feature = "catnip-libos")]
#[allow(dead_code)]
mod catnip;

//==============================================================================
// Exports
//==============================================================================

cfg_if::cfg_if! {
    if #[cfg(feature = "catnip-libos")] {
        pub use self::catnip::Test;
    } else {
        pub use self::catnap::Test;
    }
}
