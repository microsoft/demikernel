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
use ::std::{
    slice,
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Pipe client
pub struct PipeClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Underlying pipe descriptor.
    pipeqd: Option<QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PipeClient {
    /// Creates a new pipe client.
    pub fn new(mut libos: LibOS, pipe_name: String) -> Result<Self> {
        let pipeqd: QDesc = libos.open_pipe(&format!("{}:rx", pipe_name))?;
        Ok(Self {
            libos,
            pipeqd: Some(pipeqd),
        })
    }

    // Runs the target pipe client.
    pub fn run(&mut self) -> Result<()> {
        let mut push_completed: bool = false;

        // Push some data.
        // The number of pushes is set to an arbitrary value,
        // but a small one to avoid contention on the underling ring buffer.
        for _ in 0..16 {
            self.push_and_wait()?;
        }

        // Push again, but don't wait the operation to complete.
        let qt: QToken = self.push_and_dont_wait()?;

        // Poll once to ensure that the co-routine runs.
        match self.libos.wait(qt, Some(Duration::from_micros(0))) {
            Err(e) if e.errno == libc::ETIMEDOUT => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => push_completed = true,
            Ok(_) => anyhow::bail!("wait() should not complete successfully with an opcode other than DEMI_OPC_PUSH"),
            Err(e) => anyhow::bail!("wait() should not fail wth error other than ETIMEDOUT (error={:?})", e),
        }

        // Succeed to close pipe.
        // The following call to except() is safe because pipeqd is ensured to be open and assigned Some() value.
        match self.libos.close(self.pipeqd.expect("pipe should not be closed")) {
            Ok(()) => self.pipeqd = None,
            Err(e) => {
                anyhow::bail!("close() failed (error={:?})", e)
            },
        }

        // Wait for operation to complete.
        if !push_completed {
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => Ok(()),
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {
                    Ok(())
                },
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => Ok(()),
                Ok(_) => anyhow::bail!("wait() complete successfully or fail with ECANCELED"),
                Err(e) => anyhow::bail!("wait() should not fail (error={:?})", e),
            }
        } else {
            Ok(())
        }
    }

    // Runs the target pipe client.
    pub fn run_aynsc(&mut self) -> Result<()> {
        let mut push_completed: bool = false;

        // Push some data.
        // The number of pushes is set to an arbitrary value,
        // but a small one to avoid contention on the underling ring buffer.
        for _ in 0..16 {
            self.push_and_wait()?;
        }

        // Push again, but don't wait the operation to complete.
        let qt: QToken = self.push_and_dont_wait()?;

        // Poll once to ensure that the co-routine runs.
        match self.libos.wait(qt, Some(Duration::from_micros(0))) {
            Err(e) if e.errno == libc::ETIMEDOUT => {},
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => push_completed = true,
            Ok(_) => anyhow::bail!("wait() should not complete successfully with an opcode other than DEMI_OPC_PUSH"),
            Err(e) => anyhow::bail!("wait() should not fail wth error other than ETIMEDOUT (error={:?})", e),
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

        // Wait for push() operation to complete.
        if !push_completed {
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => {},
                Ok(_) => anyhow::bail!("wait() should complete successfully or fail with ECANCELED"),
                Err(e) => anyhow::bail!("wait() should not fail (error={:?})", e),
            }
        }
        Ok(())
    }

    // Pushes a scatter-gather array and waits for the operation to complete.
    fn push_and_wait(&mut self) -> Result<()> {
        // Push scatter-gather array.
        let qt: QToken = self.push_and_dont_wait()?;

        // Wait for operation to complete.
        match self.libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => Ok(()),
            Ok(_) => anyhow::bail!("wait() should not complete successfully with an opcode other than DEMI_OPC_PUSH"),
            Err(e) => anyhow::bail!("wait() should not fail (error={:?})", e),
        }
    }

    // Pushes a scatter-gather array, but does not wait for the operation to complete.
    fn push_and_dont_wait(&mut self) -> Result<QToken> {
        // Allocate a scatter-gather array. Length and value are arbitrary.
        let sga: demi_sgarray_t = match self.mksga(1, 123) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("mksga() failed (error={:?})", e),
        };

        // Push scatter-gather array.
        // The following call to except() is safe because pipeqd is ensured to be open and assigned Some() value.
        let qt: Result<QToken> = match self.libos.push(self.pipeqd.expect("pipe should not be closed"), &sga) {
            Ok(qt) => Ok(qt),
            Err(e) => Err(anyhow::anyhow!("push() failed (error={:?})", e)),
        };

        // Succeed to release scatter-gather-array.
        if let Err(e) = self.libos.sgafree(sga) {
            error!("sgafree() failed (error={:?})", e);
            warn!("leaking sga")
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
        if let Some(pipeqd) = self.pipeqd {
            // Ignore error.
            if let Err(e) = self.libos.close(pipeqd) {
                error!("close() failed (error={:?})", e);
                warn!("leaking pipeqd={:?}", pipeqd);
            }
        }
    }
}
