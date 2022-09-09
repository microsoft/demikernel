// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::fail::Fail;
use std::{
    fmt::Debug,
    ops::{
        Deref,
        DerefMut,
    },
    slice,
    sync::Arc,
};

//==============================================================================
// Structures
//==============================================================================

/// Data Buffer
#[derive(Clone, Debug)]
pub struct DataBuffer {
    /// Underlying data.
    data: Option<Arc<[u8]>>,

    /// Data offset.
    offset: usize,

    /// Data length.
    len: usize,
}

//==============================================================================
// Associated Functions
//==============================================================================

/// Associated Functions for Data Buffers
impl DataBuffer {
    /// Removes bytes from the front of the target data buffer.
    pub fn adjust(&mut self, nbytes: usize) {
        if nbytes > self.len {
            panic!("adjusting past end of buffer: {} vs {}", nbytes, self.len);
        }
        self.offset += nbytes;
        self.len -= nbytes;
    }

    /// Removes bytes from the end of the target data buffer.
    pub fn trim(&mut self, nbytes: usize) {
        if nbytes > self.len {
            panic!("trimming past beginning of buffer: {} vs {}", nbytes, self.len);
        }
        self.len -= nbytes;
    }

    // Creates a data buffer with a given capacity.
    pub fn new(capacity: usize) -> Result<Self, Fail> {
        // Check if argument is valid.
        if capacity == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-capacity buffer"));
        }

        // Create buffer.
        Ok(Self {
            data: unsafe { Some(Arc::new_zeroed_slice(capacity).assume_init()) },
            offset: 0,
            len: capacity,
        })
    }

    /// Creates a data buffer from a raw pointer and a length.
    pub fn from_raw_parts(data: *mut u8, len: usize) -> Result<Self, Fail> {
        // Check if arguments are valid.
        if len == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }
        if data.is_null() {
            return Err(Fail::new(libc::EINVAL, "null data pointer"));
        }

        // Perform conversions.
        let data: Arc<[u8]> = unsafe {
            let data_ptr: &mut [u8] = slice::from_raw_parts_mut(data, len);
            Arc::from_raw(data_ptr)
        };

        Ok(Self {
            data: Some(data),
            offset: 0,
            len,
        })
    }

    /// Consumes the data buffer returning a raw pointer to the underlying data.
    pub fn into_raw(dbuf: DataBuffer) -> Result<*const [u8], Fail> {
        if let Some(data) = dbuf.data {
            let data_ptr: *const [u8] = Arc::<[u8]>::into_raw(data);
            return Ok(data_ptr);
        }

        Err(Fail::new(libc::EINVAL, "zero-length buffer"))
    }

    /// Creates an empty buffer.
    pub fn empty() -> Self {
        Self {
            data: None,
            offset: 0,
            len: 0,
        }
    }

    /// Creates a data buffer from a slice.
    pub fn from_slice(src: &[u8]) -> Self {
        src.into()
    }
}

//==============================================================================
// Standard-Library Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Data Buffers
impl Deref for DataBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self.data {
            None => &[],
            Some(ref buf) => &buf[self.offset..(self.offset + self.len)],
        }
    }
}

/// Mutable De-Reference Trait Implementation for Data Buffers
impl DerefMut for DataBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self.data {
            None => &mut [],
            Some(ref mut data) => {
                let slice: &mut [u8] = Arc::get_mut(data).expect("cannot write to a shared buffer");
                &mut slice[self.offset..(self.offset + self.len)]
            },
        }
    }
}

/// Conversion Trait Implementation for Data Buffers
impl From<&[u8]> for DataBuffer {
    fn from(src: &[u8]) -> Self {
        let buf: Arc<[u8]> = src.into();
        Self {
            data: Some(buf),
            offset: 0,
            len: src.len(),
        }
    }
}
