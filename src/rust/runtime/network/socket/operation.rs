// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SocketOp {
    Bind,
    Listen,
    Accept,
    Accepted,
    Connect,
    Connected,
    Close,
    Closed,
}
