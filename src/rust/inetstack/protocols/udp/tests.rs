// // Copyright (c) Microsoft Corporation.
// // Licensed under the MIT license.

use crate::{
    inetstack::test_helpers::{
        self,
        engine::SharedEngine,
    },
    runtime::{
        memory::DemiBuffer,
        queue::{
            OperationResult,
            QDesc,
            QToken,
        },
    },
};
use ::anyhow::Result;
use ::libc::{
    EADDRINUSE,
    EBADF,
};
use ::std::{
    convert::TryFrom,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Bind & Close
//==============================================================================

#[test]
fn udp_bind_udp_close() -> Result<()> {
    let mut now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = match alice.udp_socket() {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("could not create socket: {:?}", e),
    };
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = match bob.udp_socket() {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("could not create socket: {:?}", e),
    };
    bob.udp_bind(bob_fd, bob_addr)?;

    now += Duration::from_micros(1);

    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}

//==============================================================================
// Push & Pop
//==============================================================================

#[test]
fn udp_push_pop() -> Result<()> {
    let mut now: Instant = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port: u16 = 80;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port: u16 = 80;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Send data to Bob.
    let buf: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
    let alice_qt: QToken = alice.udp_pushto(alice_fd, buf.clone(), bob_addr)?;
    match alice.wait(alice_qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    now += Duration::from_micros(1);

    // Receive data from Alice.
    bob.receive(alice.pop_frame()).unwrap();
    let bob_qt: QToken = bob.udp_pop(bob_fd)?;

    let (remote_addr, received_buf): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), alice_addr);
    assert_eq!(received_buf[..], buf[..]);

    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}

//==============================================================================
// Push & Pop
//==============================================================================

#[test]
fn udp_push_pop_wildcard_address() -> Result<()> {
    let mut now: Instant = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port: u16 = 80;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port: u16 = 80;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, bob_port))?;

    // Send data to Bob.
    let buf: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
    let qt: QToken = alice.udp_pushto(alice_fd, buf.clone(), bob_addr)?;
    match alice.wait(qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };

    now += Duration::from_micros(1);

    // Receive data from Alice.
    bob.receive(alice.pop_frame()).unwrap();
    let bob_qt: QToken = bob.udp_pop(bob_fd)?;
    let (remote_addr, received_buf): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), alice_addr);
    assert_eq!(received_buf[..], buf[..]);
    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}

//==============================================================================
// Ping Pong
//==============================================================================

#[test]
fn udp_ping_pong() -> Result<()> {
    let mut now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Send data to Bob.
    let buf_a: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
    let alice_qt: QToken = alice.udp_pushto(alice_fd, buf_a.clone(), bob_addr)?;
    match alice.wait(alice_qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    now += Duration::from_micros(1);

    // Receive data from Alice.
    bob.receive(alice.pop_frame()).unwrap();
    let bob_qt: QToken = bob.udp_pop(bob_fd)?;
    bob.poll();

    let (remote_addr, received_buf_a): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), alice_addr);
    assert_eq!(received_buf_a[..], buf_a[..]);

    now += Duration::from_micros(1);

    // Send data to Alice.
    let buf_b: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
    let bob_qt2: QToken = bob.udp_pushto(bob_fd, buf_b.clone(), alice_addr)?;
    match bob.wait(bob_qt2)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    bob.poll();
    now += Duration::from_micros(1);

    // Receive data from Bob.
    alice.receive(bob.pop_frame()).unwrap();
    let alice_qt: QToken = alice.udp_pop(alice_fd)?;
    let (remote_addr, received_buf_b): (Option<SocketAddrV4>, DemiBuffer) = match alice.wait(alice_qt)? {
        (_, OperationResult::Pop(addr, buf)) => (addr, buf),
        _ => anyhow::bail!("Pop failed"),
    };
    assert_eq!(remote_addr.unwrap(), bob_addr);
    assert_eq!(received_buf_b[..], buf_b[..]);

    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}

//==============================================================================
// Loop Bind & Close
//==============================================================================

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

    // Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);

    // Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);

    // Loop.
    for _ in 0..1000 {
        // Bind Alice.
        let alice_fd: QDesc = alice.udp_socket()?;
        alice.udp_bind(alice_fd, alice_addr)?;

        // Bind bob.
        let bob_fd: QDesc = bob.udp_socket()?;
        bob.udp_bind(bob_fd, bob_addr)?;

        now += Duration::from_micros(1);

        // Close peers.
        alice.udp_close(alice_fd)?;
        bob.udp_close(bob_fd)?;
    }

    Ok(())
}

//==============================================================================
// Loop Push & Pop
//==============================================================================

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

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;
    // Loop.
    for b in 0..1000 {
        // Send data to Bob.
        let buf: DemiBuffer = DemiBuffer::from_slice(&vec![(b % 256) as u8; 32][..]).expect("slice should fit");
        let alice_qt: QToken = alice.udp_pushto(alice_fd, buf.clone(), bob_addr)?;
        match alice.wait(alice_qt)? {
            (_, OperationResult::Push) => {},
            _ => anyhow::bail!("Push failed"),
        };
        alice.poll();

        now += Duration::from_micros(1);

        // Receive data from Alice.
        bob.receive(alice.pop_frame()).unwrap();
        let bob_qt: QToken = bob.udp_pop(bob_fd)?;
        let (remote_addr, received_buf): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
            (_, OperationResult::Pop(addr, buf)) => (addr, buf),
            _ => anyhow::bail!("Pop failed"),
        };
        assert_eq!(remote_addr.unwrap(), alice_addr);
        assert_eq!(received_buf[..], buf[..]);
    }

    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}

//==============================================================================
// Loop Ping Pong
//==============================================================================

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

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;
    //
    // Loop.
    for _ in 0..1000 {
        // Send data to Bob.
        let buf_a: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
        let alice_qt: QToken = alice.udp_pushto(alice_fd, buf_a.clone(), bob_addr)?;
        match alice.wait(alice_qt)? {
            (_, OperationResult::Push) => {},
            _ => anyhow::bail!("Push failed"),
        };
        alice.poll();

        now += Duration::from_micros(1);

        // Receive data from Alice.
        bob.receive(alice.pop_frame()).unwrap();
        let bob_qt: QToken = bob.udp_pop(bob_fd)?;
        let (remote_addr, received_buf_a): (Option<SocketAddrV4>, DemiBuffer) = match bob.wait(bob_qt)? {
            (_, OperationResult::Pop(addr, buf)) => (addr, buf),
            _ => anyhow::bail!("Pop failed"),
        };
        assert_eq!(remote_addr.unwrap(), alice_addr);
        assert_eq!(received_buf_a[..], buf_a[..]);

        now += Duration::from_micros(1);

        // Send data to Alice.
        let buf_b: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
        let bob_qt: QToken = bob.udp_pushto(bob_fd, buf_b.clone(), alice_addr)?;
        match bob.wait(bob_qt)? {
            (_, OperationResult::Push) => {},
            _ => anyhow::bail!("Push failed"),
        };

        now += Duration::from_micros(1);

        // Receive data from Bob.
        alice.receive(bob.pop_frame()).unwrap();
        let alice_qt: QToken = alice.udp_pop(alice_fd)?;
        let (remote_addr, received_buf_b): (Option<SocketAddrV4>, DemiBuffer) = match alice.wait(alice_qt)? {
            (_, OperationResult::Pop(addr, buf)) => (addr, buf),
            _ => anyhow::bail!("Pop failed"),
        };
        assert_eq!(remote_addr.unwrap(), bob_addr);
        assert_eq!(received_buf_b[..], buf_b[..]);
    }

    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}

//==============================================================================
// Bad Bind
//==============================================================================

#[test]
fn udp_bind_address_in_use() -> Result<()> {
    let now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Try to bind Alice again.
    match alice.udp_bind(alice_fd, alice_addr) {
        Err(e) if e.errno == EADDRINUSE => {},
        _ => anyhow::bail!("bind should have failed"),
    };

    // Close peers.
    alice.udp_close(alice_fd)?;

    Ok(())
}

#[test]
fn udp_bind_bad_file_descriptor() -> Result<()> {
    let now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port: u16 = 80;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = QDesc::try_from(u32::MAX)?;

    // Try to bind Alice.
    match alice.udp_bind(alice_fd, alice_addr) {
        Err(e) if e.errno == libc::EBADF => {},
        _ => anyhow::bail!("bind should have failed"),
    };

    Ok(())
}

//==============================================================================
// Bad Close
//==============================================================================

#[test]
fn udp_udp_close_bad_file_descriptor() -> Result<()> {
    let now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_fd: QDesc = alice.udp_socket()?;
    let alice_port: u16 = 80;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    alice.udp_bind(alice_fd, alice_addr)?;

    // Try to udp_close bad file descriptor.
    match alice.udp_close(QDesc::try_from(u32::MAX)?) {
        Err(e) if e.errno == EBADF => {},
        _ => anyhow::bail!("close should have failed"),
    };

    // Try to udp_close Alice two times.
    alice.udp_close(alice_fd)?;
    match alice.udp_close(alice_fd) {
        Err(e) if e.errno == EBADF => {},
        _ => anyhow::bail!("close should have failed"),
    };

    Ok(())
}

//==============================================================================
// Bad Pop
//==============================================================================

#[test]
fn udp_pop_not_bound() -> Result<()> {
    let mut now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port = 80;
    let alice_addr = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port = 80;
    let bob_addr = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    // Bob does not create a socket.

    // Send data to Bob.
    let buf: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
    let alice_qt: QToken = alice.udp_pushto(alice_fd, buf, bob_addr)?;
    match alice.wait(alice_qt)? {
        (_, OperationResult::Push) => {},
        _ => anyhow::bail!("Push failed"),
    };
    alice.poll();

    now += Duration::from_micros(1);

    // Receive data from Alice.
    // TODO: check that Bob drops this packet.
    // FIXME: https://github.com/microsoft/demikernel/issues/1065
    bob.receive(alice.pop_frame())?;
    // Close peers.
    alice.udp_close(alice_fd)?;
    // Bob does not have a socket.

    Ok(())
}

//==============================================================================
// Bad Push
//==============================================================================

#[test]
fn udp_push_bad_file_descriptor() -> Result<()> {
    let mut now = Instant::now();

    // Setup Alice.
    let mut alice: SharedEngine = test_helpers::new_alice2(now);
    let alice_port: u16 = 80;
    let alice_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::ALICE_IPV4, alice_port);
    let alice_fd: QDesc = alice.udp_socket()?;
    alice.udp_bind(alice_fd, alice_addr)?;

    // Setup Bob.
    let mut bob: SharedEngine = test_helpers::new_bob2(now);
    let bob_port: u16 = 80;
    let bob_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, bob_port);
    let bob_fd: QDesc = bob.udp_socket()?;
    bob.udp_bind(bob_fd, bob_addr)?;

    // Send data to Bob.
    let buf: DemiBuffer = DemiBuffer::from_slice(&vec![0x5a; 32][..]).expect("slice should fit in DemiBuffer");
    match alice.udp_pushto(QDesc::try_from(u32::MAX)?, buf.clone(), bob_addr) {
        Err(e) if e.errno == EBADF => {},
        _ => anyhow::bail!("pushto should have failed"),
    };

    now += Duration::from_micros(1);

    // Close peers.
    alice.udp_close(alice_fd)?;
    bob.udp_close(bob_fd)?;

    Ok(())
}
