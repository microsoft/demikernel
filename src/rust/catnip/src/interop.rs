#![allow(non_camel_case_types)]

use crate::{
    file_table::FileDescriptor,
    operations::OperationResult,
};
use libc::{
    c_int,
    c_void,
    sockaddr_in,
};
use std::{
    mem,
    ptr,
    slice,
};

pub type dmtr_qtoken_t = u64;

pub const DMTR_SGARRAY_MAXSIZE: usize = 1;

#[derive(Copy, Clone)]
pub struct dmtr_sgaseg_t {
    pub sgaseg_buf: *mut c_void,
    pub sgaseg_len: u32,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct dmtr_sgarray_t {
    pub sga_buf: *mut c_void,
    pub sga_numsegs: u32,
    pub sga_segs: [dmtr_sgaseg_t; DMTR_SGARRAY_MAXSIZE],
    pub sga_addr: sockaddr_in,
}

impl From<&[u8]> for dmtr_sgarray_t {
    fn from(bytes: &[u8]) -> Self {
        let buf: Box<[u8]> = bytes.into();
        let ptr = Box::into_raw(buf);
        let sgaseg = dmtr_sgaseg_t {
            sgaseg_buf: ptr as *mut _,
            sgaseg_len: bytes.len() as u32,
        };
        dmtr_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        }
    }
}

impl dmtr_sgarray_t {
    pub fn free(self) {
        for i in 0..self.sga_numsegs as usize {
            let seg = &self.sga_segs[i];
            let allocation: Box<[u8]> = unsafe {
                Box::from_raw(slice::from_raw_parts_mut(
                    seg.sgaseg_buf as *mut _,
                    seg.sgaseg_len as usize,
                ))
            };
            drop(allocation);
        }
    }
}

#[repr(C)]
#[derive(Debug, Eq, PartialEq)]
pub enum dmtr_opcode_t {
    DMTR_OPC_INVALID = 0,
    DMTR_OPC_PUSH,
    DMTR_OPC_POP,
    DMTR_OPC_ACCEPT,
    DMTR_OPC_CONNECT,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct dmtr_accept_result_t {
    qd: c_int,
    addr: sockaddr_in,
}

#[repr(C)]
pub union dmtr_qr_value_t {
    pub sga: dmtr_sgarray_t,
    pub ares: dmtr_accept_result_t,
}

#[repr(C)]
pub struct dmtr_qresult_t {
    pub qr_opcode: dmtr_opcode_t,
    pub qr_qd: c_int,
    pub qr_qt: dmtr_qtoken_t,
    pub qr_value: dmtr_qr_value_t,
}

impl dmtr_qresult_t {
    pub fn pack(result: OperationResult, qd: FileDescriptor, qt: u64) -> Self {
        match result {
            OperationResult::Connect => Self {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_CONNECT,
                qr_qd: qd as c_int,
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Accept(new_qd) => {
                let sin = unsafe { mem::zeroed() };
                let qr_value = dmtr_qr_value_t {
                    ares: dmtr_accept_result_t {
                        qd: new_qd as c_int,
                        addr: sin,
                    },
                };
                Self {
                    qr_opcode: dmtr_opcode_t::DMTR_OPC_ACCEPT,
                    qr_qd: qd as c_int,
                    qr_qt: qt,
                    qr_value,
                }
            },
            OperationResult::Push => Self {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_PUSH,
                qr_qd: qd as c_int,
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Pop(addr, bytes) => {
                let mut sga = dmtr_sgarray_t::from(&bytes[..]);
                if let Some(addr) = addr {
                    sga.sga_addr.sin_port = addr.port.into();
                    sga.sga_addr.sin_addr.s_addr = u32::from_le_bytes(addr.addr.octets());
                }
                let qr_value = dmtr_qr_value_t { sga };
                Self {
                    qr_opcode: dmtr_opcode_t::DMTR_OPC_POP,
                    qr_qd: qd as c_int,
                    qr_qt: qt,
                    qr_value,
                }
            },
            OperationResult::Failed(e) => {
                panic!("Unhandled error: {:?}", e);
            },
        }
    }
}
