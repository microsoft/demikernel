// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#[derive(Clone, Debug)]
pub struct UdpOptions {
    pub rx_checksum_offload: bool,
    pub tx_checksum_offload: bool,
}

impl Default for UdpOptions {
    fn default() -> Self {
        UdpOptions {
            rx_checksum_offload: false,
            tx_checksum_offload: false,
        }
    }
}


