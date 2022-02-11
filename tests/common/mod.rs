// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;

#[cfg(feature = "catnap-libos")]
mod catnap;

#[cfg(feature = "catnip-libos")]
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
