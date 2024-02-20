// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This file is for CPU architecture-specific things.

// ------------------------
// CPU Data Cache Line Size
// ------------------------
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub const CPU_DATA_CACHE_LINE_SIZE: usize = 64;

// Test for architecture-specific PAL
#[cfg(test)]
mod tests {
    use ::anyhow::Result;

    use crate::{
        ensure_eq,
        pal::arch::CPU_DATA_CACHE_LINE_SIZE,
    };

    // Test basic allocation, len, adjust, and trim.
    #[test]
    fn constraints() -> Result<()> {
        ensure_eq!(CPU_DATA_CACHE_LINE_SIZE.is_power_of_two(), true);
        Ok(())
    }
}
