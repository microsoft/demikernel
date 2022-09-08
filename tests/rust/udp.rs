// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(new_uninit)]

mod common;

//==============================================================================
// Imports
//==============================================================================

use ::demikernel::inetstack::{
    operations::OperationResult,
    runtime::{
        memory::{
            Buffer,
            DataBuffer,
        },
        QDesc,
        QToken,
    },
    InetStack,
};
use common::{
    arp,
    libos::*,
    ALICE_IPV4,
    ALICE_MAC,
    BOB_IPV4,
    BOB_MAC,
    PORT_BASE,
};
use crossbeam_channel::{
    self,
    Receiver,
    Sender,
};
use std::{
    net::SocketAddrV4,
    thread::{
        self,
        JoinHandle,
    },
};

//==============================================================================
// Connect
//==============================================================================

/// Opens and closes a socket using a non-ephemeral port.
fn do_udp_setup(libos: &mut InetStack) {
    let local: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, PORT_BASE);
    let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
    libos.bind(sockfd, local).unwrap();
    libos.close(sockfd).unwrap();
}

/// Opens and closes a socket using an ephemeral port.
fn do_udp_setup_ephemeral(libos: &mut InetStack) {
    const PORT_EPHEMERAL_BASE: u16 = 49152;
    let local: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, PORT_EPHEMERAL_BASE);
    let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
    libos.bind(sockfd, local).unwrap();
    libos.close(sockfd).unwrap();
}

/// Opens and closes a socket using wildcard ephemeral port.
fn do_udp_setup_wildcard_ephemeral(libos: &mut InetStack) {
    let local: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, 0);
    let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
    libos.bind(sockfd, local).unwrap();
    libos.close(sockfd).unwrap();
}

/// Tests if a socket can be successfully setup.
#[test]
fn udp_setup() {
    let (tx, rx): (Sender<DataBuffer>, Receiver<DataBuffer>) = crossbeam_channel::unbounded();
    let mut libos: InetStack = DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp());
    do_udp_setup(&mut libos);
    do_udp_setup_ephemeral(&mut libos);
    do_udp_setup_wildcard_ephemeral(&mut libos);
}

/// Tests if a connection can be successfully established in loopback mode.
#[test]
fn udp_connect_loopback() {
    let (tx, rx): (Sender<DataBuffer>, Receiver<DataBuffer>) = crossbeam_channel::unbounded();
    let mut libos: InetStack = DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp());

    let port: u16 = PORT_BASE;
    let local: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, port);

    // Open and close a connection.
    let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
    libos.bind(sockfd, local).unwrap();
    libos.close(sockfd).unwrap();
}

//==============================================================================
// Push
//==============================================================================

/// Tests if data can be successfully pushed/popped form a local endpoint to
/// itself.
#[test]
fn udp_push_remote() {
    let (alice_tx, alice_rx): (Sender<DataBuffer>, Receiver<DataBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DataBuffer>, Receiver<DataBuffer>) = crossbeam_channel::unbounded();

    let bob_port: u16 = PORT_BASE;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(BOB_IPV4, bob_port);
    let alice_port: u16 = PORT_BASE;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, alice_port);

    let alice: JoinHandle<()> = thread::spawn(move || {
        let mut libos: InetStack = DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp());

        // Open connection.
        let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
        libos.bind(sockfd, alice_addr).unwrap();

        // Cook some data.
        let bytes: Buffer = DummyLibOS::cook_data(32);

        // Push data.
        let qt: QToken = libos.pushto2(sockfd, &bytes, bob_addr).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        match qr {
            OperationResult::Push => (),
            _ => panic!("push() failed"),
        }

        // Pop data.
        let qt: QToken = libos.pop(sockfd).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => panic!("pop() failed"),
        }

        // Close connection.
        libos.close(sockfd).unwrap();
    });

    let bob: JoinHandle<()> = thread::spawn(move || {
        let mut libos: InetStack = DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp());

        // Open connection.
        let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
        libos.bind(sockfd, bob_addr).unwrap();

        // Pop data.
        let qt: QToken = libos.pop(sockfd).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        let bytes: Buffer = match qr {
            OperationResult::Pop(_, bytes) => bytes,
            _ => panic!("pop() failed"),
        };

        // Push data.
        let qt: QToken = libos.pushto2(sockfd, &bytes, alice_addr).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        match qr {
            OperationResult::Push => (),
            _ => panic!("push() failed"),
        }

        // Close connection.
        libos.close(sockfd).unwrap();
    });

    alice.join().unwrap();
    bob.join().unwrap();
}

/// Tests if data can be successfully pushed/popped in loopback mode.
#[test]
fn udp_loopback() {
    let (alice_tx, alice_rx): (Sender<DataBuffer>, Receiver<DataBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DataBuffer>, Receiver<DataBuffer>) = crossbeam_channel::unbounded();

    let bob_port: u16 = PORT_BASE;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, bob_port);
    let alice_port: u16 = PORT_BASE;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(ALICE_IPV4, alice_port);

    let alice: JoinHandle<()> = thread::spawn(move || {
        let mut libos: InetStack = DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp());

        // Open connection.
        let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
        libos.bind(sockfd, alice_addr).unwrap();

        // Cook some data.
        let bytes: Buffer = DummyLibOS::cook_data(32);

        // Push data.
        let qt: QToken = libos.pushto2(sockfd, &bytes, bob_addr).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        match qr {
            OperationResult::Push => (),
            _ => panic!("push() failed"),
        }

        // Pop data.
        let qt: QToken = libos.pop(sockfd).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => panic!("pop() failed"),
        }

        // Close connection.
        libos.close(sockfd).unwrap();
    });

    let bob = thread::spawn(move || {
        let mut libos: InetStack = DummyLibOS::new(ALICE_MAC, ALICE_IPV4, bob_tx, alice_rx, arp());

        // Open connection.
        let sockfd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
        libos.bind(sockfd, bob_addr).unwrap();

        // Pop data.
        let qt: QToken = libos.pop(sockfd).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        let bytes: Buffer = match qr {
            OperationResult::Pop(_, bytes) => bytes,
            _ => panic!("pop() failed"),
        };

        // Push data.
        let qt: QToken = libos.pushto2(sockfd, &bytes, alice_addr).unwrap();
        let (_, qr): (QDesc, OperationResult) = match libos.wait2(qt) {
            Ok((qd, qr)) => (qd, qr),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        match qr {
            OperationResult::Push => (),
            _ => panic!("push() failed"),
        }

        // Close connection.
        libos.close(sockfd).unwrap();
    });

    alice.join().unwrap();
    bob.join().unwrap();
}
