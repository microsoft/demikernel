// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::demikernel::LibOS;
use ::runtime::types::dmtr_sgarray_t;

//==============================================================================
// Constants
//==============================================================================

/// Size of scatter-gather arrays.
const SGA_MAX_SIZE: usize = 1280;

//==============================================================================
// Unit Tests for Memory Allocation
//==============================================================================

/// Tests for a single scatter-gather array allocation and deallocation.
#[test]
fn test_unit_sga_alloc_free_single() {
    let size: usize = SGA_MAX_SIZE;
    let libos: LibOS = LibOS::new();

    let sga: dmtr_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => panic!("failed to allocated sga: {:?}", e.cause),
    };
    match libos.sgafree(sga) {
        Ok(()) => (),
        Err(e) => panic!("failed to release sga: {:?}", e.cause),
    };
}

/// Tests looped allocation and deallocation of scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_tight() {
    let size: usize = SGA_MAX_SIZE;
    let libos: LibOS = LibOS::new();

    // Allocate and deallocate several times.
    for _ in 0..1_000_000 {
        let sga: dmtr_sgarray_t = match libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => panic!("failed to allocated sga: {:?}", e.cause),
        };
        match libos.sgafree(sga) {
            Ok(()) => (),
            Err(e) => panic!("failed to release sga: {:?}", e.cause),
        };
    }
}

/// Tests decoupled looped allocation and deallocation of scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_decoupled() {
    let size: usize = SGA_MAX_SIZE;
    let mut sgas: Vec<dmtr_sgarray_t> = Vec::with_capacity(1_000);
    let libos: LibOS = LibOS::new();

    // Allocate and deallocate several times.
    for _ in 0..1_000 {
        // Allocate many scatter-gather arrays.
        for _ in 0..1_000 {
            let sga: dmtr_sgarray_t = match libos.sgaalloc(size) {
                Ok(sga) => sga,
                Err(e) => panic!("failed to allocated sga: {:?}", e.cause),
            };
            sgas.push(sga);
        }

        // Deallocate all scatter-gather arrays.
        for _ in 0..1_000 {
            let sga: dmtr_sgarray_t = sgas.pop().expect("pop from empty vector?");
            match libos.sgafree(sga) {
                Ok(()) => (),
                Err(e) => panic!("failed to release sga: {:?}", e.cause),
            };
        }
    }
}
