// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::fail::Fail,
};
use ::yaml_rust::Yaml;
use std::{
    num::{
        NonZeroI64,
        NonZeroUsize,
    },
    ops::{
        Fn,
        Index,
    },
    time::Duration,
};
use windows::Win32::Networking::WinSock::tcp_keepalive;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct RioSettings {
    pub enable_rio: bool,
    pub use_large_pages: bool,
    pub buffer_size_bytes: NonZeroUsize,
    pub buffer_count: NonZeroUsize,
}

//======================================================================================================================
// Constants
//======================================================================================================================

/// Name for the libos in configs.
const LIBOS: &str = "catnap";

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Windows-specific configuration for Demikernel configuration object.
impl Config {
    /// Reads TCP keepalive settings as a `tcp_keepalive` structure from "tcp_keepalive" subsection.
    pub fn tcp_keepalive(&self) -> Result<tcp_keepalive, Fail> {
        const SECTION: &str = "tcp_keepalive";
        let section: &Yaml = Self::require_subsection(self.get_libos_section()?, SECTION)?;

        let onoff: bool = Self::require_bool_option(section, "enabled")?;
        let keepalivetime: u32 = Self::require_int_option(section, "time_millis")?;
        let keepaliveinterval: u32 = Self::require_int_option(section, "interval")?;

        Ok(tcp_keepalive {
            onoff: if onoff { 1 } else { 0 },
            keepalivetime,
            keepaliveinterval,
        })
    }

    /// Reads socket linger settings from "linger" subsection. Returned value is Some(_) if enabled; otherwise, None.
    /// The linger duration will be no larger than u16::MAX seconds.
    pub fn linger_time(&self) -> Result<Option<Duration>, Fail> {
        const SECTION: &str = "linger";
        let section: &Yaml = Self::require_subsection(self.get_libos_section()?, SECTION)?;

        let enabled: bool = Self::require_bool_option(section, "enabled")?;
        let time_seconds: u16 = Self::require_int_option(section, "time_seconds")?;

        if enabled {
            Ok(Some(Duration::new(time_seconds as u64, 0)))
        } else {
            Ok(None)
        }
    }

    /// Reads registered I/O settings from "rio" subsection.
    pub fn rio_settings(&self) -> Result<RioSettings, Fail> {
        const SECTION: &str = "rio";
        let section: &Yaml = Self::require_subsection(self.get_libos_section()?, SECTION)?;

        Ok(RioSettings {
            enable_rio: Self::require_bool_option(section, "enabled")?,
            use_large_pages: Self::require_bool_option(section, "use_large_pages")?,
            buffer_size_bytes: Self::require_nonzero_int_option(section, "buffer_size_bytes")?,
            buffer_count: Self::require_nonzero_int_option(section, "buffer_count")?,
        })
    }

    /// Get the libos subsection, requiring that it exists and is a Hash.
    fn get_libos_section(&self) -> Result<&Yaml, Fail> {
        Self::require_subsection(&self.0, LIBOS)
    }

    /// Index `yaml` to find the value at `index`, validating that the index exists.
    fn require_option<'a>(yaml: &'a Yaml, index: &str) -> Result<&'a Yaml, Fail> {
        match yaml.index(index) {
            Yaml::BadValue => {
                let message: String = format!("missing configuration option \"{}\"", index);
                Err(Fail::new(libc::EINVAL, message.as_str()))
            },
            value => Ok(value),
        }
    }

    /// Index `yaml` to find the value at `index`, validating that it exists and that the receiver returns Some(_).
    fn require_typed_option<T>(yaml: &Yaml, index: &str, receiver: &dyn Fn(&Yaml) -> Option<T>) -> Result<T, Fail> {
        let option: &Yaml = Self::require_option(yaml, index)?;
        match receiver(option) {
            Some(value) => Ok(value),
            None => {
                let message: String = format!("parameter \"{}\" has unexpected type", index);
                Err(Fail::new(libc::EINVAL, message.as_str()))
            },
        }
    }

    /// Similar to `require_typed_option` using `Yaml::as_hash` receiver. This method returns a `&Yaml` instead of
    /// yaml::Hash, and Yaml is more natural for indexing.
    fn require_subsection<'a>(yaml: &'a Yaml, index: &str) -> Result<&'a Yaml, Fail> {
        let section: &Yaml = Self::require_option(yaml, index)?;
        match section {
            Yaml::Hash(_) => Ok(section),
            _ => {
                let message: String = format!("parameter \"{}\" has unexpected type", index);
                Err(Fail::new(libc::EINVAL, message.as_str()))
            },
        }
    }

    /// Similar to `require_typed_option` using `Yaml::as_i64` as the receiver, but additionally verifies that the
    /// destination type may hold the i64 value.
    fn require_int_option<T: TryFrom<i64>>(yaml: &Yaml, index: &str) -> Result<T, Fail> {
        let val: i64 = Self::require_typed_option(yaml, index, &Yaml::as_i64)?;
        Self::try_convert_param(val, index)
    }

    fn require_nonzero_int_option<T: TryFrom<NonZeroI64>>(yaml: &Yaml, index: &str) -> Result<T, Fail> {
        let val: i64 = Self::require_typed_option(yaml, index, &Yaml::as_i64)?;
        match NonZeroI64::new(val) {
            Some(val) => Self::try_convert_param(val, index),
            None => {
                let message: String = format!("parameter \"{}\" may not be zero", index);
                Err(Fail::new(libc::ERANGE, message.as_str()))
            },
        }
    }

    fn try_convert_param<U, T: TryFrom<U>>(val: U, index: &str) -> Result<T, Fail> {
        match T::try_from(val) {
            Ok(val) => Ok(val),
            _ => {
                let message: String = format!("parameter \"{}\" is out of range", index);
                Err(Fail::new(libc::ERANGE, message.as_str()))
            },
        }
    }

    /// Same as `Self::require_typed_option` using `Yaml::as_bool` as the receiver.
    fn require_bool_option(yaml: &Yaml, index: &str) -> Result<bool, Fail> {
        Self::require_typed_option(yaml, index, &Yaml::as_bool)
    }
}
