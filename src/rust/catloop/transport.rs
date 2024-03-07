//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catloop::socket::SharedMemorySocket,
    catmem::SharedCatmemLibOS,
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            transport::NetworkTransport,
            unwrap_socketaddr,
        },
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::socket2::{
    Domain,
    Type,
};
use ::std::{
    net::{
        SocketAddr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// [CatloopTransport] builds a socket-based transport on top of shared memory queues provided by Catmem.
/// [CatloopLibOS] is stateless and purely contains multi-queue functionality necessary to run the Catloop libOS. All
/// state is kept in the [state], while [runtime] holds the coroutine scheduler and [catmem] holds a reference to the
/// underlying Catmem libOS instance.
pub struct CatloopTransport {
    /// Underlying transport.
    catmem: SharedCatmemLibOS,
    /// Underlying coroutine runtime.
    runtime: SharedDemiRuntime,
    /// Configuration.
    config: Config,
}

#[derive(Clone)]
pub struct SharedCatloopTransport(SharedObject<CatloopTransport>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedCatloopTransport {
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Self {
        Self(SharedObject::new(CatloopTransport {
            catmem: SharedCatmemLibOS::new(config, runtime.clone()),
            runtime,
            config: config.clone(),
        }))
    }
}

impl NetworkTransport for SharedCatloopTransport {
    // We use the Catmem queue descriptor as the underlying transport socket descriptor.
    type SocketDescriptor = SharedMemorySocket;

    /// Creates a socket. This function contains the libOS-level functionality needed to create a SharedCatloopQueue
    /// that wraps the underlying Catmem queue.
    fn socket(&mut self, _: Domain, _: Type) -> Result<Self::SocketDescriptor, Fail> {
        // Create fake socket.
        Ok(SharedMemorySocket::new())
    }

    /// Binds a socket to a local endpoint. This function contains the libOS-level functionality needed to bind a
    /// SharedCatloopQueue to a local address.
    fn bind(&mut self, sd: &mut Self::SocketDescriptor, local: SocketAddr) -> Result<(), Fail> {
        // Check if we are binding to a non-local address.
        let local: SocketAddrV4 = unwrap_socketaddr(local)?;
        if &self.config.local_ipv4_addr() != local.ip() {
            let cause: String = format!("cannot bind to non-local address (sd={:?})", sd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRNOTAVAIL, &cause));
        }

        // Check that the socket associated with the queue is not listening.
        sd.bind(local, &mut self.catmem)
    }

    /// Sets a SharedCatloopQueue and as a passive one. This function contains the libOS-level
    /// functionality to move the SharedCatloopQueue into a listening state.
    fn listen(&mut self, sd: &mut Self::SocketDescriptor, backlog: usize) -> Result<(), Fail> {
        sd.listen(backlog)
    }

    /// Asynchronous cross-queue code for accepting a connection. This function returns a coroutine that runs
    /// asynchronously to accept a connection and performs any necessary multi-queue operations at the libOS-level after
    /// the accept succeeds or fails.
    async fn accept(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(Self::SocketDescriptor, SocketAddr), Fail> {
        let new_port: u16 = self.runtime.alloc_ephemeral_port()?;
        match sd.accept(new_port, self.catmem.clone()).await {
            Ok(new_socket) => Ok(new_socket),
            Err(e) => {
                self.runtime.free_ephemeral_port(new_port)?;
                Err(e)
            },
        }
    }

    /// Asynchronous code to establish a connection to a remote endpoint. This function returns a coroutine that runs
    /// asynchronously to connect a queue and performs any necessary multi-queue operations at the libOS-level after
    /// the connect succeeds or fails.
    async fn connect(&mut self, sd: &mut Self::SocketDescriptor, remote: SocketAddr) -> Result<(), Fail> {
        // Wait for connect operation to complete.
        sd.connect(self.catmem.clone(), remote).await
    }

    /// Asynchronous code to close a queue. This function returns a coroutine that runs asynchronously to close a queue
    /// and the underlying Catmem queue and performs any necessary multi-queue operations at the libOS-level after
    /// the close succeeds or fails.
    async fn close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        sd.close(self.catmem.clone()).await
    }

    fn hard_close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        sd.hard_close(&mut self.catmem)
    }

    /// Asynchronous code to push to a Catloop queue.
    async fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        _: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        // Wait for push to complete.
        sd.push(self.catmem.clone(), buf).await
    }

    /// Coroutine to pop from a Catloop queue.
    async fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        size: usize,
    ) -> Result<Option<SocketAddr>, Fail> {
        sd.pop(self.catmem.clone(), buf, size).await
    }

    fn get_runtime(&self) -> &SharedDemiRuntime {
        &self.runtime
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedCatloopTransport {
    type Target = CatloopTransport;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatloopTransport {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
