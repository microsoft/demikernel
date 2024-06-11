use std::{
    cell::RefCell,
    rc::Rc,
};

#[derive(Clone)]
pub struct UmemReg {
    chunk_size: u32,
    mem: Rc<RefCell<xdp_rs::XSK_UMEM_REG>>,
}

impl UmemReg {
    pub fn new(count: u32, chunk_size: u32) -> Self {
        let total_size: u64 = count as u64 * chunk_size as u64;
        let mut buffer: Vec<u8> = Vec::<u8>::with_capacity(total_size as usize);

        let mem: Rc<RefCell<xdp_rs::XSK_UMEM_REG>> = Rc::new(RefCell::new(xdp_rs::XSK_UMEM_REG {
            TotalSize: total_size,
            ChunkSize: chunk_size,
            Headroom: 0,
            Address: buffer.as_mut_ptr() as *mut core::ffi::c_void,
        }));

        Self { mem, chunk_size }
    }

    pub fn as_ptr(&self) -> *mut xdp_rs::XSK_UMEM_REG {
        self.mem.as_ptr()
    }

    pub fn get_address(&self) -> *mut core::ffi::c_void {
        self.mem.borrow().Address
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }
}
