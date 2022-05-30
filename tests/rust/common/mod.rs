// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;

#[cfg(feature = "catpowder-libos")]
mod catpowder;

#[cfg(feature = "catnip-libos")]
mod catnip;

#[cfg(feature = "catnap-libos")]
mod catnap;

#[cfg(feature = "catcollar-libos")]
mod catcollar;

//==============================================================================
// Exports
//==============================================================================

#[cfg(feature = "catpowder-libos")]
#[allow(dead_code)]
pub use self::catpowder::Test;

#[cfg(feature = "catnip-libos")]
#[allow(dead_code)]
pub use self::catnip::Test;

#[cfg(feature = "catnap-libos")]
#[allow(dead_code)]
pub use self::catnap::Test;

#[cfg(feature = "catcollar-libos")]
pub use self::catcollar::Test;
