// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    catpowder::LinuxRuntime,
    OperationResult,
};
use ::inetstack::runtime::{
    memory::MemoryRuntime,
    types::{
        demi_accept_result_t,
        demi_opcode_t,
        demi_qr_value_t,
        demi_qresult_t,
    },
    QDesc,
};
use ::std::{
    mem,
    rc::Rc,
};

pub fn pack_result(rt: Rc<LinuxRuntime>, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept(new_qd) => {
            let sin = unsafe { mem::zeroed() };
            let qr_value = demi_qr_value_t {
                ares: demi_accept_result_t {
                    qd: new_qd.into(),
                    addr: sin,
                },
            };
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_ACCEPT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value,
            }
        },
        OperationResult::Push => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(addr, bytes) => match rt.into_sgarray(bytes) {
            Ok(mut sga) => {
                if let Some(endpoint) = addr {
                    let saddr: libc::sockaddr_in = {
                        // TODO: check the following byte order conversion.
                        libc::sockaddr_in {
                            sin_family: libc::AF_INET as u16,
                            sin_port: endpoint.port().into(),
                            sin_addr: libc::in_addr {
                                s_addr: u32::from_le_bytes(endpoint.ip().octets()),
                            },
                            sin_zero: [0; 8],
                        }
                    };
                    sga.sga_addr = unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(saddr) };
                }
                let qr_value = demi_qr_value_t { sga };
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
