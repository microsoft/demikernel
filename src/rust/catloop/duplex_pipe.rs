// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    catmem::CatmemLibOS,
    demi_sgarray_t,
    runtime::fail::Fail,
    scheduler::SchedulerHandle,
    QDesc,
    QToken,
};
use ::std::{
    cell::RefCell,
    net::Ipv4Addr,
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A bidirectional pipe.
///
/// This is a convenience data structure that facilitates operating on two
/// unidirectional (simplex) pipes that are logically related.
#[derive(Clone)]
pub struct DuplexPipe {
    /// Underlying Catmem LibOS.
    catmem: Rc<RefCell<CatmemLibOS>>,
    // Simplex pipe used for receiving data.
    rx: QDesc,
    // Simplex pipe used for transmitting data.
    tx: QDesc,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl DuplexPipe {
    /// Returns the queue descriptor that is associated with the underlying receiving simplex pipe.
    pub fn tx(&self) -> QDesc {
        self.tx
    }

    /// Returns the queue descriptor that is associated with the underlying receiving simplex pipe.
    pub fn rx(&self) -> QDesc {
        self.rx
    }

    /// Creates a duplex pipe.
    pub fn create_duplex_pipe(catmem: Rc<RefCell<CatmemLibOS>>, ipv4: &Ipv4Addr, port: u16) -> Result<Self, Fail> {
        let rx: QDesc = catmem.borrow_mut().create_pipe(&format!("{}:{}:rx", ipv4, port))?;
        let tx: QDesc = catmem.borrow_mut().create_pipe(&format!("{}:{}:tx", ipv4, port))?;
        Ok(Self { catmem, rx, tx })
    }

    /// Opens a duplex pipe.
    pub fn open_duplex_pipe(catmem: Rc<RefCell<CatmemLibOS>>, ipv4: &Ipv4Addr, port: u16) -> Result<Self, Fail> {
        // Note: the rx and tx are intentionally flipped in the formatting string bellow.
        let rx: QDesc = catmem.borrow_mut().open_pipe(&format!("{}:{}:tx", ipv4, port))?;
        let tx: QDesc = catmem.borrow_mut().open_pipe(&format!("{}:{}:rx", ipv4, port))?;
        Ok(Self { catmem, rx, tx })
    }

    /// Closes a duplex pipe.
    pub fn close(&self) -> Result<(), Fail> {
        self.catmem.borrow_mut().shutdown(self.rx)?;
        self.catmem.borrow_mut().close(self.tx)?;
        Ok(())
    }

    /// Disallows further operations on a duplex pipe. The underlying queue
    /// descriptors are released, but EoF is not pushed to the remote end.
    pub fn shutdown(&self) -> Result<(), Fail> {
        self.catmem.borrow_mut().shutdown(self.rx)?;
        self.catmem.borrow_mut().shutdown(self.tx)?;
        Ok(())
    }

    /// Pushes a scatter-gather array to a duplex pipe.
    pub fn push(&self, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        self.catmem.borrow_mut().push(self.tx, &sga)
    }

    /// Pops a scatter-gather array from a duplex pipe.
    pub fn pop(&self, size: Option<usize>) -> Result<QToken, Fail> {
        self.catmem.borrow_mut().pop(self.rx, size)
    }

    /// Polls a duplex pipe.
    pub fn poll(catmem: &Rc<RefCell<CatmemLibOS>>, qt: QToken) -> Result<Option<SchedulerHandle>, Fail> {
        let mut handle: SchedulerHandle = catmem.borrow_mut().schedule(qt)?;

        // Return this operation to the scheduling queue by removing the associated key
        // (which would otherwise cause the operation to be freed).
        if !handle.has_completed() {
            handle.take_key();
            return Ok(None);
        }

        Ok(Some(handle))
    }

    /// Drops a pending operation on a duplex pipe.
    pub fn drop(catmem: &Rc<RefCell<CatmemLibOS>>, qt: QToken) -> Result<(), Fail> {
        // Retrieve a handle to the concerned co-routine and execute its destructor.
        let handle: SchedulerHandle = catmem.borrow_mut().schedule(qt)?;
        drop(handle);
        Ok(())
    }
}
