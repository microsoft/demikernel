//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    libdpdk::{
        rte_socket_id,
        rte_ring,
        rte_mbuf,
        rte_ring_create,
        rte_ring_enqueue,
        rte_ring_dequeue,
        RING_F_SP_ENQ,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================
pub struct DPDKRing {
    ring: *mut rte_ring,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================
impl DPDKRing {
    pub fn new(count: u32) -> Self {
        let ring: *mut rte_ring = unsafe {
            let ptr = rte_ring_create(std::ptr::null(), count, rte_socket_id().try_into().unwrap(), RING_F_SP_ENQ);

            if ptr.is_null() {
                panic!("Could not allocate");
            }

            ptr
        };

        Self { ring }
    }

    pub fn enqueue(&self, obj: *mut rte_mbuf) -> Result<(), Fail> {
        if unsafe { rte_ring_enqueue(self.ring, obj as *mut std::os::raw::c_void ) } == 0 {
            Ok(())
        } else {
            Err(Fail::new(libc::EINVAL, "ring is full"))
        }
    }

    pub fn dequeue(&self) -> Option<*mut rte_mbuf> {
        let mut mbuf: *mut rte_mbuf = std::ptr::null_mut();
        let mbuf_ptr: *mut *mut rte_mbuf = &mut mbuf;

        if unsafe { rte_ring_dequeue(self.ring, mbuf_ptr as *mut *mut std::os::raw::c_void) } == 0 {
            Some(mbuf)   
        } else {
            None
        }
    }
}
