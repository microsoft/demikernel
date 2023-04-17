// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

// TODO: Remove allowances on this module.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::core::{
    mem,
    ops::{
        Deref,
        DerefMut,
    },
    ptr,
    slice,
};
use ::std::ffi;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A named shared memory region.
pub struct SharedMemory {
    /// Was this region created or opened?
    was_created: bool,
    /// Name.
    name: ffi::CString,
    /// Underlying file descriptor.
    fd: libc::c_int,
    /// Size in bytes.
    size: libc::size_t,
    /// Base address.
    addr: *mut libc::c_void,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions.
impl SharedMemory {
    /// Opens an existing named shared memory region.
    pub fn open(name: &str, len: usize) -> Result<SharedMemory, Fail> {
        let name: ffi::CString = match ffi::CString::new(name.to_string()) {
            Ok(name) => name,
            Err(_) => return Err(Fail::new(libc::EINVAL, "could not parse name of shared memory region")),
        };
        let fd: libc::c_int = unsafe {
            // Forward request to underlying POSIX OS.
            let ret: libc::c_int = libc::shm_open(name.as_ptr(), libc::O_RDWR, libc::S_IRUSR | libc::S_IWUSR);

            // Check for failure return value.
            if ret == -1 {
                let errno: libc::c_int = *libc::__errno_location();
                let cause: String = format!(
                    "failed to open shared memory region (name={:?}, len={}, errno={})",
                    name, len, errno
                );
                error!("open(): {}", cause);
                return Err(Fail::new(errno, &cause));
            }

            ret
        };

        let mut shm: SharedMemory = SharedMemory {
            was_created: false,
            fd,
            name,
            size: 0,
            addr: ptr::null_mut(),
        };

        shm.map(len)?;

        Ok(shm)
    }

    /// Creates a named shared memory region.
    pub fn create(name: &str, size: usize) -> Result<SharedMemory, Fail> {
        let name: ffi::CString = match ffi::CString::new(name.to_string()) {
            Ok(name) => name,
            Err(_) => return Err(Fail::new(libc::EINVAL, "could not parse name of shared memory region")),
        };
        // Forward request to underlying POSIX OS.
        let fd: libc::c_int = unsafe {
            let ret: libc::c_int = libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                libc::S_IRUSR | libc::S_IWUSR,
            );

            // Check for failure return value.
            if ret == -1 {
                let errno: libc::c_int = *libc::__errno_location();
                let cause: String = format!(
                    "failed to create shared memory region (name={:?}, size={}, errno={})",
                    name, size, errno
                );
                error!("create(): {}", cause);
                return Err(Fail::new(errno, &cause));
            }
            ret
        };

        let mut shm: SharedMemory = SharedMemory {
            was_created: true,
            fd,
            name,
            size: 0,
            addr: ptr::null_mut(),
        };

        shm.truncate(size)?;
        shm.map(size)?;

        Ok(shm)
    }

    /// Closes the target shared memory region.
    fn close(&mut self) -> Result<(), Fail> {
        // Forward request to underlying POSIX OS.
        unsafe {
            let ret: libc::c_int = libc::close(self.fd);
            if ret == -1 {
                return Err(Fail::new(libc::EAGAIN, "failed to close shared memory region"));
            }
        }

        self.fd = -1;

        Ok(())
    }

    /// Unlinks the target shared memory region.
    fn unlink(&mut self) -> Result<(), Fail> {
        // Forward request to underlying POSIX OS.
        unsafe {
            let ret: libc::c_int = libc::shm_unlink(self.name.as_ptr());

            // Check for failure return value.
            if ret == -1 {
                return Err(Fail::new(libc::EAGAIN, "failed to unlink shared memory region"));
            }
        }

        Ok(())
    }

    /// Truncates the target shared memory region.
    fn truncate(&mut self, size: usize) -> Result<(), Fail> {
        // Forward request to underlying POSIX OS.
        unsafe {
            let ret: libc::c_int = libc::ftruncate(self.fd, size as libc::off_t);

            // Check for failure return value.
            if ret == -1 {
                return Err(Fail::new(libc::EAGAIN, "failed to truncate shared memory region"));
            }
        };

        self.size = size;

        Ok(())
    }

    /// Maps the target shared memory region to the address space of the calling process.
    fn map(&mut self, size: usize) -> Result<(), Fail> {
        // Forward request to underlying POSIX OS.
        let addr: *mut libc::c_void = unsafe {
            let ret: *mut libc::c_void = libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                self.fd,
                0,
            );

            // Check for failure return value.
            if ret == libc::MAP_FAILED {
                return Err(Fail::new(libc::EAGAIN, "failed to map shared memory region"));
            }
            ret
        };

        self.addr = addr;
        self.size = size;

        Ok(())
    }

    // Unmaps the target shared memory region from the address space of the calling process.
    fn unmap(&mut self) -> Result<(), Fail> {
        let len: libc::size_t = self.size;
        if len == 0 {
            return Err(Fail::new(libc::EINVAL, "cannot unmap zero-length shared memory region"));
        }
        // Forward request to underlying POSIX OS.
        unsafe {
            let ret: libc::c_int = libc::munmap(self.addr, self.size);

            // Check for failure return value.
            if ret == -1 {
                return Err(Fail::new(libc::EAGAIN, "failed to unmap shared memory region"));
            }
        }

        Ok(())
    }

    /// Returns the size of the target shared memory region.
    #[allow(unused)]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Writes a value to the target shared memory region at a given offset.
    #[allow(unused)]
    pub fn write<T>(&mut self, index: usize, val: &T) {
        let size_of_t: usize = mem::size_of::<T>();
        let offset: usize = index * size_of_t;
        if offset <= (self.size - size_of_t) {
            unsafe {
                let dest: *mut u8 = (self.addr as *mut u8).add(offset);
                let src: *const T = val as *const T;
                libc::memcpy(dest as *mut libc::c_void, src as *const libc::c_void, size_of_t);
            };
        }
    }

    /// Reads a value from the target shared memory region at a given offset.
    #[allow(unused)]
    pub fn read<T>(&mut self, index: usize, val: &mut T) {
        let size_of_t: usize = mem::size_of::<T>();
        let offset: usize = index * size_of_t;
        if offset <= (self.size - size_of_t) {
            unsafe {
                let dest: *mut T = val as *mut T;
                let src: *const u8 = (self.addr as *mut u8).add(offset);
                libc::memcpy(dest as *mut libc::c_void, src as *const libc::c_void, size_of_t);
            };
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Dereference trait implementation.
impl Deref for SharedMemory {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        let data: *const u8 = self.addr as *const u8;
        let len: usize = self.size;
        unsafe { slice::from_raw_parts(data, len) }
    }
}

/// Mutable dereference trait implementation.
impl DerefMut for SharedMemory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let data: *mut u8 = self.addr as *mut u8;
        let len: usize = self.size;
        unsafe { slice::from_raw_parts_mut(data, len) }
    }
}

/// Drop trait implementation.
impl Drop for SharedMemory {
    fn drop(&mut self) {
        // 1) Unmap the underlying shared memory region from the address space of the calling process.
        match self.unmap() {
            Ok(_) => {},
            Err(e) => eprintln!("{}", e),
        };
        // 2) Close the underlying shared memory region.
        match self.close() {
            Ok(_) => {},
            Err(e) => eprintln!("{}", e),
        }
        // 3) Remove the underlying shared memory region name link.
        if self.was_created {
            match self.unlink() {
                Ok(_) => {},
                Err(e) => eprintln!("{}", e),
            }
        }
    }
}

//======================================================================================================================
// Unit Test
//======================================================================================================================

#[cfg(test)]
mod tests {
    use super::SharedMemory;
    use ::anyhow::Result;

    const SHM_SIZE: usize = 4096;

    /// Successfully opens a shared memory region.
    fn do_create(name: &str) -> Result<SharedMemory> {
        let shm: SharedMemory = match SharedMemory::create(name, SHM_SIZE) {
            Ok(shm) => shm,
            Err(_) => anyhow::bail!("creating a shared memory region with valis size should be possible"),
        };

        // Sanity check dimension of shared memory region.
        crate::ensure_eq!(shm.size(), SHM_SIZE);

        Ok(shm)
    }

    /// Successfully opens an existing shared memory region.
    fn do_open(name: &str) -> Result<SharedMemory> {
        let shm: SharedMemory = match SharedMemory::open(name, SHM_SIZE) {
            Ok(shm) => shm,
            Err(_) => anyhow::bail!("opening a shared memory region with valis size should be possible"),
        };

        // Sanity check dimension of shared memory region.
        crate::ensure_eq!(shm.size(), SHM_SIZE);

        Ok(shm)
    }

    /// Tests if we succeed to create a shared memory region.
    #[test]
    fn create() -> Result<()> {
        let shm_name: String = "shm-test-create".to_string();
        let _shm_created: SharedMemory = do_create(&shm_name)?;
        let _shm_open: SharedMemory = do_open(&shm_name)?;

        Ok(())
    }

    /// Tests if we succeed to open a shared memory region.
    #[test]
    fn open() -> Result<()> {
        let shm_name: String = "shm-test-open".to_string();
        let _shm_created: SharedMemory = do_create(&shm_name)?;

        Ok(())
    }

    /// Tets if we succeed to read/write to/from a shared memory region using read/write functions.
    #[test]
    fn read_write() -> Result<()> {
        let shm_name: String = "shm-test-read-write".to_string();
        let mut shm: SharedMemory = do_create(&shm_name)?;

        // Write bytes.
        for i in 0..shm.size() {
            shm.write::<u8>(i, &((i & 255) as u8));
        }

        // Read bytes.
        for i in 0..shm.size() {
            let mut val: u8 = 0;
            shm.read::<u8>(i, &mut val);
            crate::ensure_eq!(val, (i & 255) as u8);
        }

        Ok(())
    }

    /// Tets if we succeed to read/write to/from a shared memory region using dereference trait.
    #[test]
    fn read_write_deref() -> Result<()> {
        let shm_name: String = "shm-test-read-write-deref".to_string();
        let mut shm: SharedMemory = do_create(&shm_name)?;

        // Write bytes.
        for i in 0..shm.size() {
            shm[i] = (i & 255) as u8;
        }

        // Read bytes.
        for i in 0..shm.size() {
            crate::ensure_eq!(shm[i], (i & 255) as u8);
        }

        Ok(())
    }

    /// Tests if we succeed to read/write to a shared memory region that is mapped at multiple address ranges.
    #[test]
    fn read_write_multiple_ranges() -> Result<()> {
        let shm_name: String = "shm-test-read-write-multiple-ranges".to_string();
        let mut shm_wronly: SharedMemory = do_create(&shm_name)?;
        let shm_rdonly: SharedMemory = do_open(&shm_name)?;

        // Write bytes.
        for i in 0..shm_wronly.size() {
            shm_wronly[i] = (i & 255) as u8;
        }

        // Read bytes.
        for i in 0..shm_wronly.size() {
            crate::ensure_eq!(shm_rdonly[i], (i & 255) as u8);
        }

        Ok(())
    }
}
