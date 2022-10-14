// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This file is for CPU architecture-specific things.

// ------------------------
// CPU Data Cache Line Size
// ------------------------
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub const CPU_DATA_CACHE_LINE_SIZE: usize = 64;
