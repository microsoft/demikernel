// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::env;
use core::slice;

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
    if sga.sga_segs[0].sgaseg_len as usize != size {
        freesga(libos, sga);
        let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            size,
            seglen
        );
    }

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

    Ok(sga)
}

//======================================================================================================================
// freesga()
//======================================================================================================================

/// Free scatter-gather array and warn on error.
fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        println!("ERROR: sgafree() failed (error={:?})", e);
        println!("WARN: leaking sga");
    }
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
            freesga(libos, sga);
            anyhow::bail!("failed to push: {:?}", e)
        },
    };

    // If error, close pipe.
    // FIXME: https://github.com/demikernel/demikernel/issues/647
    if let Err(e) = libos.wait(qt, None) {
        freesga(libos, sga);
        anyhow::bail!("failed to wait on push: {:?}", e);
    }

    // Do not silently fail here if unable to free scatter-gather array.
    libos.sgafree(sga)?;

    Ok(())
}

//======================================================================================================================
// close()
//======================================================================================================================

/// Closes a pipe and warns if not successful.
fn close(libos: &mut LibOS, qd: QDesc) {
    if let Err(e) = libos.close(qd) {
        println!("ERROR: close() failed (error={:?})", e);
        println!("WARN: leaking qd={:?}", qd);
    }
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
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };

    // Setup peer.
    let pipeqd: QDesc = match libos.create_pipe(&name) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to open memory queue: {:?}", e),
    };

    // Receive bytes.
    let mut round: u8 = 0;
    let mut nbytes: usize = 0;
    while nbytes < (NROUNDS as usize * BUFFER_SIZE) {
        // Pop data.
        let qt: QToken = match libos.pop(pipeqd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, close pipe.
                close(&mut libos, pipeqd);
                anyhow::bail!("pop failed: {:?}", e.cause)
            },
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) => match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
                _ => {
                    // If error, close pipe.
                    close(&mut libos, pipeqd);
                    anyhow::bail!("unexpected operation result")
                },
            },
            _ => {
                // If error, close pipe.
                close(&mut libos, pipeqd);
                anyhow::bail!("server failed to wait()")
            },
        };

        let recvbuf: &[u8] = unsafe {
            slice::from_raw_parts(
                sga.sga_segs[0].sgaseg_buf as *const u8,
                sga.sga_segs[0].sgaseg_len as usize,
            )
        };

        // Sanity received data.
        for x in &recvbuf[..] {
            assert_eq!(*x, round);
            nbytes += 1;
            if nbytes % BUFFER_SIZE == 0 {
                round += 1;
            }
        }

        if libos.sgafree(sga).is_err() {
            // If error, close pipes.
            close(&mut libos, pipeqd);
            anyhow::bail!("failed to release scatter-gather array");
        };

        println!("pop {:?}", round - 1);
    }

    close(&mut libos, pipeqd);

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
    let pipeqd: QDesc = match libos.open_pipe(&name) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to open memory queue: {:?}", e.cause),
    };

    // Send and receive bytes in a locked step.
    for i in 0..NROUNDS {
        // If error, close pipe.
        if let Err(e) = push_and_wait(&mut libos, pipeqd, BUFFER_SIZE, i) {
            close(&mut libos, pipeqd);
            anyhow::bail!("failed to push: {:?}", e)
        }
        println!("push {:?}", i);
    }

    close(&mut libos, pipeqd);

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
