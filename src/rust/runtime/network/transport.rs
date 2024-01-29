// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demi_sgarray_t,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
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
        buf: &mut DemiBuffer,
        size: usize,
    ) -> impl std::future::Future<Output = Result<Option<SocketAddr>, Fail>>;

    /// Asynchronously close a socket.
    fn close(&mut self, sd: &mut Self::SocketDescriptor) -> impl std::future::Future<Output = Result<(), Fail>>;

    /// Pull the common runtime out of the transport. We only need this because traits do not support members.
    fn get_runtime(&self) -> &SharedDemiRuntime;
}

impl<N: NetworkTransport> MemoryRuntime for N {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.get_runtime().clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.get_runtime().into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.get_runtime().sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.get_runtime().sgafree(sga)
    }
}
