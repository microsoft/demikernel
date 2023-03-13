// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    queue::IoQueue,
};
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
/// TODO: This should be byte-oriented, not buffer-oriented.
pub struct SharedQueue<T> {
    /// Send-side endpoint.
    tx: Rc<RefCell<Sender<T>>>,
    /// Receive-side endpoint.
    rx: Rc<RefCell<Receiver<T>>>,
    /// Length of shared queue.
    length: Rc<RefCell<usize>>,
    /// Capacity of shared queue.
    capacity: usize,
}

/// Per-queue metadata for a UDP socket.
pub struct UdpQueue {
    addr: Option<SocketAddrV4>,
    recv_queue: Option<SharedQueue<SharedQueueSlot<DemiBuffer>>>,
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

/// Getters and setters for per UDP queue metadata.
impl UdpQueue {
    pub fn new() -> Self {
        Self {
            addr: None,
            recv_queue: None,
        }
    }

    /// Check whether the queue/socket is bound to an address.
    pub fn is_bound(&self) -> bool {
        self.addr != None
    }

    /// Get the address assigned to this socket.
    pub fn get_addr(&self) -> Result<SocketAddrV4, Fail> {
        match self.addr {
            Some(addr) => Ok(addr.clone()),
            None => Err(Fail::new(libc::ENOTCONN, "port not bound")),
        }
    }

    /// Get the recv queue associated with this socket.
    pub fn get_recv_queue(&self) -> SharedQueue<SharedQueueSlot<DemiBuffer>> {
        match &self.recv_queue {
            Some(recv) => recv.clone(),
            None => panic!("No allocated receive queue!"),
        }
    }

    /// Set the address assigned to this socket/Demikernel queue.
    pub fn set_addr(&mut self, addr: SocketAddrV4) {
        self.addr = Some(addr);
    }

    /// Set the recv_queue for this socket/Demikernel queue.
    pub fn set_recv_queue(&mut self, queue: SharedQueue<SharedQueueSlot<DemiBuffer>>) {
        self.recv_queue = Some(queue);
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Clone Trait Implementation for Shared Queues.
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

/// IoQueue Trait Implementation for UDP Queues.
impl IoQueue for UdpQueue {
    fn get_qtype(&self) -> crate::QType {
        crate::QType::UdpSocket
    }
}
