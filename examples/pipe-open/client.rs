// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::slice;

use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    LibOS,
    QDesc,
    QToken,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Pipe client
pub struct PipeClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Pipe name.
    pipe_name: String,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PipeClient {
    /// Creates a new pipe client.
    pub fn new(libos: LibOS, pipe_name: String) -> Result<Self> {
        Ok(Self { libos, pipe_name })
    }

    // Runs the target pipe client.
    pub fn run(&mut self, niterations: usize) -> Result<()> {
        let mut qds: Vec<QDesc> = Vec::default();

        // Open several pipes.
        for i in 0..niterations {
            let pipeqd: QDesc = self.libos.open_pipe(&format!("{}:rx", self.pipe_name))?;
            qds.push(pipeqd);
            // Clean up open pipes on error
            // FIXME: https://github.com/demikernel/demikernel/issues/638
            self.push_and_wait(pipeqd, 1, i as u8)?;
        }

        // Close all TCP pipes.
        for qd in qds {
            self.libos.close(qd)?;
        }

        Ok(())
    }

    /// Makes a scatter-gather array.
    fn mksga(&mut self, size: usize, value: u8) -> Result<demi_sgarray_t> {
        // Allocate scatter-gather array.
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        assert_eq!(sga.sga_segs[0].sgaseg_len as usize, size);

        // Fill in scatter-gather array.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.fill(value);

        Ok(sga)
    }

    /// Pushes a scatter-gather array and waits for the operation to complete.
    fn push_and_wait(&mut self, pipeqd: QDesc, length: usize, value: u8) -> Result<()> {
        let sga: demi_sgarray_t = self.mksga(length, value)?;

        // Clean up scatter-gather array on error.
        // FIXME: https://github.com/demikernel/demikernel/issues/638
        let qt: QToken = self.libos.push(pipeqd, &sga)?;

        // Clean up scatter-gather array on error.
        // FIXME: https://github.com/demikernel/demikernel/issues/638
        self.libos.wait(qt, None)?;

        self.libos.sgafree(sga)?;

        Ok(())
    }
}
