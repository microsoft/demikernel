
//==============================================================================
// Constants
//==============================================================================

#[allow(unused)]
pub(crate) const GREEN: &'static str = "\x1B[32m";
#[allow(unused)]
pub(crate) const YELLOW: &'static str = "\x1B[33m";
#[allow(unused)]
pub(crate) const CLEAR: &'static str = "\x1B[0m";

//==============================================================================
// Data
//==============================================================================

#[allow(unused)]
static mut CAPY_LOG_ENABLE: bool = false;
#[allow(unused)]
static mut CAPY_LOG_MIG_ENABLE: bool = false;

//==============================================================================
// Macros
//==============================================================================

macro_rules! __capy_log {
    ($($arg:tt)*) => {
        if $crate::capylog::log::__is_capy_log_enabled() {
            eprintln!("{}{}{}", $crate::capylog::log::GREEN, format_args!($($arg)*), $crate::capylog::log::CLEAR);
        }
    };
}

macro_rules! __capy_log_mig {
    ($($arg:tt)*) => {
        if $crate::capylog::log::__is_capy_log_mig_enabled() {
            eprintln!("{}{}{}", $crate::capylog::log::YELLOW, format_args!($($arg)*), $crate::capylog::log::CLEAR);
        }
    };
}

#[allow(unused)]
pub(crate) use __capy_log;
#[allow(unused)]
pub(crate) use __capy_log_mig;

//==============================================================================
// Functions
//==============================================================================

#[allow(unused)]
#[inline]
pub(crate) fn __is_capy_log_enabled() -> bool {
    unsafe { CAPY_LOG_ENABLE }
}

#[allow(unused)]
#[inline]
pub(crate) fn __is_capy_log_mig_enabled() -> bool {
    unsafe { CAPY_LOG_MIG_ENABLE }
}

#[allow(unused)]
pub(super) fn init() {
    if let Ok(val) = std::env::var("CAPY_LOG") {
        unsafe {
            CAPY_LOG_ENABLE = val == "all" || val == "base";
            CAPY_LOG_MIG_ENABLE = val == "all" || val == "mig";
        }
    }
    eprintln!("[CAPYLOG] capy_log is {}", if unsafe { CAPY_LOG_ENABLE } {"on"} else {"off"} );
    eprintln!("[CAPYLOG] capy_log_mig is {}", if unsafe { CAPY_LOG_MIG_ENABLE } {"on"} else {"off"});
}