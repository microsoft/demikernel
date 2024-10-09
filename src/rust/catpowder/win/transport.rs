// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

//======================================================================================================================
// Structures
//======================================================================================================================

use std::{
    future::Future,
    net::{SocketAddr, SocketAddrV4},
};

use crate::{
    catpowder::win::runtime::SharedCatpowderRuntime,
    demi_sgarray_t,
    inetstack::SharedInetStack,
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        network::transport::NetworkTransport,
        SharedDemiRuntime, SharedObject,
    },
    SocketOption,
};

/// Underlying network transport.
pub struct CatpowderTransport {
    /// Catpowder runtime instance.
    runtime: SharedCatpowderRuntime,

    /// Underlying inet stack.
    inet_stack: SharedInetStack,
}

/// A network transport built on top of Windows overlapped I/O.
#[derive(Clone)]
pub struct SharedCatpowderTransport(SharedObject<CatpowderTransport>);

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl NetworkTransport for SharedCatpowderTransport {
    type SocketDescriptor = <SharedInetStack as NetworkTransport>::SocketDescriptor;

    fn socket(&mut self, domain: socket2::Domain, typ: socket2::Type) -> Result<Self::SocketDescriptor, Fail> {
        self.0.inet_stack.socket(domain, typ)
    }

    fn set_socket_option(&mut self, sd: &mut Self::SocketDescriptor, option: SocketOption) -> Result<(), Fail> {
        self.0.inet_stack.set_socket_option(sd, option)
    }

    fn get_socket_option(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        option: SocketOption,
    ) -> Result<SocketOption, Fail> {
        self.0.inet_stack.get_socket_option(sd, option)
    }

    fn getpeername(&mut self, sd: &mut Self::SocketDescriptor) -> Result<SocketAddrV4, Fail> {
        self.0.inet_stack.getpeername(sd)
    }

    fn bind(&mut self, sd: &mut Self::SocketDescriptor, local: std::net::SocketAddr) -> Result<(), Fail> {
        self.0.inet_stack.bind(sd, local)?;

        Ok(())
    }

    fn listen(&mut self, sd: &mut Self::SocketDescriptor, backlog: usize) -> Result<(), Fail> {
        self.0.inet_stack.listen(sd, backlog)
    }

    fn hard_close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        self.0.inet_stack.hard_close(sd)
    }

    fn accept(
        &mut self,
        sd: &mut Self::SocketDescriptor,
    ) -> impl Future<Output = Result<(Self::SocketDescriptor, SocketAddr), Fail>> {
        self.0.inet_stack.accept(sd)
    }

    fn connect(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        remote: SocketAddr,
    ) -> impl Future<Output = Result<(), Fail>> {
        self.0.inet_stack.connect(sd, remote)
    }

    fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> impl Future<Output = Result<(), Fail>> {
        self.0.inet_stack.push(sd, buf, addr)
    }

    fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        size: usize,
    ) -> impl Future<Output = Result<(Option<SocketAddr>, DemiBuffer), Fail>> {
        self.0.inet_stack.pop(sd, size)
    }

    fn close(&mut self, sd: &mut Self::SocketDescriptor) -> impl Future<Output = Result<(), Fail>> {
        self.0.inet_stack.close(sd)
    }

    fn get_runtime(&self) -> &SharedDemiRuntime {
        self.0.inet_stack.get_runtime()
    }
}

impl MemoryRuntime for SharedCatpowderTransport {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.0.inet_stack.clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.0.inet_stack.into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.0.inet_stack.sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.0.inet_stack.sgafree(sga)
    }
}
