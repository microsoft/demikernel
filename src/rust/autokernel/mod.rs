pub(crate) mod profile;
pub mod log;
pub mod time_log;
pub mod parameters;

//==============================================================================
// Profile
//==============================================================================

#[macro_export]
macro_rules! ak_profile {
    ($name:expr) => {
        $crate::invoke_if_feature!("ak-profile", crate::aklog::profile::__ak_profile, $name)
    };
}

/// Merges this time interval with the last one profiled, ensuring that the name is the same.
#[macro_export]
macro_rules! ak_profile_merge_previous {
    ($name:expr) => {
        $crate::invoke_if_feature!("ak-profile", crate::aklog::profile::__ak_profile_merge_previous, $name)
    };
}

#[macro_export]
macro_rules! ak_profile_total {
    ($name:expr) => {
        $crate::invoke_if_feature!("ak-profile", crate::aklog::profile::__ak_profile_total, $name)
    };
}

#[macro_export]
macro_rules! ak_profile_dump {
    ($dump:expr) => {
        $crate::invoke_if_feature!("ak-profile", crate::aklog::profile::__ak_profile_dump, $dump)
    };
}

//==============================================================================
// Log
//==============================================================================

#[macro_export]
macro_rules! ak_log {
    ($($arg:tt)*) => {
        $crate::invoke_if_feature!("ak-log", $crate::aklog::log::__ak_log, $($arg)*)
    };
}

#[macro_export]
macro_rules! ak2_log {
    ($($arg:tt)*) => {
        $crate::invoke_if_feature!("ak-log", $crate::aklog::log::__ak2_log, $($arg)*)
    };
}

//==============================================================================
// Time Log
//==============================================================================

#[macro_export]
macro_rules! ak_time_log {
    ($($arg:tt)*) => {
        $crate::invoke_if_feature!("ak-time-log", $crate::aklog::time_log::__ak_time_log, $($arg)*)
    };
}

#[macro_export]
macro_rules! ak_time_log_dump {
    ($dump:expr) => {
        eprintln!("ak_time_log_dump");
        $crate::invoke_if_feature!("ak-time-log", crate::aklog::time_log::__ak_time_log_dump, $dump)
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
    #[cfg(feature = "ak-profile")]
    profile::init();
    #[cfg(feature = "ak-log")]
    log::init();
    #[cfg(feature = "ak-time-log")]
    time_log::init();
}