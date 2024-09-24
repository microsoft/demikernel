
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
static mut AK_LOG_ENABLE: bool = false;
#[allow(unused)]
static mut AK2_LOG_ENABLE: bool = false;

//==============================================================================
// Macros
//==============================================================================

macro_rules! __ak_log {
    ($($arg:tt)*) => {
        if $crate::aklog::log::__is_ak_log_enabled() {
            eprintln!("{}{}{}", $crate::aklog::log::GREEN, format_args!($($arg)*), $crate::aklog::log::CLEAR);
        }
    };
}

macro_rules! __ak2_log {
    ($($arg:tt)*) => {
        if $crate::aklog::log::__is_ak2_log_enabled() {
            eprintln!("{}{}{}", $crate::aklog::log::YELLOW, format_args!($($arg)*), $crate::aklog::log::CLEAR);
        }
    };
}

#[allow(unused)]
pub(crate) use __ak_log;
#[allow(unused)]
pub(crate) use __ak2_log;

//==============================================================================
// Functions
//==============================================================================

#[allow(unused)]
#[inline]
pub(crate) fn __is_ak_log_enabled() -> bool {
    unsafe { AK_LOG_ENABLE }
}

#[allow(unused)]
#[inline]
pub(crate) fn __is_ak2_log_enabled() -> bool {
    unsafe { AK2_LOG_ENABLE }
}

#[allow(unused)]
pub(super) fn init() {
    if let Ok(val) = std::env::var("AK_LOG") {
        unsafe {
            AK_LOG_ENABLE = val == "1" || val == "2";
            AK2_LOG_ENABLE = val == "2" ;
        }
    }
    eprintln!("[AKLOG] ak_log is {}", if unsafe { AK_LOG_ENABLE } {"on"} else {"off"} );
    eprintln!("[AKLOG] ak2_log is {}", if unsafe { AK2_LOG_ENABLE } {"on"} else {"off"});
}