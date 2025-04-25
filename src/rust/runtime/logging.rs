// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::flexi_logger::{with_thread, Logger, LoggerHandle};
use ::std::sync::OnceLock;

//======================================================================================================================
// Static Variables
//======================================================================================================================

/// Guardian to the logging initialize function.
static LOG_HANDLE: OnceLock<LoggerHandle> = OnceLock::new();

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Initializes logging features.
pub fn initialize() {
    let _ = LOG_HANDLE.get_or_init(|| Logger::try_with_env().unwrap().format(with_thread).start().unwrap());
}

/// Initialize logging features. The given callback function will initialize FlexiLogger (using a
/// `flexi_logger::Logger` constructor) as desired by the consumer, returning the Logger instance
/// which is then started by Demikernel.
#[allow(unused)]
pub fn custom_initialize<F: FnOnce() -> Logger>(f: F) {
    let _ = LOG_HANDLE.get_or_init(|| {
        let logger: Logger = f();
        logger.start().unwrap()
    });
}
