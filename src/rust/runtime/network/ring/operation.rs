// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/// Encondes control operations on a ring.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RingControlOperation {
    Close,
    Closed,
}
