// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::futures::{
    channel::mpsc::{
        self,
        Receiver,
        Sender,
    },
    StreamExt,
};
use ::libc::EIO;
use ::std::{
    cell::RefCell,
    net::SocketAddrV4,
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Shared Queue Slot
pub struct SharedQueueSlot<T> {
    /// Local endpoint.
    pub local: SocketAddrV4,
    /// Remote endpoint.
    pub remote: SocketAddrV4,
    /// Associated data.
    pub data: T,
}

/// Shared Queue
///
/// TODO: Reuse this structure in TCP stack, for send/receive queues.
pub struct SharedQueue<T> {
    /// Send-side endpoint.
    tx: Rc<RefCell<Sender<T>>>,
    /// Receive-side endpoint.
    rx: Rc<RefCell<Receiver<T>>>,
    /// Length of shared queue.
    length: Rc<RefCell<usize>>,
    /// Capacity of shahred queue.
    capacity: usize,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated Functions Shared Queues
impl<T> SharedQueue<T> {
    /// Instantiates a shared queue.
    pub fn new(size: usize) -> Self {
        let (tx, rx): (Sender<T>, Receiver<T>) = mpsc::channel(size);
        Self {
            tx: Rc::new(RefCell::new(tx)),
            rx: Rc::new(RefCell::new(rx)),
            length: Rc::new(RefCell::new(0)),
            capacity: size,
        }
    }

    /// Pushes a message to the target shared queue.
    #[allow(unused_must_use)]
    pub fn push(&self, msg: T) -> Result<(), Fail> {
        if *self.length.borrow() == self.capacity {
            while *self.length.borrow() >= self.capacity / 2 {
                self.try_pop();
            }
        }

        match self.tx.borrow_mut().try_send(msg) {
            Ok(_) => {
                *self.length.borrow_mut() += 1;
                Ok(())
            },
            Err(_) => Err(Fail::new(EIO, "failed to push to shared queue")),
        }
    }

    /// Synchronously attempts to pop a message from the target shared queue.
    pub fn try_pop(&self) -> Result<Option<T>, Fail> {
        match self.rx.borrow_mut().try_next() {
            Ok(Some(msg)) => {
                *self.length.borrow_mut() -= 1;
                Ok(Some(msg))
            },
            Ok(None) => Err(Fail::new(EIO, "failed to pop from shared queue")),
            Err(_) => Ok(None),
        }
    }

    /// Asynchronously pops a message from the target shared queue.
    pub async fn pop(&mut self) -> Result<T, Fail> {
        match self.rx.borrow_mut().next().await {
            Some(msg) => {
                *self.length.borrow_mut() -= 1;
                Ok(msg)
            },
            None => Err(Fail::new(EIO, "failed to pop from shared queue")),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Clone Trait Implementation for Shared Queues
impl<T> Clone for SharedQueue<T> {
    /// Clones the target [SharedQueue].
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            rx: self.rx.clone(),
            length: self.length.clone(),
            capacity: self.capacity,
        }
    }
}
