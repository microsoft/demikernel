// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::OperationResult;
use ::runtime::{
    types::{
        dmtr_accept_result_t,
        dmtr_opcode_t,
        dmtr_qr_value_t,
        dmtr_qresult_t,
    },
    QDesc,
    Runtime,
};
use ::std::mem;

pub fn pack_result<RT: Runtime>(rt: &RT, result: OperationResult<RT::Buf>, qd: QDesc, qt: u64) -> dmtr_qresult_t {
    match result {
        OperationResult::Connect => dmtr_qresult_t {
            qr_opcode: dmtr_opcode_t::DMTR_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept(new_qd) => {
            let sin = unsafe { mem::zeroed() };
            let qr_value = dmtr_qr_value_t {
                ares: dmtr_accept_result_t {
                    qd: new_qd.into(),
                    addr: sin,
                },
            };
            dmtr_qresult_t {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_ACCEPT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value,
            }
        },
        OperationResult::Push => dmtr_qresult_t {
            qr_opcode: dmtr_opcode_t::DMTR_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(addr, bytes) => match rt.into_sgarray(bytes) {
            Ok(mut sga) => {
                if let Some(addr) = addr {
                    sga.sga_addr.sin_port = addr.get_port().into();
                    sga.sga_addr.sin_addr.s_addr = u32::from_le_bytes(addr.get_address().octets());
                }
                let qr_value = dmtr_qr_value_t { sga };
                dmtr_qresult_t {
                    qr_opcode: dmtr_opcode_t::DMTR_OPC_POP,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                dmtr_qresult_t {
                    qr_opcode: dmtr_opcode_t::DMTR_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            dmtr_qresult_t {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
