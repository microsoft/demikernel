pub(crate) mod profile;
pub mod log;
pub mod time_log;

//==============================================================================
// Profile
//==============================================================================

#[macro_export]
macro_rules! capy_profile {
    ($name:expr) => {
        $crate::invoke_if_feature!("capy-profile", crate::capylog::profile::__capy_profile, $name)
    };
}

/// Merges this time interval with the last one profiled, ensuring that the name is the same.
#[macro_export]
macro_rules! capy_profile_merge_previous {
    ($name:expr) => {
        $crate::invoke_if_feature!("capy-profile", crate::capylog::profile::__capy_profile_merge_previous, $name)
    };
}

#[macro_export]
macro_rules! capy_profile_total {
    ($name:expr) => {
        $crate::invoke_if_feature!("capy-profile", crate::capylog::profile::__capy_profile_total, $name)
    };
}

#[macro_export]
macro_rules! capy_profile_dump {
    ($dump:expr) => {
        $crate::invoke_if_feature!("capy-profile", crate::capylog::profile::__capy_profile_dump, $dump)
    };
}

//==============================================================================
// Log
//==============================================================================

#[macro_export]
macro_rules! capy_log {
    ($($arg:tt)*) => {
        $crate::invoke_if_feature!("capy-log", $crate::capylog::log::__capy_log, $($arg)*)
    };
}

#[macro_export]
macro_rules! capy_log_mig {
    ($($arg:tt)*) => {
        $crate::invoke_if_feature!("capy-log", $crate::capylog::log::__capy_log_mig, $($arg)*)
    };
}

//==============================================================================
// Time Log
//==============================================================================

#[macro_export]
macro_rules! capy_time_log {
    ($($arg:tt)*) => {
        $crate::invoke_if_feature!("capy-time-log", $crate::capylog::time_log::__capy_time_log, $($arg)*)
    };
}

#[macro_export]
macro_rules! capy_time_log_dump {
    ($dump:expr) => {
        $crate::invoke_if_feature!("capy-time-log", crate::capylog::time_log::__capy_time_log_dump, $dump)
    };
}

//==============================================================================
// General
//==============================================================================

#[macro_export]
macro_rules! invoke_if_feature {
    ($feature:expr, $macro:path) => {
        #[cfg(feature = $feature)]
        $macro!()
    };

    ($feature:expr, $macro:path, $($arg:tt)*) => {
        #[cfg(feature = $feature)]
        $macro!($($arg)*)
    };
}

pub(crate) fn init() {
    #[cfg(feature = "capy-profile")]
    profile::init();
    #[cfg(feature = "capy-log")]
    log::init();
    #[cfg(feature = "capy-time-log")]
    time_log::init();
}