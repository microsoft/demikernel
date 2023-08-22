// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catloop::CatloopQueue,
    inetstack::protocols::ip::EphemeralPorts,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        queue::{
            IoQueueTable,
            QDesc,
        },
    },
};
use ::rand::{
    prelude::SmallRng,
    SeedableRng,
};
use ::std::net::SocketAddrV4;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catloop Runtime
pub struct CatloopRuntime {
    /// Ephemeral port allocator.
    ephemeral_ports: EphemeralPorts,
    /// Table of queue descriptors, it has one entry for each existing queue descriptor in Catloop LibOS.
    qtable: IoQueueTable<CatloopQueue>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Catloop Runtime. This data structure holds all of the cross-queue state for the Catloop libOS.
impl CatloopRuntime {
    pub fn new() -> Self {
        let mut rng: SmallRng = SmallRng::from_entropy();
        Self {
            ephemeral_ports: EphemeralPorts::new(&mut rng),
            qtable: IoQueueTable::<CatloopQueue>::new(),
        }
    }

    /// Allocates a new [CatloopQueue].
    pub fn alloc_queue(&mut self, queue: CatloopQueue) -> QDesc {
        self.qtable.alloc(queue)
    }

    pub fn free_queue(&mut self, qd: QDesc) {
        self.qtable.free(&qd);
    }

    /// Gets the [CatloopQueue] associated with `qd`. If not `qd` does not refer to a valid, then return `EBADF` is returned.
    pub fn get_queue(&self, qd: QDesc) -> Result<CatloopQueue, Fail> {
        match self.qtable.get(&qd) {
            Some(queue) => Ok(queue.clone()),
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_queue(): {}", cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    /// Checks whether `local` is bound to `addr`. On successful completion it returns `true` if not bound and `false` if
    /// already in use.
    pub fn is_bound_to_addr(&self, local: SocketAddrV4) -> bool {
        for (_, queue) in self.qtable.get_values() {
            match queue.local() {
                Some(addr) if addr == local => return false,
                _ => continue,
            }
        }
        true
    }

    /// Allocates an ephemeral port. If `port` is `Some(port)` then it tries to allocate `port`.
    pub fn alloc_ephemeral_port(&mut self, port: Option<u16>) -> Result<Option<u16>, Fail> {
        if let Some(port) = port {
            self.ephemeral_ports.alloc_port(port)?;
            Ok(None)
        } else {
            Ok(Some(self.ephemeral_ports.alloc_any()?))
        }
    }

    /// Releases an ephemeral `port`.
    pub fn free_ephemeral_port(&mut self, port: u16) -> Result<(), Fail> {
        self.ephemeral_ports.free(port)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory Runtime Trait Implementation for Catloop Runtime
impl MemoryRuntime for CatloopRuntime {}

impl Drop for CatloopRuntime {
    /// Releases all resources allocated by Catloop.
    fn drop(&mut self) {
        for (qd, queue) in self.qtable.get_values() {
            if let Err(e) = queue.close() {
                warn!("drop(): leaking qd={:?} (e={:?})", qd, e);
            }
            if let Some(addr) = queue.local() {
                if EphemeralPorts::is_private(addr.port()) {
                    if self.ephemeral_ports.free(addr.port()).is_err() {
                        warn!("drop(): leaking ephemeral port (port={})", addr.port());
                    }
                }
            }
        }
    }
}
