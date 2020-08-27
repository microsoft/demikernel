// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(const_fn, const_panic, const_alloc_layout)]
#![feature(const_mut_refs, const_type_name)]
#![feature(generators, generator_trait)]
#![feature(min_const_generics)]
#![feature(new_uninit)]
#![feature(maybe_uninit_uninit_array, maybe_uninit_extra, maybe_uninit_ref)]
#![feature(never_type)]
#![feature(wake_trait)]
#![feature(raw)]
#![feature(try_blocks)]
#![feature(type_alias_impl_trait)]
#![warn(clippy::all)]
#![recursion_limit = "512"]
#![feature(test)]
extern crate test;

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate log;

#[macro_use]
extern crate derive_more;

pub mod collections;
pub mod engine;
pub mod fail;
pub mod file_table;
pub mod interop;
pub mod libos;
pub mod logging;
pub mod operations;
pub mod options;
pub mod protocols;
pub mod runtime;
pub mod scheduler;
pub mod sync;
pub mod test_helpers;
pub mod timer;

// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::alloc;
use tracy_client::static_span;

pub struct ProfiledAllocator<T>(T);

unsafe impl<T: alloc::GlobalAlloc> alloc::GlobalAlloc for ProfiledAllocator<T> {
    unsafe fn alloc(&self, layout: alloc::Layout) -> *mut u8 {
        let _s = static_span!();
        self.0.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: alloc::Layout) {
        let _s = static_span!();
        self.0.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: alloc::Layout) -> *mut u8 {
        let _s = static_span!();
        self.0.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: alloc::Layout, new_size: usize) -> *mut u8 {
        let _s = static_span!();
        self.0.realloc(ptr, layout, new_size)
    }
}

// use mimalloc::MiMalloc;
// static GLOBAL: MiMalloc = MiMalloc;
#[global_allocator]
static GLOBAL: ProfiledAllocator<alloc::System> = ProfiledAllocator(alloc::System);
