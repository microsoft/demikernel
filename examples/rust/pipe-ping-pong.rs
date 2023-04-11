// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::core::slice;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::env;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Size of a data buffer.
const BUFFER_SIZE: usize = 64;

/// Number of rounds to execute.
/// This should fit in a byte.
const NROUNDS: u8 = 128;

//======================================================================================================================
// mksga()
//======================================================================================================================

/// Makes a scatter-gather array.
fn mksga(libos: &mut LibOS, size: usize, value: u8) -> Result<demi_sgarray_t> {
    // Allocate scatter-gather array.
    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    // If error, should free scatter-gather array (and possibly close pipes).
    // FIXME: https://github.com/demikernel/demikernel/issues/647
    assert_eq!(sga.sga_segs[0].sgaseg_len as usize, size);

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

    Ok(sga)
}

//======================================================================================================================
// chksga()
//======================================================================================================================

/// Sanity checks the contents of a scatter-gather array.
fn chksga(sga: &demi_sgarray_t, expected_value: u8) -> Result<usize> {
    let recvbuf: &[u8] = unsafe {
        slice::from_raw_parts(
            sga.sga_segs[0].sgaseg_buf as *const u8,
            sga.sga_segs[0].sgaseg_len as usize,
        )
    };

    // Sanity received data.
    for x in &recvbuf[..] {
        if *x != expected_value {
            anyhow::bail!("got={:?}, expected={:?}", *x, expected_value);
        }
    }

    Ok(recvbuf.len())
}

//======================================================================================================================
// push_and_wait()
//======================================================================================================================

/// Pushes a scatter-gather array and waits for the operation to complete.
fn push_and_wait(libos: &mut LibOS, pipeqd: QDesc, length: usize, value: u8) -> Result<()> {
    let sga: demi_sgarray_t = mksga(libos, length, value)?;

    let qt: QToken = match libos.push(pipeqd, &sga) {
        Ok(qt) => qt,
        Err(e) => {
            // On error, close pipes and free scatter-gather array.
            // FIXME: https://github.com/demikernel/demikernel/issues/647
            anyhow::bail!("failed to push: {:?}", e.cause)
        },
    };

    // On error, close pipes and free scatter-gather array.
    // FIXME: https://github.com/demikernel/demikernel/issues/647
    libos.wait(qt, None)?;

    if libos.sgafree(sga).is_err() {
        // On error, close pipes.
        // FIXME: https://github.com/demikernel/demikernel/issues/647
        anyhow::bail!("failed to release scatter-gather array");
    };

    Ok(())
}

//======================================================================================================================
// pop_and_wait()
//======================================================================================================================

/// Pops a scatter-gather array and waits for the operation to complete.
fn pop_and_wait(libos: &mut LibOS, pipeqd: QDesc, expected_length: usize, expected_value: u8) -> Result<()> {
    let mut nreceived: usize = 0;
    while nreceived < expected_length {
        // Pop data.
        let qt: QToken = match libos.pop(pipeqd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, close pipes and free scatter-gather array.
                // FIXME: https://github.com/demikernel/demikernel/issues/647
                anyhow::bail!("pop failed: {:?}", e.cause)
            },
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) => match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
                _ => {
                    // If error, close pipes and free scatter-gather array.
                    // FIXME: https://github.com/demikernel/demikernel/issues/647
                    anyhow::bail!("unexpected operation result")
                },
            },
            _ => {
                // If error, close pipes.
                // FIXME: https://github.com/demikernel/demikernel/issues/647
                anyhow::bail!("server failed to wait()")
            },
        };

        // Sanity received data.
        // If error, close pipes.
        // FIXME: https://github.com/demikernel/demikernel/issues/647
        nreceived += chksga(&sga, expected_value)?;

        // If error, close pipes.
        // FIXME: https://github.com/demikernel/demikernel/issues/647
        libos.sgafree(sga)?;
    }

    Ok(())
}

//======================================================================================================================
// server()
//======================================================================================================================

fn server(name: &str) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };

    // Setup peer.
    let pipeqd_rx: QDesc = match libos.create_pipe(&format!("{}:rx", name)) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to open memory queue: {:?}", e.cause),
    };

    // Setup peer.
    let pipeqd_tx: QDesc = match libos.create_pipe(&format!("{}:tx", name)) {
        Ok(qd) => qd,
        Err(e) => {
            // Clean up open pipes on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/647
            anyhow::bail!("failed to open memory queue: {:?}", e.cause)
        },
    };

    // Send and receive bytes in a locked step.
    for i in 0..NROUNDS {
        // Clean up open pipes on error.
        // FIXME: https://github.com/demikernel/demikernel/issues/647
        pop_and_wait(&mut libos, pipeqd_rx, BUFFER_SIZE, i)?;
        push_and_wait(&mut libos, pipeqd_tx, BUFFER_SIZE, i)?;
        println!("pong {:?}", i);
    }

    libos.close(pipeqd_rx)?;
    libos.close(pipeqd_tx)?;

    Ok(())
}

//======================================================================================================================
// client()
//======================================================================================================================

fn client(name: &str) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };

    // Setup peer.
    let pipeqd_tx: QDesc = match libos.open_pipe(&format!("{}:rx", name)) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to open memory queue: {:?}", e.cause),
    };
    let pipeqd_rx: QDesc = match libos.open_pipe(&format!("{}:tx", name)) {
        Ok(qd) => qd,
        Err(e) => {
            // If error, close open pipes.
            // FIXME: https://www.microsoft.com/en-us/research/people/eschouks/
            anyhow::bail!("failed to open memory queue: {:?}", e.cause)
        },
    };

    // Send and receive bytes in a locked step.
    for i in 0..NROUNDS {
        // If error, close open pipes.
        // FIXME: https://www.microsoft.com/en-us/research/people/eschouks/
        push_and_wait(&mut libos, pipeqd_tx, BUFFER_SIZE, i)?;
        pop_and_wait(&mut libos, pipeqd_rx, BUFFER_SIZE, i)?;
        println!("pong {:?}", i);
    }

    // Should probably close both pipes?
    // FIXME: https://github.com/demikernel/demikernel/issues/647
    libos.close(pipeqd_tx)?;

    Ok(())
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &String) {
    println!("Usage: {} MODE pipe-name", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        if args[1] == "--server" {
            let ret: Result<()> = server(&args[2]);
            return ret;
        } else if args[1] == "--client" {
            let ret: Result<()> = client(&args[2]);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
