/// Transport API. This represents a higher level network API with full socket functionality.
use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        scheduler::Yielder,
        SharedDemiRuntime,
    },
};
use ::socket2::{
    Domain,
    Type,
};
use ::std::{
    fmt::Debug,
    net::SocketAddr,
};

//======================================================================================================================
// Trait Definition
//======================================================================================================================

/// This trait represents a high-level network API that supports both connection-based and connection-less
/// communication using sockets.
pub trait NetworkTransport: Clone + 'static {
    type SocketDescriptor: Debug;

    /// Create a new network transport.
    fn new(config: &Config, runtime: &mut SharedDemiRuntime) -> Self;

    /// Create a socket using the network transport layer.
    fn socket(&mut self, domain: Domain, typ: Type) -> Result<Self::SocketDescriptor, Fail>;

    /// Bind an address to the socket.
    fn bind(&mut self, sd: &mut Self::SocketDescriptor, local: SocketAddr) -> Result<(), Fail>;

    /// Listen on this socket in the network transport layer.
    fn listen(&mut self, sd: &mut Self::SocketDescriptor, backlog: usize) -> Result<(), Fail>;

    /// Forcibly close this socket in the network transport layer. This function should only be used in Drop and other
    /// internal functions, never exposed to the application.
    fn hard_close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail>;

    /// Asynchronously accept a new connection on a listening socket.
    fn accept(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        yielder: Yielder,
    ) -> impl std::future::Future<Output = Result<(Self::SocketDescriptor, SocketAddr), Fail>>;

    /// Asynchronously connect this socket to [remote].
    fn connect(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        remote: SocketAddr,
        yielder: Yielder,
    ) -> impl std::future::Future<Output = Result<(), Fail>>;

    /// Push data to a connected socket.
    fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
        yielder: Yielder,
    ) -> impl std::future::Future<Output = Result<(), Fail>>;

    /// Pop data from a connected socket.
    fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: Yielder,
    ) -> impl std::future::Future<Output = Result<Option<SocketAddr>, Fail>>;

    /// Asynchronously close a socket.
    fn close(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        yielder: Yielder,
    ) -> impl std::future::Future<Output = Result<(), Fail>>;
}
