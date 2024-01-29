// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::DEFAULT_TIMEOUT;
use anyhow::Result;
use demikernel::{
    runtime::types::demi_opcode_t,
    LibOS,
    QDesc,
    QToken,
};

/// Returns true if the error code indicates that the socket is closed.
pub fn is_closed(ret: i64) -> bool {
    match ret as i32 {
        libc::ECONNRESET | libc::ENOTCONN | libc::ECANCELED | libc::EBADF => true,
        _ => false,
    }
}

/// Issues a close() operation and waits for it to complete.
pub fn close_and_wait(libos: &mut LibOS, qd: QDesc) -> Result<()> {
    let qt: QToken = match libos.async_close(qd) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("async_close() failed for qd: {:?}: {:?}", qd, e),
    };

    match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && is_closed(qr.qr_ret) => {},
        _ => anyhow::bail!("wait() should succeed with async_close()"),
    }

    Ok(())
}
