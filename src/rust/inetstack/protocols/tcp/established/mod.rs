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
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::futures::channel::mpsc;
use ::std::{
    net::SocketAddrV4,
    time::Duration,
};

#[derive(Clone)]
pub struct EstablishedSocket<const N: usize> {
    pub cb: SharedControlBlock<N>,
    // We need this to eventually stop the background task on close.
    #[allow(unused)]
    runtime: SharedDemiRuntime,
    /// The background co-routines handles various tasks, such as retransmission and acknowledging.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: TaskHandle,
}

impl<const N: usize> EstablishedSocket<N> {
    pub fn new(
        cb: SharedControlBlock<N>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        mut runtime: SharedDemiRuntime,
    ) -> Result<Self, Fail> {
        // TODO: Maybe add the queue descriptor here.
        let handle: TaskHandle = runtime.insert_background_coroutine(
            "Inetstack::TCP::established::background",
            Box::pin(background::background(cb.clone(), dead_socket_tx)),
        )?;
        Ok(Self {
            cb,
            background: handle.clone(),
            runtime: runtime.clone(),
        })
    }

    pub fn receive(&mut self, header: &mut TcpHeader, data: DemiBuffer) {
        self.cb.receive(header, data)
    }

    pub fn send(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        self.cb.send(buf)
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        self.cb.pop(size, yielder).await
    }

    pub fn close(&mut self) -> Result<(), Fail> {
        self.cb.close()
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

// TODO: Uncomment this once we have proper resource clean up on asynchronous close.
// FIXME: https://github.com/microsoft/demikernel/issues/988
// impl<const N: usize> Drop for EstablishedSocket<N> {
//     fn drop(&mut self) {
//         if let Err(e) = self.runtime.remove_background_coroutine(&self.background) {
//             panic!("Failed to drop established socket (error={})", e);
//         }
//     }
// }
