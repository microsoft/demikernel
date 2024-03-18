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

    if let Err(e) = libos.wait(qt, None) {
        freesga(libos, sga);
        anyhow::bail!("failed to wait on push: {:?}", e);
    }

    // Do not silently fail here if unable to free scatter-gather array.
    libos.sgafree(sga)?;

    Ok(())
}

// The pipe server.
pub struct PipeServer {
    /// Underlying libOS.
    libos: LibOS,
    /// The pipe qd.
    pipeqd: QDesc,
    /// The scatter-gather array.
    sga: Option<demi_sgarray_t>,
}

// Implementation of the pipe server.
impl PipeServer {
    pub fn new(mut libos: LibOS, pipe_name: &str) -> Result<Self> {
        // Create the pipe.
        let pipeqd: QDesc = match libos.create_pipe(&pipe_name) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to open memory queue: {:?}", e),
        };

        return Ok(Self {
            libos,
            pipeqd,
            sga: None,
        });
    }

    fn run(&mut self, buffer_size: usize, nrounds: usize) -> Result<()> {
        // Receive bytes.
        let mut round: u8 = 0;
        let mut nbytes: usize = 0;
        while nbytes < (nrounds * buffer_size) {
            // Pop data.
            let qt: QToken = match self.libos.pop(self.pipeqd, None) {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("pop failed: {:?}", e.cause),
            };
            self.sga = match self.libos.wait(qt, None) {
                Ok(qr) => match qr.qr_opcode {
                    demi_opcode_t::DEMI_OPC_POP => unsafe { Some(qr.qr_value.sga) },
                    _ => anyhow::bail!("unexpected operation result"),
                },
                _ => anyhow::bail!("server failed to wait()"),
            };

            if let Some(sga) = self.sga {
                // Sanity received data.
                let recvbuf: &[u8] = unsafe {
                    slice::from_raw_parts(
                        sga.sga_segs[0].sgaseg_buf as *const u8,
                        sga.sga_segs[0].sgaseg_len as usize,
                    )
                };
                for x in &recvbuf[..] {
                    demikernel::ensure_eq!(*x, round);
                    nbytes += 1;
                    if nbytes % buffer_size == 0 {
                        round += 1;
                    }
                }
                // Free up the scatter-gather array.
                match self.libos.sgafree(sga) {
                    Ok(_) => self.sga = None,
                    Err(_) => anyhow::bail!("failed to release scatter-gather array"),
                };
            } else {
                anyhow::bail!("expected a valid sga");
            }

            println!("pop {:?} bytes", nbytes);
        }

        Ok(())
    }
}

// Drop implementation for the pipe server.
impl Drop for PipeServer {
    fn drop(&mut self) {
        if let Err(e) = self.libos.close(self.pipeqd) {
            println!("ERROR: close() failed (error={:?})", e);
            println!("WARN: leaking pipeqd={:?}", self.pipeqd);
        }
        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
    }
}

// The pipe client.
pub struct PipeClient {
    /// Underlying libOS.
    libos: LibOS,
    /// The pipe qd.
    pipeqd: QDesc,
}

// Implementation of the pipe client.
impl PipeClient {
    pub fn new(mut libos: LibOS, pipe_name: &str) -> Result<Self> {
        // Open the pipe.
        let pipeqd: QDesc = match libos.open_pipe(&pipe_name) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to open memory queue: {:?}", e.cause),
        };

        return Ok(Self { libos, pipeqd });
    }

    pub fn run(&mut self, buffer_size: usize, nrounds: u8) -> Result<()> {
        // Send and receive bytes in a locked step.
        for i in 0..nrounds {
            if let Err(e) = push_and_wait(&mut self.libos, self.pipeqd, buffer_size, i) {
                anyhow::bail!("failed to push: {:?}", e)
            }
            println!("push {:?}", i);
        }

        Ok(())
    }
}

// Drop implementation for the pipe client.
impl Drop for PipeClient {
    fn drop(&mut self) {
        if let Err(e) = self.libos.close(self.pipeqd) {
            println!("ERROR: close() failed (error={:?})", e);
            println!("WARN: leaking pipeqd={:?}", self.pipeqd);
        }
    }
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
        // Create the libos instance.
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        let libos: LibOS = match LibOS::new(libos_name) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
        };
        let pipe_name: &str = &args[2];

        // Invoke the appropriate peer.
        if args[1] == "--server" {
            let mut server: PipeServer = PipeServer::new(libos, pipe_name)?;
            return server.run(BUFFER_SIZE, NROUNDS as usize);
        } else if args[1] == "--client" {
            let mut client: PipeClient = PipeClient::new(libos, pipe_name)?;
            return client.run(BUFFER_SIZE, NROUNDS);
        }
    }

    usage(&args[0]);

    Ok(())
}
