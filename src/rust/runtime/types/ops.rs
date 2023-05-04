// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types)]

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::types::{
    memory::demi_sgarray_t,
    queue::demi_qtoken_t,
};

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

/// Result for `accept()`
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct demi_accept_result_t {
    pub qd: i32,
    pub addr: libc::sockaddr,
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

#[cfg(test)]
mod test {

    use super::*;
    use ::std::mem;

    /// Tests if `demi_accept_result_t` has the expected size.
    #[test]
    fn test_size_demi_accept_result_t() -> Result<(), anyhow::Error> {
        // Size of a u32.
        const QD_SIZE: usize = 4;
        // Size of a sockaddr structure.
        const ADDR_SIZE: usize = 16;
        // Size of a demi_accept_result_t structure.
        crate::ensure_eq!(mem::size_of::<demi_accept_result_t>(), QD_SIZE + ADDR_SIZE);
        Ok(())
    }

    /// Tests if `demi_qr_value_t` has the expected size.
    #[test]
    fn test_size_demi_qr_value_t() -> Result<(), anyhow::Error> {
        // Size of a demi_sgarray_t structure.
        const SGA_SIZE: usize = mem::size_of::<demi_sgarray_t>();
        // Size of a demi_accept_result_t structure.
        const ARES_SIZE: usize = mem::size_of::<demi_accept_result_t>();
        // Size of a demi_qr_value_t structure.
        crate::ensure_eq!(mem::size_of::<demi_qr_value_t>(), std::cmp::max(SGA_SIZE, ARES_SIZE));
        Ok(())
    }

    /// Tests if `demi_qresult_t` has the expected size.
    #[test]
    fn test_size_demi_qresult_t() -> Result<(), anyhow::Error> {
        // Size of a demi_opcode_t enum.
        const QR_OPCODE_SIZE: usize = 4;
        // Size of a u32.
        const QR_QD_SIZE: usize = 4;
        // Size of a demi_qtoken_t type alias.
        const QR_QT_SIZE: usize = 8;
        // Size of a u64.
        const QR_RET_SIZE: usize = 8;
        // Size of a demi_qr_value_t structure.
        const QR_VALUE_SIZE: usize = mem::size_of::<demi_qr_value_t>();
        // Size of a demi_qresult_t structure.
        crate::ensure_eq!(
            mem::size_of::<demi_qresult_t>(),
            QR_OPCODE_SIZE + QR_QD_SIZE + QR_QT_SIZE + QR_RET_SIZE + QR_VALUE_SIZE
        );
        Ok(())
    }
}
