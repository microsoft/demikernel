// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::types::{memory::demi_sgarray_t, queue::demi_qtoken_t};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Operation Code
#[repr(u32)]
#[derive(Debug, Eq, PartialEq)]
pub enum demi_opcode_t {
    DEMI_OPC_INVALID = 0,
    DEMI_OPC_PUSH,
    DEMI_OPC_POP,
    DEMI_OPC_ACCEPT,
    DEMI_OPC_CONNECT,
    DEMI_OPC_CLOSE,
    DEMI_OPC_FAILED,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct demi_accept_result_t {
    pub qd: i32,              // 4 bytes.
    pub addr: libc::sockaddr, // 16 bytes.
}

#[repr(C)]
pub union demi_qr_value_t {
    pub sga: demi_sgarray_t,
    pub ares: demi_accept_result_t,
}

/// Result
#[repr(C)]
pub struct demi_qresult_t {
    pub qr_opcode: demi_opcode_t,
    pub qr_qd: u32,
    pub qr_qt: demi_qtoken_t,
    pub qr_ret: i64,
    pub qr_value: demi_qr_value_t,
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {

    use crate::runtime::types::ops::*;
    use ::std::mem;

    #[test]
    fn test_size_demi_accept_result_t() -> Result<(), anyhow::Error> {
        // Size of a u32.
        const QD_SIZE: usize = 4;
        // Size of a sockaddr structure.
        const ADDR_SIZE: usize = 16;

        crate::ensure_eq!(mem::size_of::<demi_accept_result_t>(), QD_SIZE + ADDR_SIZE);
        Ok(())
    }

    #[test]
    fn test_size_demi_qr_value_t() -> Result<(), anyhow::Error> {
        // Size of a demi_sgarray_t structure.
        const SGA_SIZE: usize = mem::size_of::<demi_sgarray_t>();
        // Size of a demi_accept_result_t structure.
        const ARES_SIZE: usize = mem::size_of::<demi_accept_result_t>();

        crate::ensure_eq!(mem::size_of::<demi_qr_value_t>(), std::cmp::max(SGA_SIZE, ARES_SIZE));
        Ok(())
    }

    #[test]
    fn test_size_demi_qresult_t() -> Result<(), anyhow::Error> {
        const QR_OPCODE_SIZE: usize = 4;
        crate::ensure_eq!(mem::size_of::<demi_opcode_t>(), QR_OPCODE_SIZE);
        const QR_QD_SIZE: usize = 4;
        crate::ensure_eq!(mem::size_of::<u32>(), QR_QD_SIZE);
        const QR_QT_SIZE: usize = 8;
        crate::ensure_eq!(mem::size_of::<demi_qtoken_t>(), QR_QT_SIZE);
        const QR_RET_SIZE: usize = 8;
        crate::ensure_eq!(mem::size_of::<u64>(), QR_RET_SIZE);
        const QR_VALUE_SIZE: usize = mem::size_of::<demi_qr_value_t>();
        const QR_RESULT_SIZE: usize = QR_OPCODE_SIZE + QR_QD_SIZE + QR_QT_SIZE + QR_RET_SIZE + QR_VALUE_SIZE;
        const PADDING: usize = match QR_RESULT_SIZE % mem::align_of::<demi_qresult_t>() {
            0 => 0,
            remainder => mem::align_of::<demi_qresult_t>() - remainder,
        };

        // C headers are packed to 1 byte, so there should be no padding.
        crate::ensure_eq!(PADDING, 0);

        // Size of a demi_qresult_t structure.
        crate::ensure_eq!(mem::size_of::<demi_qresult_t>(), QR_RESULT_SIZE + PADDING);
        Ok(())
    }
}
