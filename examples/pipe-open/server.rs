// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
    LibOS,
    QDesc,
    QToken,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Pipe server
pub struct PipeServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Pipe name.
    pipe_name: String,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PipeServer {
    /// Creates a new pipe server.
    pub fn new(libos: LibOS, pipe_name: String) -> Result<Self> {
        Ok(Self { libos, pipe_name })
    }

    /// Runs the target pipe server.
    pub fn run(&mut self) -> Result<()> {
        let pipeqd: QDesc = self.libos.create_pipe(&format!("{}:rx", self.pipe_name))?;

        while self.pop_and_wait(pipeqd)? > 0 {}

        self.libos.close(pipeqd)?;

        Ok(())
    }

    /// Pops a scatter-gather array and waits for the operation to complete.
    fn pop_and_wait(&mut self, pipeqd: QDesc) -> Result<usize> {
        let mut n: usize = 0;

        // Pop data.
        let qt: QToken = self.libos.pop(pipeqd, None)?;
        let qr: demi_qresult_t = self.libos.wait(qt, None)?;
        let sga: demi_sgarray_t = match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            _ => anyhow::bail!("unexpected operation result"),
        };
        n += sga.sga_segs[0].sgaseg_len as usize;

        self.libos.sgafree(sga)?;

        Ok(n)
    }
}
