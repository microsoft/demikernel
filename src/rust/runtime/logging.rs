// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::std::sync::Once;

//==============================================================================
// Static Variables
//==============================================================================

/// Guardian to the logging initialize function.
static INIT_LOG: Once = Once::new();

//==============================================================================
// Standalone Functions
//==============================================================================

/// Initializes logging features.
pub fn initialize() {
    INIT_LOG.call_once(|| {
        // install global collector configured based on RUST_LOG env var.
        tracing_subscriber::fmt::init();
    });
}
