// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::flexi_logger::{with_thread, writers::LogWriter, Logger, LoggerHandle};
use ::std::sync::OnceLock;

use crate::runtime::types::demi_log_callback_t;

//======================================================================================================================
// Static Variables
//======================================================================================================================

/// Guardian to the logging initialize function.
static LOG_HANDLE: OnceLock<LoggerHandle> = OnceLock::new();

//======================================================================================================================
// Structures
//======================================================================================================================
/// A LogWriter type which calls a C callback function to log messages.
pub struct CallbackLogWriter {
    callback: demi_log_callback_t,
}

//=====================================================================================================================
// Associated Functions
//=====================================================================================================================
impl CallbackLogWriter {
    /// Creates a new `CallbackLogWriter` instance.
    pub fn new(callback: demi_log_callback_t) -> Self {
        Self { callback }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================
impl LogWriter for CallbackLogWriter {
    fn write(&self, _now: &mut flexi_logger::DeferredNow, record: &log::Record) -> std::io::Result<()> {
        let module: &str = record.module_path().unwrap_or("{unnamed}");
        let file: &str = record.file().unwrap_or("{unknown file}");
        let message: String = record.args().to_string();
        ((self.callback)(
            record.level() as i32,
            module.as_ptr() as *const std::ffi::c_char,
            module.len() as u32,
            file.as_ptr() as *const std::ffi::c_char,
            file.len() as u32,
            record.line().unwrap_or(0),
            message.as_ptr() as *const std::ffi::c_char,
            message.len() as u32,
        ));

        Ok(())
    }

    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

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
