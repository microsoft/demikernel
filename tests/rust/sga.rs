// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::anyhow::Result;
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
fn do_test_unit_sga_alloc_free_single(size: usize) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name, None) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };

    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate sga: {:?}", e),
    };
    match libos.sgafree(sga) {
        Ok(()) => Ok(()),
        Err(e) => anyhow::bail!("failed to release sga: {:?}", e.cause),
    }
}

/// Tests a single allocation and deallocation of a small scatter-gather array.
#[test]
fn test_unit_sga_alloc_free_single_small() -> Result<()> {
    do_test_unit_sga_alloc_free_single(SGA_SIZE_SMALL)
}

/// Tests a single allocation and deallocation of a big scatter-gather array.
#[test]
fn test_unit_sga_alloc_free_single_big() -> Result<()> {
    do_test_unit_sga_alloc_free_single(SGA_SIZE_BIG)
}

//==============================================================================
// test_unit_sga_alloc_free_loop_tight()
//==============================================================================

/// Tests looped allocation and deallocation of scatter-gather arrays.
fn do_test_unit_sga_alloc_free_loop_tight(size: usize) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name, None) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };

    // Allocate and deallocate several times.
    for _ in 0..1_000_000 {
        let sga: demi_sgarray_t = match libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => {
                // Free previously allocated sgas on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("failed to allocate sga: {:?}", e)
            },
        };
        match libos.sgafree(sga) {
            Ok(()) => (),
            Err(e) => {
                // Continue freeing remainder on error?
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("failed to release sga: {:?}", e.cause)
            },
        };
    }

    Ok(())
}

/// Tests looped allocation and deallocation of small scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_tight_small() -> Result<()> {
    do_test_unit_sga_alloc_free_loop_tight(SGA_SIZE_SMALL)
}

/// Tests looped allocation and deallocation of big scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_tight_big() -> Result<()> {
    do_test_unit_sga_alloc_free_loop_tight(SGA_SIZE_BIG)
}

//==============================================================================
// test_unit_sga_alloc_free_loop_decoupled()
//==============================================================================

/// Tests decoupled looped allocation and deallocation of scatter-gather arrays.
fn do_test_unit_sga_alloc_free_loop_decoupled(size: usize) -> Result<()> {
    let mut sgas: Vec<demi_sgarray_t> = Vec::with_capacity(1_000);
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name, None) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };

    // Allocate and deallocate several times.
    for _ in 0..1_000 {
        // Allocate many scatter-gather arrays.
        for _ in 0..1_000 {
            let sga: demi_sgarray_t = match libos.sgaalloc(size) {
                Ok(sga) => sga,
                Err(e) => {
                    // Free previously allocated sgas on error.
                    // FIXME: https://github.com/demikernel/demikernel/issues/633
                    anyhow::bail!("failed to allocate sga: {:?}", e)
                },
            };
            sgas.push(sga);
        }

        // Deallocate all scatter-gather arrays.
        for _ in 0..1_000 {
            let sga: demi_sgarray_t = match sgas.pop() {
                Some(sga) => sga,
                None => {
                    // Free previously allocated sgas on error.
                    // FIXME: https://github.com/demikernel/demikernel/issues/633
                    anyhow::bail!("pop from empty vector?")
                },
            };
            match libos.sgafree(sga) {
                Ok(()) => (),
                Err(e) => {
                    // Continue freeing remainder on error?
                    // FIXME: https://github.com/demikernel/demikernel/issues/633
                    anyhow::bail!("failed to release sga: {:?}", e.cause)
                },
            };
        }
    }

    Ok(())
}

/// Tests decoupled looped allocation and deallocation of small scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_decoupled_small() -> Result<()> {
    do_test_unit_sga_alloc_free_loop_decoupled(SGA_SIZE_SMALL)
}

/// Tests decoupled looped allocation and deallocation of big scatter-gather arrays.
#[test]
fn test_unit_sga_alloc_free_loop_decoupled_big() -> Result<()> {
    do_test_unit_sga_alloc_free_loop_decoupled(SGA_SIZE_BIG)
}
