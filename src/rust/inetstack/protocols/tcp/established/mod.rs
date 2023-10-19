// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod background;
pub mod congestion_control;
mod ctrlblk;
mod rto;
mod sender;

pub use self::ctrlblk::{
    SharedControlBlock,
    State,
};

use crate::{
    inetstack::protocols::tcp::segment::TcpHeader,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        QDesc,
        SharedDemiRuntime,
    },
    scheduler::TaskHandle,
};
use ::futures::channel::mpsc;
use ::std::{
    net::SocketAddrV4,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

#[derive(Clone)]
pub struct EstablishedSocket<const N: usize> {
    pub cb: SharedControlBlock<N>,
    /// The background co-routines handles various tasks, such as retransmission and acknowledging.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: TaskHandle,
}

impl<const N: usize> EstablishedSocket<N> {
    pub fn new(
        cb: SharedControlBlock<N>,
        qd: QDesc,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        mut runtime: SharedDemiRuntime,
    ) -> Result<Self, Fail> {
        // TODO: Maybe add the queue descriptor here.
        let handle: TaskHandle = runtime.insert_background_coroutine(
            "Inetstack::TCP::established::background",
            Box::pin(background::background(cb.clone(), qd, dead_socket_tx)),
        )?;
        Ok(Self {
            cb,
            background: handle.clone(),
        })
    }

    pub fn receive(&mut self, header: &mut TcpHeader, data: DemiBuffer) {
        self.cb.receive(header, data)
    }

    pub fn send(&self, buf: DemiBuffer) -> Result<(), Fail> {
        self.cb.send(buf)
    }

    pub fn poll_recv(&mut self, ctx: &mut Context, size: Option<usize>) -> Poll<Result<DemiBuffer, Fail>> {
        self.cb.poll_recv(ctx, size)
    }

    pub fn close(&mut self) -> Result<(), Fail> {
        self.cb.close()
    }

    pub fn poll_close(&self) -> Poll<Result<(), Fail>> {
        self.cb.poll_close()
    }

    pub fn remote_mss(&self) -> usize {
        self.cb.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.cb.rto()
    }

    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.cb.get_local(), self.cb.get_remote())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Drop for EstablishedSocket<N> {
    fn drop(&mut self) {
        self.background.deschedule();
    }
}
