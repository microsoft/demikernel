// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod background;
pub mod congestion_control;
mod ctrlblk;
mod rto;
mod sender;

pub use self::{
    sender::{
        Sender,
        UnackedSegment,
    },
};

use crate::{
    collections::async_queue::SharedAsyncQueue,
    inetstack::{
        protocols::{
            ipv4::Ipv4Header,
            tcp::{
                congestion_control::CongestionControlConstructor,
                established::ctrlblk::SharedControlBlock,
                segment::TcpHeader,
                SeqNumber,
            },
        },
        MacAddress,
        SharedArpPeer,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            config::TcpConfig,
            socket::option::TcpSocketOptions,
            NetworkRuntime,
        },
        QDesc,
        SharedDemiRuntime,
    },
    QToken,
};
use ::futures::{
    channel::mpsc,
    FutureExt,
};
use ::std::{
    net::SocketAddrV4,
    time::Duration,
};

#[cfg(feature = "tcp-migration")]
pub use ctrlblk::state::ControlBlockState;

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcp::peer::state::TcpState;


use crate::{capy_log, capy_log_mig};
// #[cfg(all(feature = "tcp-migration", test))]
// pub use ctrlblk::state::test::get_state as test_get_control_block_state;


#[derive(Clone)]
pub struct EstablishedSocket<N: NetworkRuntime> {
    pub cb: SharedControlBlock<N>,
    recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
    // We need this to eventually stop the background task on close.
    #[allow(unused)]
    runtime: SharedDemiRuntime,
    /// The background co-routines handles various tasks, such as retransmission and acknowledging.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background_task_qt: QToken,
}

impl<N: NetworkRuntime> EstablishedSocket<N> {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        mut runtime: SharedDemiRuntime,
        transport: N,
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        ack_queue: SharedAsyncQueue<usize>,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
        arp: SharedArpPeer<N>,
        receiver_seq_no: SeqNumber,
        ack_delay_timeout: Duration,
        receiver_window_size: u32,
        receiver_window_scale: u32,
        sender_seq_no: SeqNumber,
        sender_window_size: u32,
        sender_window_scale: u8,
        sender_mss: usize,
        cc_constructor: CongestionControlConstructor,
        congestion_control_options: Option<congestion_control::Options>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        socket_queue: Option<SharedAsyncQueue<SocketAddrV4>>,
    ) -> Result<Self, Fail> {
        capy_log!("Creating new EstablishedSocket with {} (CONN recv_queue len: {})", remote, recv_queue.len());
        // TODO: Maybe add the queue descriptor here.
        let cb = SharedControlBlock::new(
            local,
            remote,
            runtime.clone(),
            transport,
            local_link_addr,
            tcp_config,
            default_socket_options,
            arp,
            receiver_seq_no,
            ack_delay_timeout,
            receiver_window_size,
            receiver_window_scale,
            sender_seq_no,
            sender_window_size,
            sender_window_scale,
            sender_mss,
            cc_constructor,
            congestion_control_options,
            recv_queue.clone(),
            ack_queue.clone(),
            socket_queue,
        );
        let qt: QToken = runtime.insert_background_coroutine(
            "bgc::inetstack::tcp::established::background",
            Box::pin(background::background(cb.clone(), dead_socket_tx).fuse()),
        )?;
        Ok(Self {
            cb,
            recv_queue,
            background_task_qt: qt.clone(),
            runtime: runtime.clone(),
        })
    }

    pub fn get_recv_queue(&self) -> SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)> {
        self.recv_queue.clone()
    }

    pub fn send(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        self.cb.send(buf)
    }

    pub async fn push(&mut self, nbytes: usize) -> Result<(), Fail> {
        self.cb.push(nbytes).await
    }

    pub async fn pop(&mut self, size: Option<usize>) -> Result<DemiBuffer, Fail> {
        self.cb.pop(size).await
    }

    pub async fn close(&mut self) -> Result<(), Fail> {
        self.cb.close().await
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

    #[cfg(feature = "tcp-migration")]
    pub fn from_state(
        mut runtime: SharedDemiRuntime,
        transport: N,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
        arp: SharedArpPeer<N>,
        ack_delay_timeout: Duration,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        socket_queue: Option<SharedAsyncQueue<SocketAddrV4>>,
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        state: TcpState
    ) -> Result<Self, Fail> {
        let cb = SharedControlBlock::<N>::from_state(
            runtime.clone(),
            transport,
            local_link_addr,
            tcp_config,
            default_socket_options,
            arp,
            ack_delay_timeout,
            socket_queue,
            recv_queue.clone(),
            state.cb,
        );
        let qt: QToken = runtime.insert_background_coroutine(
            "bgc::inetstack::tcp::established::background",
            Box::pin(background::background(cb.clone(), dead_socket_tx).fuse()),
        )?;
        Ok(Self {
            cb,
            recv_queue,
            background_task_qt: qt.clone(),
            runtime: runtime.clone(),
        })
    }
}
