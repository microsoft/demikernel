// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::arrayvec::ArrayVec;
use ::libc::c_void;
use ::runtime::{
    fail::Fail,
    memory::{
        Buffer,
        DataBuffer,
        MemoryRuntime,
    },
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        consts::RECEIVE_BATCH_SIZE,
        types::MacAddress,
        NetworkRuntime,
    },
    scheduler::Scheduler,
    types::{
        demi_sgarray_t,
        demi_sgaseg_t,
    },
    Runtime,
};
use ::std::{
    mem,
    net::Ipv4Addr,
    ptr,
    slice,
};

//==============================================================================
// Structures
//==============================================================================

/// POSIX Runtime
#[derive(Clone)]
pub struct PosixRuntime {
    /// Scheduler
    pub scheduler: Scheduler,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for POSIX Runtime
impl PosixRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for PosixRuntime {
    /// Converts a runtime buffer into a scatter-gather array.
    fn into_sgarray(&self, buf: Buffer) -> Result<demi_sgarray_t, Fail> {
        let len: usize = buf.len();
        let sgaseg: demi_sgaseg_t = match buf {
            Buffer::Heap(dbuf) => {
                let dbuf_ptr: *const [u8] = DataBuffer::into_raw(Clone::clone(&dbuf))?;
                demi_sgaseg_t {
                    sgaseg_buf: dbuf_ptr as *mut c_void,
                    sgaseg_len: len as u32,
                }
            },
        };
        Ok(demi_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Allocates a scatter-gather array.
    fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // Allocate a heap-managed buffer.
        let dbuf: DataBuffer = DataBuffer::new(size)?;
        let dbuf_ptr: *const [u8] = DataBuffer::into_raw(dbuf)?;
        let sgaseg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: dbuf_ptr as *mut c_void,
            sgaseg_len: size as u32,
        };
        Ok(demi_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Releases a scatter-gather array.
    fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "scatter-gather array with invalid size"));
        }

        // Release heap-managed buffer.
        let sgaseg: demi_sgaseg_t = sga.sga_segs[0];
        let (data_ptr, length): (*mut u8, usize) = (sgaseg.sgaseg_buf as *mut u8, sgaseg.sgaseg_len as usize);

        // Convert back raw slice to a heap buffer and drop allocation.
        DataBuffer::from_raw_parts(data_ptr, length)?;

        Ok(())
    }

    /// Clones a scatter-gather array.
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<Buffer, Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "scatter-gather array with invalid size"));
        }

        let sgaseg: demi_sgaseg_t = sga.sga_segs[0];
        let (ptr, len): (*mut c_void, usize) = (sgaseg.sgaseg_buf, sgaseg.sgaseg_len as usize);

        Ok(Buffer::Heap(DataBuffer::from_slice(unsafe {
            slice::from_raw_parts(ptr as *const u8, len)
        })))
    }
}

/// Network Runtime Trait Implementation for POSIX Runtime
impl NetworkRuntime for PosixRuntime {
    // TODO: Rely on a default implementation for this.
    fn transmit(&self, _pkt: impl runtime::network::PacketBuf) {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn receive(&self) -> ArrayVec<Buffer, RECEIVE_BATCH_SIZE> {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn local_link_addr(&self) -> MacAddress {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn local_ipv4_addr(&self) -> Ipv4Addr {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn arp_options(&self) -> ArpConfig {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn tcp_options(&self) -> TcpConfig {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn udp_options(&self) -> UdpConfig {
        unreachable!()
    }
}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for PosixRuntime {}
