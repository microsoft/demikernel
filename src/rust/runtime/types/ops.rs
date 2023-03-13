// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types)]

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::types::{
    memory::demi_sgarray_t,
    queue::demi_qtoken_t,
};
use ::libc::{
    c_int,
    sockaddr,
};

//==============================================================================
// Structures
//==============================================================================

/// Operation Code
#[repr(C)]
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
#[repr(C)]
#[derive(Copy, Clone)]
pub struct demi_accept_result_t {
    pub qd: c_int,
    pub addr: sockaddr,
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
    pub qr_qd: c_int,
    pub qr_qt: demi_qtoken_t,
    pub qr_ret: c_int,
    pub qr_value: demi_qr_value_t,
}
