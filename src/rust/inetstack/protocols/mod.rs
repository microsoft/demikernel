// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod layer1;
pub mod layer2;
pub mod layer3;
pub mod layer4;

pub const MAX_HEADER_SIZE: usize =
    layer2::ETHERNET2_HEADER_SIZE + layer3::ipv4::IPV4_HEADER_MAX_SIZE as usize + layer4::tcp::MAX_TCP_HEADER_SIZE;
