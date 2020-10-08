// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::file_table::FileDescriptor;
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet::{
            self,
        },
        ipv4,
        tcp::operations::{
            AcceptFuture,
            ConnectFuture,
            PopFuture,
            PushFuture,
        },
    },
    runtime::Runtime,
};
use bytes::Bytes;
use std::{
    future::Future,
    net::Ipv4Addr,
    time::{
        Duration,
    },
};
use crate::file_table::FileTable;

#[cfg(test)]
use crate::protocols::{ethernet::MacAddress};
#[cfg(test)]
use hashbrown::HashMap;

pub struct Engine<RT: Runtime> {
    rt: RT,
    arp: arp::Peer<RT>,
    ipv4: ipv4::Peer<RT>,

    file_table: FileTable,
}

impl<RT: Runtime> Engine<RT> {
    pub fn new(rt: RT) -> Result<Self, Fail> {
        let now = rt.now();
        let file_table = FileTable::new();
        let arp = arp::Peer::new(now, rt.clone())?;
        let ipv4 = ipv4::Peer::new(rt.clone(), arp.clone(), file_table.clone());
        Ok(Engine { rt, arp, ipv4, file_table })
    }

    pub fn rt(&self) -> &RT {
        &self.rt
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), Fail> {
        let frame = ethernet::Frame::attach(&bytes)?;
        let header = frame.header();
        if self.rt.local_link_addr() != header.dest_addr() && !header.dest_addr().is_broadcast() {
            println!(
                "Misdelivered {:?} {:?} {}",
                self.rt.local_link_addr(),
                header.dest_addr(),
                header.dest_addr().is_broadcast()
            );
            return Err(Fail::Misdelivered {});
        }

        match header.ether_type()? {
            ethernet::EtherType::Arp => self.arp.receive(frame),
            ethernet::EtherType::Ipv4 => self.ipv4.receive(frame),
        }
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        self.ipv4.ping(dest_ipv4_addr, timeout)
    }

    pub fn tcp_socket(&mut self) -> FileDescriptor {
        self.ipv4.tcp.socket()
    }

    pub fn tcp_connect(
        &mut self,
        socket_fd: FileDescriptor,
        remote_endpoint: ipv4::Endpoint,
    ) -> ConnectFuture<RT> {
        self.ipv4.tcp.connect(socket_fd, remote_endpoint)
    }

    pub fn tcp_bind(
        &mut self,
        socket_fd: FileDescriptor,
        endpoint: ipv4::Endpoint,
    ) -> Result<(), Fail> {
        self.ipv4.tcp.bind(socket_fd, endpoint)
    }

    pub fn tcp_accept(&mut self, handle: FileDescriptor) -> AcceptFuture<RT> {
        self.ipv4.tcp.accept(handle)
    }

    pub fn tcp_push(&mut self, socket_fd: FileDescriptor, buf: Bytes) -> PushFuture<RT> {
        self.ipv4.tcp.push(socket_fd, buf)
    }

    pub fn tcp_pop(&mut self, socket_fd: FileDescriptor) -> PopFuture<RT> {
        self.ipv4.tcp.pop(socket_fd)
    }

    pub fn tcp_close(&mut self, socket_fd: FileDescriptor) -> Result<(), Fail> {
        self.ipv4.tcp.close(socket_fd)
    }

    pub fn tcp_listen(&mut self, socket_fd: FileDescriptor, backlog: usize) -> Result<(), Fail> {
        self.ipv4.tcp.listen(socket_fd, backlog)
    }

    #[cfg(test)]
    pub fn arp_query(&self, ipv4_addr: Ipv4Addr) -> impl Future<Output = Result<MacAddress, Fail>> {
        self.arp.query(ipv4_addr)
    }

    #[cfg(test)]
    pub fn tcp_mss(&self, handle: FileDescriptor) -> Result<usize, Fail> {
        self.ipv4.tcp_mss(handle)
    }

    #[cfg(test)]
    pub fn tcp_rto(&self, handle: FileDescriptor) -> Result<Duration, Fail> {
        self.ipv4.tcp_rto(handle)
    }

    #[cfg(test)]
    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }

    #[cfg(test)]
    pub fn import_arp_cache(&self, cache: HashMap<Ipv4Addr, MacAddress>) {
        self.arp.import_cache(cache)
    }
}
