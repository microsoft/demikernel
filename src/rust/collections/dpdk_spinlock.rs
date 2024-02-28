//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    pal::arch,
    runtime::libdpdk::{
        rte_spinlock_t,
        rte_spinlock_lock,
        rte_spinlock_trylock,
        rte_spinlock_unlock,
        rte_zmalloc,
    }
};

//======================================================================================================================
// Structures
//======================================================================================================================
pub struct DPDKSpinLock {
    locked: *mut rte_spinlock_t,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================
impl DPDKSpinLock {
    pub fn new() -> Self {
        let locked: *mut rte_spinlock_t = unsafe { 
            let ptr: *mut rte_spinlock_t = rte_zmalloc(std::ptr::null(), std::mem::size_of::<rte_spinlock_t>(), arch::CPU_DATA_CACHE_LINE_SIZE.try_into().unwrap()) as *mut rte_spinlock_t;
            
            if ptr.is_null() {
                panic!("Could not allocate");
            }

            ptr
        };

        Self { locked }
    }

    pub fn set(&mut self) {
        unsafe { (*self.locked).locked = 1 }
    }

    pub fn lock(&self) {
        unsafe { rte_spinlock_lock(self.locked) }
    }

    pub fn trylock(&self) -> bool {
        let ret: i32 = unsafe { rte_spinlock_trylock(self.locked) };
        
        ret == 1
    }
    
    pub fn unlock(&self) {
        unsafe { rte_spinlock_unlock(self.locked) }
    }
}
