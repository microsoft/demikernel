mod background;
pub mod state;

use self::{
    background::background,
    state::ControlBlock,
};
use crate::{
    fail::Fail,
    protocols::{
        ipv4,
        tcp::segment::TcpHeader,
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
    sync::Bytes,
};
use std::{
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

pub struct EstablishedSocket<RT: Runtime> {
    pub cb: Rc<ControlBlock<RT>>,
    #[allow(unused)]
    background_work: SchedulerHandle,
}

impl<RT: Runtime> EstablishedSocket<RT> {
    pub fn new(cb: ControlBlock<RT>) -> Self {
        let cb = Rc::new(cb);
        let future = background(cb.clone());
        let handle = cb.rt.spawn(future);
        Self {
            cb: cb.clone(),
            background_work: handle,
        }
    }

    pub fn receive(&self, header: &TcpHeader, data: Bytes) {
        self.cb.receive(header, data)
    }

    pub fn send(&self, buf: Bytes) -> Result<(), Fail> {
        self.cb.sender.send(buf, &self.cb)
    }

    pub fn peek(&self) -> Result<Bytes, Fail> {
        self.cb.receiver.peek()
    }

    pub fn recv(&self) -> Result<Option<Bytes>, Fail> {
        self.cb.receiver.recv()
    }

    pub fn poll_recv(&self, ctx: &mut Context) -> Poll<Result<Bytes, Fail>> {
        self.cb.receiver.poll_recv(ctx)
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.cb.close()
    }

    pub fn remote_mss(&self) -> usize {
        self.cb.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.cb.current_rto()
    }

    pub fn endpoints(&self) -> (ipv4::Endpoint, ipv4::Endpoint) {
        (self.cb.local.clone(), self.cb.remote.clone())
    }
}
