// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    num::NonZeroUsize,
    ptr::NonNull,
};

use crate::{
    catnap::transport::{
        error::{
            expect_last_win32_error,
            map_win_err,
        },
        page_alloc::PageAllocator,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};

use super::config::RioSettings;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct RioRuntime {
    allocator: PageAllocator,

    buffers: Vec<DemiBuffer>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl RioRuntime {
    pub fn new(settings: &RioSettings) -> Result<Self, Fail> {
        assert!(
            settings.enable_rio,
            "RioRuntime should not be constructed when not enabled"
        );

        let allocation_size: NonZeroUsize = settings.buffer_count.saturating_mul(settings.buffer_size_bytes);
        let buffer_count: usize = allocation_size.get() / settings.buffer_size_bytes.get();

        let mut result: Self = Self {
            allocator: PageAllocator::new(settings.use_large_pages)?,
            buffers: Vec::with_capacity(settings.buffer_count.get()),
        };

        let buf: NonNull<u8> = result.allocator.alloc(allocation_size)?;

        for i in 0..buffer_count {
            result.buffers.append(buf.add(i * settings.buffer_size_bytes.get()))
        }

        Ok(result)
    }
}

//======================================================================================================================
// Helper Functions
//======================================================================================================================
