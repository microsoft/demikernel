// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    alloc::Layout,
    mem::MaybeUninit,
    num::NonZeroUsize,
    ptr::NonNull,
};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{
            CloseHandle,
            GetLastError,
            FALSE,
            HANDLE,
            LUID,
        },
        Security::{
            AdjustTokenPrivileges,
            LookupPrivilegeValueW,
            LUID_AND_ATTRIBUTES,
            SE_LOCK_MEMORY_NAME,
            SE_PRIVILEGE_ENABLED,
            TOKEN_ADJUST_PRIVILEGES,
            TOKEN_PRIVILEGES,
            TOKEN_QUERY,
        },
        System::{
            Memory::{
                GetLargePageMinimum,
                VirtualAlloc,
                VirtualFree,
                MEM_COMMIT,
                MEM_LARGE_PAGES,
                MEM_RELEASE,
                MEM_RESERVE,
                PAGE_READWRITE,
                VIRTUAL_ALLOCATION_TYPE,
            },
            SystemInformation::{
                GetSystemInfo,
                SYSTEM_INFO,
            },
            Threading::{
                GetCurrentProcess,
                OpenProcessToken,
            },
        },
    },
};

use crate::{
    catnap::transport::error::{
        expect_last_win32_error,
        map_win_err,
    },
    runtime::fail::Fail,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct PageAllocator {
    page_size: NonZeroUsize,
    large_page_size: Option<NonZeroUsize>,
}

//======================================================================================================================
// Constants
//======================================================================================================================

// Safety: constant value is not zero.
const FALLBACK_ALLOC_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4096) };

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PageAllocator {
    /// Create a new page allocator.
    pub fn new(enable_large_page: bool) -> Result<Self, Fail> {
        let result: Self = Self {
            // NB
            page_size: Self::get_page_size(),
            large_page_size: if enable_large_page {
                Self::get_large_page_size().map_err(map_win_err)?
            } else {
                None
            },
        };

        Ok(result)
    }

    /// Get the allocation granularity, which should be either a large page or a standard page.
    pub fn get_allocation_unit(&self) -> NonZeroUsize {
        self.large_page_size.unwrap_or(self.page_size)
    }

    /// Get the allocator's unit as a Layout. Both size and alignment will be `self.get_allocation_unit()`.
    #[allow(unused)]
    pub fn get_allocation_layout(&self) -> Layout {
        let unit: NonZeroUsize = self.get_allocation_unit();

        // Safety: Windows system page size will always be a power of two and non-zero.
        unsafe { Layout::from_size_align_unchecked(unit.get(), unit.get()) }
    }

    /// Allocate a number of pages according to `size`, where `size` will be rounded up to the next page boundary.
    pub fn alloc(&self, size: NonZeroUsize) -> Result<NonNull<u8>, Fail> {
        let alloc_unit: usize = self.get_allocation_unit().get();
        let size: NonZeroUsize = size.saturating_add(alloc_unit - (((size.get() - 1) % alloc_unit) + 1));

        let alloc_type: VIRTUAL_ALLOCATION_TYPE = MEM_COMMIT
            | MEM_RESERVE
            | self
                .large_page_size
                .and(Some(MEM_LARGE_PAGES))
                .unwrap_or(VIRTUAL_ALLOCATION_TYPE(0));

        let ptr: *mut u8 = unsafe { VirtualAlloc(None, size.get(), alloc_type, PAGE_READWRITE) as *mut u8 };

        NonNull::new(ptr).ok_or_else(expect_last_win32_error)
    }

    /// Deallocate a number of pages. The pointer passed into this function must be a pointer returned from alloc.
    pub fn dealloc(&self, ptr: NonNull<u8>) -> Result<(), Fail> {
        unsafe { VirtualFree(ptr.as_ptr().cast(), 0, MEM_RELEASE) }.map_err(map_win_err)
    }

    /// Get the system standard page size.
    fn get_page_size() -> NonZeroUsize {
        let info: SYSTEM_INFO = unsafe {
            let mut info: MaybeUninit<SYSTEM_INFO> = MaybeUninit::uninit();
            GetSystemInfo(info.as_mut_ptr());
            info.assume_init()
        };

        // dwPageSize should never be zero, but even if it is, 4KB will be valid for non-Itanium systems.
        NonZeroUsize::new(info.dwPageSize as usize).unwrap_or(FALLBACK_ALLOC_SIZE)
    }

    /// Enable large pages and get the large page size. Ok(None) indicates large pages is not supported.
    fn get_large_page_size() -> windows::core::Result<Option<NonZeroUsize>> {
        Self::enable_privilege(SE_LOCK_MEMORY_NAME)?;

        Ok(NonZeroUsize::new(unsafe { GetLargePageMinimum() }))
    }

    /// Enable the named privilege for the current process.
    fn enable_privilege(priv_name: PCWSTR) -> windows::core::Result<()> {
        let mut proc_token: HANDLE = HANDLE::default();
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut proc_token,
            )
        }?;

        let cleanup = || unsafe {
            let _ = CloseHandle(proc_token);
        };

        let err_cleanup = |e: windows::core::Error| -> windows::core::Result<()> {
            cleanup();
            Err(e)
        };

        let mut tp: TOKEN_PRIVILEGES = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: LUID::default(),
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        unsafe { LookupPrivilegeValueW(PCWSTR::null(), priv_name, &mut tp.Privileges[0].Luid) }.or_else(err_cleanup)?;

        unsafe { AdjustTokenPrivileges(proc_token, FALSE, Some(&tp), 0, None, None) }.or_else(err_cleanup)?;

        // NB: AdjustTokenPrivileges may succeed and indicate ERROR_NOT_ALL_ASSIGNED, which means the privilege was not
        // granted.
        let result: windows::core::Result<()> = unsafe { GetLastError() };

        cleanup();
        result
    }
}
