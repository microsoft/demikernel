// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Exports
//==============================================================================

pub mod accept;
pub mod close;
pub mod connect;
pub mod pop;
pub mod push;
pub mod pushto;

/// Check whether `errno` indicates that we should retry.
pub fn retry_errno(errno: i32) -> bool {
    errno == libc::EINPROGRESS || errno == libc::EWOULDBLOCK || errno == libc::EAGAIN || errno == libc::EALREADY
}
