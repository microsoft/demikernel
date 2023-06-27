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
    QDesc,
    QToken,
};
use ::log::{
    error,
    warn,
};
use ::std::slice;
use std::time::Duration;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Pipe client
pub struct PipeClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Underlying pipe descriptor.
    pipeqd: QDesc,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PipeClient {
    /// Creates a new pipe client.
    pub fn new(mut libos: LibOS, pipe_name: String) -> Result<Self> {
        let pipeqd: QDesc = libos.open_pipe(&format!("{}:rx", pipe_name))?;
        Ok(Self { libos, pipeqd })
    }

    // Runs the target pipe client.
    pub fn run(&mut self) -> Result<()> {
        // Push data while it is non-blocking for 1 second, then assume that
        // the server has closed its end of the pipe and stop sending.
        loop {
            // Push scatter-gather array.
            let qt: QToken = self.push_and_dont_wait()?;

            // Wait for operation to complete or timeout.
            match self.libos.wait(qt, Some(Duration::from_secs(1))) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => {},
                Err(e) if e.errno == libc::ETIMEDOUT => break,
                Ok(_) => {
                    anyhow::bail!("wait() should not complete successfully with an opcode other than DEMI_OPC_PUSH")
                },
                Err(e) => anyhow::bail!("wait() should not fail (error={:?})", e),
            }
        }

        Ok(())
    }

    // Pushes a scatter-gather array, but does not wait for the operation to complete.
    fn push_and_dont_wait(&mut self) -> Result<QToken> {
        // Allocate a scatter-gather array. Length and value are arbitrary.
        let sga: demi_sgarray_t = match self.mksga(1, 123) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("mksga() failed (error={:?})", e),
        };

        // Push scatter-gather array.
        let qt: Result<QToken> = match self.libos.push(self.pipeqd, &sga) {
            Ok(qt) => Ok(qt),
            Err(e) => Err(anyhow::anyhow!("push() failed (error={:?})", e)),
        };

        // Succeed to release scatter-gather-array.
        if let Err(e) = self.libos.sgafree(sga) {
            warn!("leaking sga");
            error!("sgafree() failed (error={:?})", e);
        }

        qt
    }

    /// Makes a scatter-gather array.
    fn mksga(&mut self, size: usize, value: u8) -> Result<demi_sgarray_t> {
        // Allocate scatter-gather array.
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        demikernel::ensure_eq!(sga.sga_segs[0].sgaseg_len as usize, size);

        // Fill in scatter-gather array.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.fill(value);

        Ok(sga)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for PipeClient {
    // Releases all resources allocated to a pipe client.
    fn drop(&mut self) {
        // Ignore error.
        if let Err(e) = self.libos.close(self.pipeqd) {
            warn!("leaking pipeqd={:?}", self.pipeqd);
            error!("close() failed (error={:?})", e);
        }
    }
}
