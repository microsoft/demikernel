use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        scheduler::{
            Yielder,
            YielderHandle,
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
    collections::HashMap,
    net::SocketAddr,
};

pub type SocketDescriptor = usize;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Underlying network transport.
pub struct CatnapTransport {
    _handles: HashMap<usize, YielderHandle>,
}

#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

impl SharedCatnapTransport {
    pub fn new(_config: &Config, mut _runtime: SharedDemiRuntime) -> Self {
        unimplemented!("this function is missing")
    }

    pub fn socket(&mut self, _domain: Domain, _typ: Type) -> Result<SocketDescriptor, Fail> {
        unimplemented!("this function is missing")
    }

    pub fn bind(&mut self, _sd: &mut SocketDescriptor, _local: SocketAddr) -> Result<(), Fail> {
        unimplemented!("this function is missing")
    }

    pub fn listen(&mut self, _sd: &mut SocketDescriptor, _backlog: usize) -> Result<(), Fail> {
        unimplemented!("this function is missing")
    }

    pub async fn accept(
        &mut self,
        _sd: &mut SocketDescriptor,
        _yielder: Yielder,
    ) -> Result<(SocketDescriptor, SocketAddr), Fail> {
        unimplemented!("this function is missing")
    }

    pub async fn connect(
        &mut self,
        _sd: &mut SocketDescriptor,
        _remote: SocketAddr,
        _yielder: Yielder,
    ) -> Result<(), Fail> {
        unimplemented!("this function is missing")
    }

    pub async fn push(
        &mut self,
        _sd: &mut SocketDescriptor,
        _buf: &mut DemiBuffer,
        _addr: Option<SocketAddr>,
        _yielder: Yielder,
    ) -> Result<(), Fail> {
        unimplemented!("this function is missing")
    }

    pub async fn pop(
        &mut self,
        _sd: &mut SocketDescriptor,
        _buf: &mut DemiBuffer,
        _size: usize,
        _yielder: Yielder,
    ) -> Result<Option<SocketAddr>, Fail> {
        unimplemented!("this function is missing")
    }

    pub fn close(&mut self, _sd: &mut SocketDescriptor) -> Result<(), Fail> {
        unimplemented!("this function is missing")
    }

    pub async fn async_close(&mut self, _sd: &mut SocketDescriptor, _yielder: Yielder) -> Result<(), Fail> {
        unimplemented!("this function is missing")
    }

    #[allow(dead_code)]
    pub async fn poll(&mut self) {
        unimplemented!("this function is missing")
    }
}
