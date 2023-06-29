// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
    LibOS,
    QDesc,
    QToken,
};
use ::log::{
    error,
    warn,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Pipe server
pub struct PipeServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Underlying pipe descriptor.
    pipeqd: Option<QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PipeServer {
    /// Creates a new pipe server.
    pub fn new(mut libos: LibOS, pipe_name: String) -> Result<Self> {
        let pipeqd: QDesc = libos.create_pipe(&format!("{}:rx", pipe_name))?;
        Ok(Self {
            libos,
            pipeqd: Some(pipeqd),
        })
    }

    /// Runs the target pipe server.
    pub fn run(&mut self) -> Result<()> {
        while self.pop_and_wait()? > 0 {
            // noop.
        }

        Ok(())
    }

    /// Pops a scatter-gather array and waits for the operation to complete.
    fn pop_and_wait(&mut self) -> Result<usize> {
        let mut n: usize = 0;

        // Pop data.
        // The following call to except() is safe because pipeqd is ensured to be open and assigned Some() value.
        let qt: QToken = self.libos.pop(self.pipeqd.expect("pipe should not be closed"), None)?;
        let qr: demi_qresult_t = self.libos.wait(qt, None)?;
        let sga: demi_sgarray_t = match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            demi_opcode_t::DEMI_OPC_FAILED => return Ok(self.handle_fail(&qr)?),
            _ => anyhow::bail!("unexpected operation result"),
        };
        n += sga.sga_segs[0].sgaseg_len as usize;

        if let Err(e) = self.libos.sgafree(sga) {
            error!("sgafree() failed (error={:?})", e);
            warn!("leaking sga");
        }

        Ok(n)
    }

    /// Handles an operation that failed.
    fn handle_fail(&mut self, qr: &demi_qresult_t) -> Result<usize> {
        let qd: QDesc = qr.qr_qd.into();
        let qt: QToken = qr.qr_qt.into();
        let errno: i64 = qr.qr_ret;

        // Check if client has reset the connection.
        if errno == libc::ECONNRESET as i64 {
            println!("INFO: client reset connection (qd={:?})", qd);
        } else {
            println!(
                "WARN: operation failed, ignoring (qd={:?}, qt={:?}, errno={:?})",
                qd, qt, errno
            );
        }

        Ok(0)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for PipeServer {
    // Releases all resources allocated to a pipe server.
    fn drop(&mut self) {
        if let Some(pipeqd) = self.pipeqd {
            // Ignore error.
            if let Err(e) = self.libos.close(pipeqd) {
                error!("close() failed (error={:?})", e);
                warn!("leaking pipeqd={:?}", pipeqd);
            }
        }
    }
}
