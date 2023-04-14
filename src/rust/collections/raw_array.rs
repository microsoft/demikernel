// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::core::{
    alloc::Layout,
    ops::{
        Deref,
        DerefMut,
    },
    ptr,
    slice,
};
use ::std::alloc;

//======================================================================================================================
// Imports
//======================================================================================================================

/// A fixed-size array type.
pub struct RawArray<T> {
    /// Capacity of the array.
    cap: usize,
    /// Pointer to the underlying data.
    ptr: ptr::NonNull<T>,
    /// Is the underlying memory managed by this module?
    is_managed: bool,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions.
impl<T> RawArray<T> {
    /// Creates a managed raw array.
    pub fn new(cap: usize) -> Result<RawArray<T>, Fail> {
        // Check if capacity is invalid.
        if cap == 0 {
            return Err(Fail::new(libc::EINVAL, "cannot create a raw array with zero capacity"));
        }

        // Allocate underlying memory.
        let layout: Layout = match Layout::array::<T>(cap) {
            Ok(layout) => layout,
            Err(_) => return Err(Fail::new(libc::EAGAIN, "failed to create memory layout for raw array")),
        };
        let ptr: ptr::NonNull<T> = {
            let ptr: *mut u8 = unsafe { alloc::alloc(layout) };
            match ptr::NonNull::new(ptr as *mut T) {
                Some(p) => p,
                None => alloc::handle_alloc_error(layout),
            }
        };

        Ok(RawArray {
            ptr,
            cap,
            is_managed: true,
        })
    }

    /// Constructs an unmanaged raw array from a pointer and a length.
    pub fn from_raw_parts(ptr: *mut T, len: usize) -> Result<RawArray<T>, Fail> {
        // Check if capacity is invalid.
        if len == 0 {
            return Err(Fail::new(libc::EINVAL, "cannot create a raw array with zero capacity"));
        }

        // Check and cast provided slice.
        let ptr: ptr::NonNull<T> = match ptr::NonNull::new(ptr) {
            Some(ptr) => ptr,
            None => return Err(Fail::new(libc::EAGAIN, "cannot create raw array from null pointer")),
        };

        Ok(RawArray {
            ptr,
            cap: len,
            is_managed: false,
        })
    }

    /// Gets a mutable slice to the underlying data in the target raw array.
    pub unsafe fn get_mut(&self) -> &mut [T] {
        slice::from_raw_parts_mut(self.ptr.as_ptr(), self.cap)
    }

    /// Gets a slice to the underlying data in the target raw array.
    pub unsafe fn get(&self) -> &[T] {
        slice::from_raw_parts(self.ptr.as_ptr(), self.cap)
    }

    /// Returns the capacity of the target raw array.
    pub fn capacity(&self) -> usize {
        self.cap
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Dereference trait implementation.
impl<T> Deref for RawArray<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { self.get() }
    }
}

/// Mutable dereference trait implementation.
impl<T> DerefMut for RawArray<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.get_mut() }
    }
}

/// Drop trait implementation.
impl<T> Drop for RawArray<T> {
    fn drop(&mut self) {
        // Check if underlying memory was allocated by this module.
        if self.is_managed {
            // Release underlying memory.
            let layout: Layout = Layout::array::<T>(self.cap).unwrap();
            unsafe {
                alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout);
            }
            self.is_managed = false;
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use super::RawArray;
    use ::anyhow::Result;

    const ARRAY_LENGTH: usize = 4;

    /// Constructs a managed raw array.
    fn do_new() -> Result<RawArray<u8>> {
        match RawArray::<u8>::new(ARRAY_LENGTH) {
            Ok(a) => {
                // Sanity check capacity of raw array.
                crate::ensure_eq!(a.capacity(), ARRAY_LENGTH);
                Ok(a)
            },
            Err(_) => anyhow::bail!("creating managed raw arrays should be possible"),
        }
    }

    /// Constructs an unmanaged raw array.
    fn do_from_raw_parts(array: &mut [u8]) -> Result<RawArray<u8>> {
        match RawArray::<u8>::from_raw_parts(array.as_mut_ptr(), array.len()) {
            Ok(a) => {
                // Sanity check capacity of raw array.
                crate::ensure_eq!(a.capacity(), array.len());
                Ok(a)
            },
            Err(_) => anyhow::bail!("constructing unmanaged raw arrays should be possible"),
        }
    }

    /// Tests if we succeed to create a managed raw array.
    #[test]
    fn new() -> Result<()> {
        let _: RawArray<u8> = do_new()?;
        Ok(())
    }

    /// Tests if we fail to create a managed raw array with zero capacity.
    #[test]
    fn bad_new() -> Result<()> {
        match RawArray::<u8>::new(0) {
            Ok(_) => anyhow::bail!("creating managed raw arrays with zero capacity should fail"),
            Err(_) => Ok(()),
        }
    }

    /// Tests if succeed to access and modify raw arrays using dereference traits.
    #[test]
    fn deref_mut() -> Result<()> {
        // Create a managed raw array.
        let mut raw_array: RawArray<u8> = do_new()?;

        // Fill-in raw array.
        for i in 0..raw_array.capacity() {
            raw_array[i] = (i + 1) as u8;
        }

        // Sanity check contents of raw array.
        for i in 0..raw_array.capacity() {
            crate::ensure_eq!(raw_array[i], ((i + 1) as u8));
        }

        Ok(())
    }

    /// Tests if we succeed to construct an unmanaged array from raw parts.
    #[test]
    fn frow_raw_parts() -> Result<()> {
        let mut array: [u8; ARRAY_LENGTH] = [1, 2, 3, 4];

        // Construct an unmanaged raw array.
        let raw_array: RawArray<u8> = do_from_raw_parts(&mut array)?;

        // Sanity check contents of raw array.
        for i in 0..ARRAY_LENGTH {
            crate::ensure_eq!(raw_array[i], (i + 1) as u8);
        }

        Ok(())
    }

    /// Tests if we succeed to access and modify a raw array using unsafe dereference functions.
    #[test]
    fn deref_mut_unsafe() -> Result<()> {
        let mut array: [u8; ARRAY_LENGTH] = [0; ARRAY_LENGTH];

        // Construct an unmanaged raw array.
        let raw_array: RawArray<u8> = do_from_raw_parts(&mut array)?;

        // Write to raw array using unsafe interface.
        for i in 0..raw_array.capacity() {
            unsafe {
                let data: &mut [u8] = raw_array.get_mut();
                data[i] = (raw_array.capacity() - i) as u8;
            }
        }

        // Sanity check contents of raw array using unsafe interface.
        for i in 0..ARRAY_LENGTH {
            unsafe {
                let data: &mut [u8] = raw_array.get_mut();
                crate::ensure_eq!(data[i], (raw_array.capacity() - i) as u8);
            }
        }

        Ok(())
    }
}
