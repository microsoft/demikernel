// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    memory::{
        DemiBuffer,
        MemoryRuntime,
    },
    SharedDemiRuntime,
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
pub trait NetworkTransport: Clone + 'static + MemoryRuntime {
    type SocketDescriptor: Debug;

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
    ) -> impl std::future::Future<Output = Result<(Self::SocketDescriptor, SocketAddr), Fail>>;

    /// Asynchronously connect this socket to [remote].
    fn connect(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        remote: SocketAddr,
    ) -> impl std::future::Future<Output = Result<(), Fail>>;

    /// Push data to a connected socket.
    fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> impl std::future::Future<Output = Result<(), Fail>>;

    /// Pop data from a connected socket.
    fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        size: usize,
    ) -> impl std::future::Future<Output = Result<(Option<SocketAddr>, DemiBuffer), Fail>>;

    /// Asynchronously close a socket.
    fn close(&mut self, sd: &mut Self::SocketDescriptor) -> impl std::future::Future<Output = Result<(), Fail>>;

    /// Pull the common runtime out of the transport. We only need this because traits do not support members.
    fn get_runtime(&self) -> &SharedDemiRuntime;
}
