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
use ::std::time::Duration;

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
        let mut pop_completed: bool = false;

        // Succeed to pop some data.
        // The number of pops is set to an arbitrary value.
        for _ in 0..16 {
            self.pop_and_wait()?;
        }

        // Succeed to pop.
        let qt: QToken = self.pop_and_dont_wait()?;

        // Poll once to ensure that the co-routine runs.
        match self.libos.wait(qt, Some(Duration::from_micros(0))) {
            Err(e) if e.errno == libc::ETIMEDOUT => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => {
                pop_completed = true;
                self.handle_pop(&qr)?;
            },
            Ok(_) => anyhow::bail!("wait() should complete successfully"),
            Err(e) => anyhow::bail!("wait() should fail with ETIMEDOUT (error={:?})", e),
        }

        // Succeed to close pipe.
        // The following call to except() is safe because pipeqd is ensured to be open and assigned Some() value.
        match self.libos.close(self.pipeqd.expect("pipe should not be closed")) {
            Ok(()) => self.pipeqd = None,
            Err(e) => anyhow::bail!("close() failed (error={:?})", e),
        }

        // Wait for operation to complete.
        if !pop_completed {
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => Ok(()),
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {
                    Ok(())
                },
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => self.handle_pop(&qr),
                Ok(_) => {
                    anyhow::bail!("wait() should either complete successfully or fail with ECANCELED")
                },
                Err(e) => anyhow::bail!("wait() should failed (error={:?})", e),
            }
        } else {
            Ok(())
        }
    }

    /// Runs the target pipe server.
    pub fn run_async(&mut self) -> Result<()> {
        let mut pop_completed: bool = false;

        // Succeed to pop some data.
        // The number of pops is set to an arbitrary value.
        for _ in 0..16 {
            self.pop_and_wait()?;
        }

        // Succeed to pop.
        let qt: QToken = self.pop_and_dont_wait()?;

        // Poll once to ensure that the co-routine runs.
        match self.libos.wait(qt, Some(Duration::from_micros(0))) {
            Err(e) if e.errno == libc::ETIMEDOUT => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => {
                pop_completed = true;
                self.handle_pop(&qr)?;
            },
            Ok(_) => anyhow::bail!("wait() should complete successfully"),
            Err(e) => anyhow::bail!("wait() should fail with ETIMEDOUT (error={:?})", e),
        }

        // Succeed to close pipe.
        // The following call to except() is safe because pipeqd is ensured to be open and assigned Some() value.
        let qt_close: QToken = match self.libos.async_close(self.pipeqd.expect("pipe should not be closed")) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("async_close() failed (error={:?})", e),
        };

        // Ensure that async_close() completes.
        match self.libos.wait(qt_close, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => self.pipeqd = None,
            Ok(_) => anyhow::bail!("wait() should not complete successfully with an opcode other than DEMI_OPC_CLOSE"),
            Err(e) => anyhow::bail!("wait() should not fail (error={:?})", e),
        }

        // Wait for operation to complete.
        if !pop_completed {
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => self.handle_pop(&qr)?,
                Ok(_) => {
                    anyhow::bail!("wait() should either complete successfully or fail with ECANCELED")
                },
                Err(e) => anyhow::bail!("wait() should failed (error={:?})", e),
            }
        };
        Ok(())
    }

    /// Pops a scatter-gather array, but does not wait for the operation to complete.
    fn pop_and_dont_wait(&mut self) -> Result<QToken> {
        // The following call to except() is safe because pipeqd is ensured to be open and assigned Some() value.
        match self.libos.pop(self.pipeqd.expect("pipe should not be closed"), None) {
            Ok(qt) => Ok(qt),
            Err(e) => anyhow::bail!("pop() failed (error={:?})", e),
        }
    }

    /// Pops a scatter-gather array and waits for the operation to complete.
    fn pop_and_wait(&mut self) -> Result<()> {
        let qt: QToken = self.pop_and_dont_wait()?;

        // Wait for operation to complete.
        match self.libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => {
                let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                // Succeed to release scatter-gather array.
                match self.libos.sgafree(sga) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        warn!("leaking sga");
                        anyhow::bail!("sgafree() failed (error={:?})", e);
                    },
                }
            },
            Ok(_) => anyhow::bail!("wait() should not complete successfully with an opcode other than DEMI_OPC_POP"),
            Err(e) => anyhow::bail!("wait() should not fail (error={:?})", e),
        }
    }

    /// Handles the completion of a pop operation.
    fn handle_pop(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

        // Succeed to release scatter-gather array.
        match self.libos.sgafree(sga) {
            Ok(()) => Ok(()),
            Err(e) => {
                warn!("leaking sga");
                anyhow::bail!("sgafree() failed (error={:?})", e);
            },
        }
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
