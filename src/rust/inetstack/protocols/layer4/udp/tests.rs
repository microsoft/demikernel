// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::{
        consts::MAX_HEADER_SIZE,
        test_helpers::{self, engine::SharedEngine},
    },
    runtime::{
        memory::DemiBuffer,
        queue::{OperationResult, QDesc, QToken},
    },
};
use ::anyhow::Result;
use ::libc::EBADF;
use ::std::{
    net::{Ipv4Addr, SocketAddrV4},
    time::{Duration, Instant},
};

//======================================================================================================================
// Bind & Close
//======================================================================================================================

#[test]
fn udp_bind_udp_close() -> Result<()> {
    let mut now = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = match bob.udp_socket() {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("could not create socket: {:?}", e),
    };
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port = 80;
    let carrie_addr = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = match carrie.udp_socket() {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("could not create socket: {:?}", e),
    };
    carrie.udp_bind(carrie_fd, carrie_addr)?;

    now += Duration::from_micros(1);

    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}

//======================================================================================================================
// Push & Pop
//======================================================================================================================

#[test]
fn udp_push_pop() -> Result<()> {
    let mut now: Instant = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port: u16 = 80;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port: u16 = 80;
    let carrie_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = carrie.udp_socket()?;
    carrie.udp_bind(carrie_fd, carrie_addr)?;

    // Send data to Carrie.
    let buf: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
        .expect("slice should fit in DemiBuffer");
    let bob_qt: QToken = bob.udp_pushto(bob_fd, buf.clone(), carrie_addr)?;
    match bob.wait(bob_qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    now += Duration::from_micros(1);

    // Take a packet from Bob and deliver to Carrie.
    carrie.push_frame(bob.pop_frame());
    let carrie_qt: QToken = carrie.udp_pop(carrie_fd)?;

    let (remote_addr, received_buf): (Option<SocketAddrV4>, DemiBuffer) = match carrie.wait(carrie_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), bob_addr);
    assert_eq!(received_buf[..], buf[..]);

    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}

//======================================================================================================================
// Push & Pop
//======================================================================================================================

#[test]
fn udp_push_pop_wildcard_address() -> Result<()> {
    let mut now: Instant = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port: u16 = 80;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port: u16 = 80;
    let carrie_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = carrie.udp_socket()?;
    carrie.udp_bind(carrie_fd, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, carrie_port))?;

    // Send data to Carrie.
    let buf: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
        .expect("slice should fit in DemiBuffer");
    let qt: QToken = bob.udp_pushto(bob_fd, buf.clone(), carrie_addr)?;
    match bob.wait(qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };

    now += Duration::from_micros(1);

    // Take a packet from Bob and deliver to Carrie.
    carrie.push_frame(bob.pop_frame());
    let carrie_qt: QToken = carrie.udp_pop(carrie_fd)?;
    let (remote_addr, received_buf): (Option<SocketAddrV4>, DemiBuffer) = match carrie.wait(carrie_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), bob_addr);
    assert_eq!(received_buf[..], buf[..]);
    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}

//======================================================================================================================
// Ping Pong
//======================================================================================================================

#[test]
fn udp_ping_pong() -> Result<()> {
    let mut now = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port = 80;
    let carrie_addr = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = carrie.udp_socket()?;
    carrie.udp_bind(carrie_fd, carrie_addr)?;

    // Send data to Carrie.
    let buf_a: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
        .expect("slice should fit in DemiBuffer");
    let bob_qt: QToken = bob.udp_pushto(bob_fd, buf_a.clone(), carrie_addr)?;
    match bob.wait(bob_qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    now += Duration::from_micros(1);

    // Take a packet from Bob and deliver to Carrie.
    carrie.push_frame(bob.pop_frame());
    let carrie_qt: QToken = carrie.udp_pop(carrie_fd)?;

    let (remote_addr, received_buf_a): (Option<SocketAddrV4>, DemiBuffer) = match carrie.wait(carrie_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), bob_addr);
    assert_eq!(received_buf_a[..], buf_a[..]);

    now += Duration::from_micros(1);

    // Send data to Bob.
    let buf_b: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
        .expect("slice should fit in DemiBuffer");
    let carrie_qt2: QToken = carrie.udp_pushto(carrie_fd, buf_b.clone(), bob_addr)?;
    match carrie.wait(carrie_qt2)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    now += Duration::from_micros(1);

    // Take a packet from Carrie and deliver to Bob.
    bob.push_frame(carrie.pop_frame());
    let bob_qt: QToken = bob.udp_pop(bob_fd)?;
    let (remote_addr, received_buf_b): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), carrie_addr);
    assert_eq!(received_buf_b[..], buf_b[..]);

    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}

//======================================================================================================================
// Loop Bind & Close
//======================================================================================================================

#[test]
fn udp_loop1_bind_udp_close() -> Result<()> {
    // Loop.
    for _ in 0..1000 {
        udp_bind_udp_close()?;
    }

    Ok(())
}

#[test]
fn udp_loop2_bind_udp_close() -> Result<()> {
    let mut now = Instant::now();

    // Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);

    // Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port = 80;
    let carrie_addr = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);

    // Loop.
    for _ in 0..1000 {
        // Bind Bob.
        let bob_fd: QDesc = bob.udp_socket()?;
        bob.udp_bind(bob_fd, bob_addr)?;

        // Bind carrie.
        let carrie_fd: QDesc = carrie.udp_socket()?;
        carrie.udp_bind(carrie_fd, carrie_addr)?;

        now += Duration::from_micros(1);

        // Close peers.
        bob.udp_close(bob_fd)?;
        carrie.udp_close(carrie_fd)?;
    }

    Ok(())
}

//======================================================================================================================
// Loop Push & Pop
//======================================================================================================================

#[test]
fn udp_loop1_push_pop() -> Result<()> {
    // Loop.
    for _ in 0..1000 {
        udp_push_pop()?;
    }

    Ok(())
}

#[test]
fn udp_loop2_push_pop() -> Result<()> {
    let mut now = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port = 80;
    let carrie_addr = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = carrie.udp_socket()?;
    carrie.udp_bind(carrie_fd, carrie_addr)?;
    // Loop.
    for b in 0..1000 {
        // Send data to Carrie.
        let buf: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![(b % 256) as u8; 32][..], MAX_HEADER_SIZE)
            .expect("slice should fit");
        let bob_qt: QToken = bob.udp_pushto(bob_fd, buf.clone(), carrie_addr)?;
        match bob.wait(bob_qt)? {
            (_, OperationResult::Push) => {},
            _ => anyhow::bail!("Push failed"),
        };

        now += Duration::from_micros(1);

        // Take a packet from Bob and deliver to Carrie.
        carrie.push_frame(bob.pop_frame());
        let carrie_qt: QToken = carrie.udp_pop(carrie_fd)?;
        let (remote_addr, received_buf): (Option<SocketAddrV4>, DemiBuffer) = match carrie.wait(carrie_qt)? {
            (_, OperationResult::Pop(addr, buf)) => (addr, buf),
            _ => anyhow::bail!("Pop failed"),
        };
        assert_eq!(remote_addr.unwrap(), bob_addr);
        assert_eq!(received_buf[..], buf[..]);
    }

    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}

//======================================================================================================================
// Loop Ping Pong
//======================================================================================================================

#[test]
fn udp_loop1_ping_pong() -> Result<()> {
    // Loop.
    for _ in 0..1000 {
        udp_ping_pong()?;
    }

    Ok(())
}

#[test]
fn udp_loop2_ping_pong() -> Result<()> {
    let mut now = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port = 80;
    let carrie_addr = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = carrie.udp_socket()?;
    carrie.udp_bind(carrie_fd, carrie_addr)?;
    //
    // Loop.
    for _ in 0..1000 {
        // Send data to Carrie.
        let buf_a: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
            .expect("slice should fit in DemiBuffer");
        let bob_qt: QToken = bob.udp_pushto(bob_fd, buf_a.clone(), carrie_addr)?;
        match bob.wait(bob_qt)? {
            (_, OperationResult::Push) => {},
            _ => anyhow::bail!("Push failed"),
        };

        now += Duration::from_micros(1);

        // Take a packet from Bob and deliver to Carrie.
        carrie.push_frame(bob.pop_frame());
        let carrie_qt: QToken = carrie.udp_pop(carrie_fd)?;
        let (remote_addr, received_buf_a): (Option<SocketAddrV4>, DemiBuffer) = match carrie.wait(carrie_qt)? {
            (_, OperationResult::Pop(addr, buf)) => (addr, buf),
            _ => anyhow::bail!("Pop failed"),
        };
        assert_eq!(remote_addr.unwrap(), bob_addr);
        assert_eq!(received_buf_a[..], buf_a[..]);

        now += Duration::from_micros(1);

        // Send data to Bob.
        let buf_b: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
            .expect("slice should fit in DemiBuffer");
        let carrie_qt: QToken = carrie.udp_pushto(carrie_fd, buf_b.clone(), bob_addr)?;
        match carrie.wait(carrie_qt)? {
            (_, OperationResult::Push) => {},
            _ => anyhow::bail!("Push failed"),
        };

        now += Duration::from_micros(1);

        // Take a packet from Carrie and deliver to Bob.
        bob.push_frame(carrie.pop_frame());
        let bob_qt: QToken = bob.udp_pop(bob_fd)?;
        let (remote_addr, received_buf_b): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
            (_, OperationResult::Pop(addr, buf)) => (addr, buf),
            _ => anyhow::bail!("Pop failed"),
        };
        assert_eq!(remote_addr.unwrap(), carrie_addr);
        assert_eq!(received_buf_b[..], buf_b[..]);
    }

    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}

//======================================================================================================================
// Bad Pop
//======================================================================================================================

#[test]
fn udp_pop_not_bound() -> Result<()> {
    let mut now = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port = 80;
    let carrie_addr = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    // Carrie does not create a socket.

    // Send data to Carrie.
    let buf: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
        .expect("slice should fit in DemiBuffer");
    let bob_qt: QToken = bob.udp_pushto(bob_fd, buf, carrie_addr)?;
    match bob.wait(bob_qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };

    now += Duration::from_micros(1);

    // Take a packet from Bob and deliver to Carrie.
    // TODO: check that Carrie drops this packet.
    // FIXME: https://github.com/microsoft/demikernel/issues/1065
    carrie.push_frame(bob.pop_frame());
    // Close peers.
    bob.udp_close(bob_fd)?;
    // Carrie does not have a socket.

    Ok(())
}

//======================================================================================================================
// Bad Push
//======================================================================================================================

#[test]
fn udp_push_bad_file_descriptor() -> Result<()> {
    let mut now = Instant::now();

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let bob_port: u16 = 80;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Setup Carrie.
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let carrie_port: u16 = 80;
    let carrie_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::CARRIE_IPV4, carrie_port);
    let carrie_fd: QDesc = carrie.udp_socket()?;
    carrie.udp_bind(carrie_fd, carrie_addr)?;

    // Send data to Carrie.
    let buf: DemiBuffer = DemiBuffer::from_slice_with_headroom(&vec![0x5a; 32][..], MAX_HEADER_SIZE)
        .expect("slice should fit in DemiBuffer");
    match bob.udp_pushto(QDesc::try_from(u32::MAX)?, buf.clone(), carrie_addr) {
        Err(e) if e.errno == EBADF => {},
        _ => anyhow::bail!("pushto should have failed"),
    };

    now += Duration::from_micros(1);

    // Close peers.
    bob.udp_close(bob_fd)?;
    carrie.udp_close(carrie_fd)?;

    Ok(())
}
