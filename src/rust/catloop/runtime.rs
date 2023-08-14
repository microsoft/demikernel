// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    catloop::{
        CatloopQueue,
        Operation,
        OperationTask,
        Socket,
    },
    inetstack::protocols::ip::EphemeralPorts,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        queue::{
            IoQueueTable,
            QDesc,
            QToken,
            QType,
        },
        types::demi_opcode_t,
    },
};
use ::rand::{
    prelude::SmallRng,
    SeedableRng,
};
use ::std::{
    collections::HashMap,
    net::SocketAddrV4,
    pin::Pin,
};
//==============================================================================
// Structures
//==============================================================================

/// Catloop Runtime
pub struct CatloopRuntime {
    /// Ephemeral port allocator.
    ephemeral_ports: EphemeralPorts,
    /// Table of queue descriptors. This table has one entry for each existing queue descriptor in Catloop LibOS.
    qtable: IoQueueTable<CatloopQueue>,
    /// Table for ongoing operations.
    catloop_qts: HashMap<QToken, (demi_opcode_t, QDesc)>,
    /// Table for ongoing operations.
    catmem_qts: HashMap<QToken, (demi_opcode_t, QDesc)>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Catloop Runtime. This data structure holds all of the cross-queue state for the Catloop libOS.
impl CatloopRuntime {
    pub fn new() -> Self {
        let mut rng: SmallRng = SmallRng::from_entropy();
        Self {
            ephemeral_ports: EphemeralPorts::new(&mut rng),
            qtable: IoQueueTable::<CatloopQueue>::new(),
            catmem_qts: HashMap::default(),
            catloop_qts: HashMap::default(),
        }
    }

    /// This function allocates a new Catloop queue of [qtype].
    pub fn alloc_queue(&mut self, qtype: QType) -> QDesc {
        self.qtable.alloc(CatloopQueue::new(qtype))
    }

    pub fn free_queue(&mut self, qd: QDesc) {
        self.qtable.free(&qd);
    }

    /// Get the CatloopQueue associated with this [qd]. If not a valid queue, then return EBADF with "invalid queue
    /// descriptor".
    pub fn get_queue(&mut self, qd: QDesc) -> Result<&mut CatloopQueue, Fail> {
        match self.qtable.get_mut(&qd) {
            Some(queue) => Ok(queue),
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_queue(): {}", cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    /// This function inserts a given queue token into the Catloop queue token table.
    pub fn insert_catloop_qt(&mut self, qt: QToken, opcode: demi_opcode_t, qd: QDesc) {
        self.catloop_qts.insert(qt, (opcode, qd));
    }

    /// This function inserts a given queue token into the Catmem queue token table.
    pub fn insert_catmem_qt(&mut self, qt: QToken, opcode: demi_opcode_t, qd: QDesc) {
        self.catmem_qts.insert(qt, (opcode, qd));
    }

    /// This function gets all current queue tokens in the Catloop queue token table.
    pub fn get_catloop_qt(&self, qt: QToken) -> Option<&(demi_opcode_t, QDesc)> {
        self.catloop_qts.get(&qt)
    }

    /// This function gets all current queue tokens in the Catmem queue token table.
    pub fn get_catmem_qt(&self, qt: QToken) -> Option<&(demi_opcode_t, QDesc)> {
        self.catmem_qts.get(&qt)
    }

    /// This function gets all current queue tokens in the Catloop queue token table.
    pub fn free_catloop_qt(&mut self, qt: QToken) -> Option<(demi_opcode_t, QDesc)> {
        self.catloop_qts.remove(&qt)
    }

    /// This function gets all current queue tokens in the Catmem queue token table.
    pub fn free_catmem_qt(&mut self, qt: QToken) -> Option<(demi_opcode_t, QDesc)> {
        self.catmem_qts.remove(&qt)
    }

    /// This function checks whether [local] is already bound to a queue. Returns true if not bound and false if
    /// already in use.
    pub fn check_bind_addr(&self, local: SocketAddrV4) -> bool {
        for (_, queue) in self.qtable.get_values() {
            match queue.get_socket() {
                Socket::Active(Some(addr)) | Socket::Passive(addr) if addr == local => return false,
                _ => continue,
            }
        }
        true
    }

    /// This function allocates an ephemeral port.
    pub fn alloc_ephemeral_port(&mut self, port: Option<u16>) -> Result<Option<u16>, Fail> {
        if let Some(port) = port {
            self.ephemeral_ports.alloc_port(port)?;
            Ok(None)
        } else {
            Ok(Some(self.ephemeral_ports.alloc_any()?))
        }
    }

    pub fn free_ephemeral_port(&mut self, port: u16) -> Result<(), Fail> {
        self.ephemeral_ports.free(port)
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for Catloop Runtime
impl MemoryRuntime for CatloopRuntime {}

impl Drop for CatloopRuntime {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for (_, queue) in self.qtable.get_values() {
            if let Some(duplex_pipe) = queue.get_pipe() {
                if duplex_pipe.close().is_err() {
                    warn!("drop(): failed to close duplex pipe");
                }
            }
            if let Socket::Active(Some(addr)) | Socket::Passive(addr) = queue.get_socket() {
                if EphemeralPorts::is_private(addr.port()) {
                    if self.ephemeral_ports.free(addr.port()).is_err() {
                        warn!("drop(): leaking ephemeral port (port={})", addr.port());
                    }
                }
            }
        }
    }
}
