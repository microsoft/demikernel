// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/// Maximum size for a receive buffer.
/// This is set to be the largest power of two that fits in 9000-byte jumbo frames.
pub const RECVBUF_SIZE_MAX: usize = 8192;
