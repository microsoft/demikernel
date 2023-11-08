// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::demikernel::config::Config;
use ::anyhow::Error;
use ::std::{
    collections::HashMap,
    ffi::CString,
    net::Ipv4Addr,
};
use ::yaml_rust::Yaml;

enum CqConfig {}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

const LIBOS: &str = "rio";

/// Rio associated functions for Demikernel configuration object.
impl Config {
    /// Reads the "send_buffer_size" parameter from the underlying configuration file.
    pub fn send_buffer_size(&self) -> usize {
        self.0[LIBOS]["send_buffer_size"].as_i64().unwrap() as usize
    }

    /// Reads the "send_buffer_count" parameter from the underlying configuration file.
    pub fn send_buffer_count(&self) -> usize {
        self.0[LIBOS]["send_buffer_count"].as_i64().unwrap() as usize
    }

    /// Reads the "revc_buffer_size" parameter from the underlying configuration file.
    pub fn revc_buffer_size(&self) -> usize {
        self.0[LIBOS]["revc_buffer_size"].as_i64().unwrap() as usize
    }

    /// Reads the "revc_buffer_count" parameter from the underlying configuration file.
    pub fn revc_buffer_count(&self) -> usize {
        self.0[LIBOS]["revc_buffer_count"].as_i64().unwrap() as usize
    }

    /// Reads the "alloc_recv_bufs_per_socket" parameter from the underlying configuration file.
    pub fn alloc_recv_bufs_per_socket(&self) -> bool {
        self.0[LIBOS]["alloc_recv_bufs_per_socket"].as_bool().unwrap()
    }
}
