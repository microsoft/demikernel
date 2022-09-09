// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::demikernel::{
    runtime::types::demi_sgarray_t,
    LibOS,
    LibOSName,
};

//==============================================================================
// Constants
//==============================================================================

/// Size for small scatter-gather arrays.
const SGA_SIZE_SMALL: usize = 64;

/// Size for big scatter-gather arrays.
const SGA_SIZE_BIG: usize = 1280;

//==============================================================================
// test_unit_sga_alloc_free_single()
//==============================================================================

/// Tests for a single scatter-gather array allocation and deallocation.
fn do_test_unit_sga_alloc_free_single(size: usize) {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => panic!("failed to allocate sga: {:?}", e.cause),
    };
    match libos.sgafree(sga) {
        Ok(()) => (),
        Err(e) => panic!("failed to release sga: {:?}", e.cause),
    };
}

/// Tests a single allocation and deallocation of a small scatter-gather array.
#[test]
fn test_unit_sga_alloc_free_single_small() {
    do_test_unit_sga_alloc_free_single(SGA_SIZE_SMALL)
}

/// Tests a single allocation and deallocation of a big scatter-gather array.
#[test]
fn test_unit_sga_alloc_free_single_big() {
    do_test_unit_sga_alloc_free_single(SGA_SIZE_BIG)
}

//==============================================================================
// test_unit_sga_alloc_free_loop_tight()
//==============================================================================

/// Tests looped allocation and deallocation of scatter-gather arrays.
fn do_test_unit_sga_alloc_free_loop_tight(size: usize) {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    // Allocate and deallocate several times.
    for _ in 0..1_000_000 {
        let sga: demi_sgarray_t = match libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => panic!("failed to allocate sga: {:?}", e.cause),
        };
        match libos.sgafree(sga) {
            Ok(()) => (),
            Err(e) => panic!("failed to release sga: {:?}", e.cause),
        };
    }
}

/// Tests looped allocation and deallocation of small scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_tight_small() {
    do_test_unit_sga_alloc_free_loop_tight(SGA_SIZE_SMALL)
}

/// Tests looped allocation and deallocation of big scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_tight_big() {
    do_test_unit_sga_alloc_free_loop_tight(SGA_SIZE_BIG)
}

//==============================================================================
// test_unit_sga_alloc_free_loop_decoupled()
//==============================================================================

/// Tests decoupled looped allocation and deallocation of scatter-gather arrays.
fn do_test_unit_sga_alloc_free_loop_decoupled(size: usize) {
    let mut sgas: Vec<demi_sgarray_t> = Vec::with_capacity(1_000);
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    // Allocate and deallocate several times.
    for _ in 0..1_000 {
        // Allocate many scatter-gather arrays.
        for _ in 0..1_000 {
            let sga: demi_sgarray_t = match libos.sgaalloc(size) {
                Ok(sga) => sga,
                Err(e) => panic!("failed to allocate sga: {:?}", e.cause),
            };
            sgas.push(sga);
        }

        // Deallocate all scatter-gather arrays.
        for _ in 0..1_000 {
            let sga: demi_sgarray_t = sgas.pop().expect("pop from empty vector?");
            match libos.sgafree(sga) {
                Ok(()) => (),
                Err(e) => panic!("failed to release sga: {:?}", e.cause),
            };
        }
    }
}

/// Tests decoupled looped allocation and deallocation of small scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_decoupled_small() {
    do_test_unit_sga_alloc_free_loop_decoupled(SGA_SIZE_SMALL)
}

/// Tests decoupled looped allocation and deallocation of big scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_decoupled_big() {
    do_test_unit_sga_alloc_free_loop_decoupled(SGA_SIZE_BIG)
}
