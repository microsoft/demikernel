// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod memory;
mod ops;
mod queue;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    memory::{
        demi_sgarray_t,
        demi_sgaseg_t,
        DEMI_SGARRAY_MAXLEN,
    },
    ops::{
        demi_accept_result_t,
        demi_opcode_t,
        demi_qr_value_t,
        demi_qresult_t,
    },
    queue::demi_qtoken_t,
};
