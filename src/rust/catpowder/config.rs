// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::network::types::MacAddress,
};

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Config {
    /// Reads the "local interface name" parameter from the underlying configuration file.
    pub fn local_interface_name(&self) -> String {
        // FIXME: this function should return a Result.

        // FIXME: Change the follow key from "catnip" to "catpowder".
        let local_interface_name: &str = self.0["catnip"]["my_interface_name"]
            .as_str()
            .ok_or_else(|| anyhow::format_err!("Couldn't find my_interface_name config"))
            .unwrap();

        local_interface_name.to_string()
    }

    /// Reads the "local link address" parameter from the underlying configuration file.
    pub fn local_link_addr(&self) -> MacAddress {
        // FIXME: this function should return a Result.

        // Parse local IPv4 address.
        // FIXME: Change the follow key from "catnip" to "catpowder".
        let local_link_addr: MacAddress = MacAddress::parse_str(
            self.0["catnip"]["my_link_addr"]
                .as_str()
                .ok_or_else(|| anyhow::format_err!("Couldn't find my_link_addr in config"))
                .unwrap(),
        )
        .unwrap();
        local_link_addr
    }
}
