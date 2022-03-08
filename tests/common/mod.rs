// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;

#[cfg(feature = "catpowder-libos")]
#[allow(dead_code)]
mod catpowder;

#[cfg(feature = "catnip-libos")]
#[allow(dead_code)]
mod catnip;

#[cfg(feature = "catnap-libos")]
#[allow(dead_code)]
mod catnap;

//==============================================================================
// Exports
//==============================================================================

cfg_if::cfg_if! {
    if #[cfg(feature = "catnip-libos")] {
        pub use self::catnip::Test;
    } else if #[cfg(feature = "catpowder-libos")] {
        pub use self::catpowder::Test;
    } else {
        pub use self::catnap::Test;
    }
}
