// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod common;

//==============================================================================
// Imports
//==============================================================================

use self::common::Test;
use ::demikernel::{
    Ipv4Endpoint,
    OperationResult,
    QDesc,
    QToken,
};
use ::std::{
    panic,
    process,
    sync::{
        mpsc,
        mpsc::{
            Receiver,
            Sender,
        },
    },
    thread,
    thread::JoinHandle,
    time::Duration,
};

//==============================================================================
// UDP Ping Pong
//==============================================================================

#[test]
fn udp_ping_pong() {
    let mut test: Test = Test::new();
    let fill_char: u8 = 'a' as u8;
    let local_addr: Ipv4Endpoint = test.local_addr();
    let remote_addr: Ipv4Endpoint = test.remote_addr();

    // Setup peer.
    let sockfd: QDesc = match test.libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match test.libos.bind(sockfd, local_addr) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Run peers.
    if test.is_server() {
        let mut npongs: usize = 0;
        loop {
            let sendbuf: Vec<u8> = test.mkbuf(fill_char);
            let mut qt: QToken = match test.libos.pop(sockfd) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to pop: {:?}", e.cause),
            };

            // Spawn timeout thread.
            let (sender, receiver): (Sender<i32>, Receiver<i32>) = mpsc::channel();
            let t: JoinHandle<()> =
                thread::spawn(
                    move || match receiver.recv_timeout(Duration::from_secs(60)) {
                        Ok(_) => {},
                        _ => process::exit(0),
                    },
                );

            // Wait for incoming data.
            // TODO: add type annotation to the following variable once we drop generics on OperationResult.
            let recvbuf = match test.libos.wait2(qt) {
                Ok((_, OperationResult::Pop(_, buf))) => buf,
                _ => panic!("server failed to wait()"),
            };

            // Join timeout thread.
            sender.send(0).expect("failed to join timeout thread");
            t.join().expect("failed to join thread");

            // Sanity check contents of received buffer.
            assert!(
                Test::bufcmp(&sendbuf, recvbuf),
                "server sendbuf != recevbuf"
            );

            // Send data.
            qt = match test.libos.pushto2(sockfd, sendbuf.as_slice(), remote_addr) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to push: {:?}", e.cause),
            };
            match test.libos.wait(qt) {
                Ok(_) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
            };

            npongs += 1;
            println!("pong {:?}", npongs);
        }
    } else {
        let mut npongs: usize = 1000;
        let mut npings: usize = 0;
        let mut qtokens: Vec<QToken> = Vec::new();
        let sendbuf: Vec<u8> = test.mkbuf(fill_char);

        // Push pop first packet.
        let (qt_push, qt_pop): (QToken, QToken) = {
            let qt_push: QToken = match test.libos.pushto2(sockfd, sendbuf.as_slice(), remote_addr)
            {
                Ok(qt) => qt,
                Err(e) => panic!("failed to push: {:?}", e.cause),
            };
            let qt_pop: QToken = match test.libos.pop(sockfd) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to pop: {:?}", e.cause),
            };
            (qt_push, qt_pop)
        };
        qtokens.push(qt_push);
        qtokens.push(qt_pop);

        // Send packets.
        while npongs > 0 {
            // FIXME: If any packet is lost this will hang.

            // TODO: add type annotation to the following variable once we drop generics on OperationResult.
            let (ix, _, result) = match test.libos.wait_any2(&qtokens) {
                Ok(result) => result,
                Err(e) => panic!("operation failed: {:?}", e.cause),
            };
            qtokens.swap_remove(ix);

            // Parse result.
            match result {
                OperationResult::Push => {
                    let (qt_push, qt_pop): (QToken, QToken) = {
                        let qt_push: QToken =
                            match test.libos.pushto2(sockfd, sendbuf.as_slice(), remote_addr) {
                                Ok(qt) => qt,
                                Err(e) => panic!("failed to push: {:?}", e.cause),
                            };
                        let qt_pop: QToken = match test.libos.pop(sockfd) {
                            Ok(qt) => qt,
                            Err(e) => panic!("failed to pop: {:?}", e.cause),
                        };
                        (qt_push, qt_pop)
                    };
                    qtokens.push(qt_push);
                    qtokens.push(qt_pop);
                    println!("ping {:?}", npings);
                },
                OperationResult::Pop(_, recvbuf) => {
                    // Sanity received buffer.
                    assert!(
                        Test::bufcmp(&sendbuf, recvbuf),
                        "server expectbuf != recevbuf"
                    );
                    npings += 1;
                    npongs -= 1;
                },
                _ => panic!("unexpected result"),
            }
        }
    }

    // TODO: close socket when we get close working properly in catnip.
}

//==============================================================================
// TCP Ping Pong Single Connection
//==============================================================================

#[test]
fn tcp_ping_pong_single() {
    let mut test: Test = Test::new();
    let fill_char: u8 = 'a' as u8;
    let nrounds: usize = 1024;
    let local_addr: Ipv4Endpoint = test.local_addr();
    let remote_addr: Ipv4Endpoint = test.remote_addr();
    let expectbuf: Vec<u8> = test.mkbuf(fill_char);

    // Setup peer.
    let sockqd: QDesc = match test.libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match test.libos.bind(sockqd, local_addr) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Run peers.
    if test.is_server() {
        // Mark as a passive one.
        match test.libos.listen(sockqd, 16) {
            Ok(()) => (),
            Err(e) => panic!("listen failed: {:?}", e.cause),
        };

        // Accept incoming connections.
        let qt: QToken = match test.libos.accept(sockqd) {
            Ok(qt) => qt,
            Err(e) => panic!("accept failed: {:?}", e.cause),
        };
        let qd: QDesc = match test.libos.wait2(qt) {
            Ok((_, OperationResult::Accept(qd))) => qd,
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // Perform multiple ping-pong rounds.
        for i in 0..nrounds {
            // Pop data.
            let qtoken: QToken = match test.libos.pop(qd) {
                Ok(qt) => qt,
                Err(e) => panic!("pop failed: {:?}", e.cause),
            };
            // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
            let recvbuf = match test.libos.wait2(qtoken) {
                Ok((_, OperationResult::Pop(_, buf))) => buf,
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            // Sanity check received data.
            assert!(
                Test::bufcmp(&expectbuf, recvbuf.clone()),
                "server expectbuf != recvbuf"
            );

            // Push data.
            let qt: QToken = match test.libos.push2(qd, &recvbuf[..]) {
                Ok(qt) => qt,
                Err(e) => panic!("push failed: {:?}", e.cause),
            };
            match test.libos.wait2(qt) {
                Ok((_, OperationResult::Push)) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            println!("pong {:?}", i);
        }
    } else {
        let sendbuf: Vec<u8> = test.mkbuf(fill_char);
        let qt: QToken = match test.libos.connect(sockqd, remote_addr) {
            Ok(qt) => qt,
            Err(e) => panic!("connect failed: {:?}", e.cause),
        };
        match test.libos.wait2(qt) {
            Ok((_, OperationResult::Connect)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // Issue n sends.
        for i in 0..nrounds {
            // Push data.
            let qt: QToken = match test.libos.push2(sockqd, &sendbuf[..]) {
                Ok(qt) => qt,
                Err(e) => panic!("push failed: {:?}", e.cause),
            };
            match test.libos.wait2(qt) {
                Ok((_, OperationResult::Push)) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            // Pop data.
            let qtoken: QToken = match test.libos.pop(sockqd) {
                Ok(qt) => qt,
                Err(e) => panic!("pop failed: {:?}", e.cause),
            };
            // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
            let recvbuf = match test.libos.wait2(qtoken) {
                Ok((_, OperationResult::Pop(_, buf))) => buf,
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            // Sanity check received data.
            assert!(
                Test::bufcmp(&expectbuf, recvbuf.clone()),
                "server expectbuf != recvbuf"
            );

            println!("ping {:?}", i);
        }
    }

    // TODO: close socket when we get close working properly in catnip.
}

//==============================================================================
// TCP Ping Pong Multiple Connections
//==============================================================================

#[test]
fn tcp_ping_pong_multiple() {
    let mut test: Test = Test::new();
    let fill_char: u8 = 'a' as u8;
    let nrounds: usize = 1024;
    let local_addr: Ipv4Endpoint = test.local_addr();
    let remote_addr: Ipv4Endpoint = test.remote_addr();
    let expectbuf: Vec<u8> = test.mkbuf(fill_char);

    // Setup peer.
    let sockqd: QDesc = match test.libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match test.libos.bind(sockqd, local_addr) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Run peers.
    if test.is_server() {
        let mut npongs: usize = 0;
        let mut qtokens: Vec<QToken> = Vec::new();

        // Mark as a passive one.
        match test.libos.listen(sockqd, 16) {
            Ok(()) => (),
            Err(e) => panic!("listen failed: {:?}", e.cause),
        };

        // Accept incoming connections.
        let qt: QToken = match test.libos.accept(sockqd) {
            Ok(qt) => qt,
            Err(e) => panic!("accept failed: {:?}", e.cause),
        };
        qtokens.push(qt);

        // Perform multiple ping-pong rounds.
        while npongs < nrounds {
            // TODO: add type annotation to the following variable once we drop generics on OperationResult.
            let (idx, qd, qr) = match test.libos.wait_any2(&qtokens) {
                Ok((idx, qd, qr)) => (idx, qd, qr),
                Err(e) => panic!("accept failed: {:?}", e.cause),
            };
            qtokens.swap_remove(idx);

            match qr {
                OperationResult::Accept(qd) => {
                    // Accept more connections.
                    let qt: QToken = match test.libos.accept(sockqd) {
                        Ok(qt) => qt,
                        Err(e) => panic!("accept failed: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                    // First pop on connection.
                    let qt: QToken = match test.libos.pop(qd) {
                        Ok(qt) => qt,
                        Err(e) => panic!("pop failed: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                OperationResult::Pop(_, buf) => {
                    // Sanity check received data.
                    assert!(
                        Test::bufcmp(&expectbuf, buf.clone()),
                        "server expectbuf != recvbuf"
                    );
                    let qt: QToken = match test.libos.push2(qd, &buf[..]) {
                        Ok(qt) => qt,
                        Err(e) => panic!("push failed: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                OperationResult::Push => {
                    npongs += 1;
                    println!("npong {:?}", npongs);
                    // Pop more data.
                    let qt: QToken = match test.libos.pop(qd) {
                        Ok(qt) => qt,
                        Err(e) => panic!("pop failed: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                _ => unreachable!(),
            };
        }
    } else {
        let sendbuf: Vec<u8> = test.mkbuf(fill_char);
        let qt: QToken = match test.libos.connect(sockqd, remote_addr) {
            Ok(qt) => qt,
            Err(e) => panic!("connect failed: {:?}", e.cause),
        };
        match test.libos.wait2(qt) {
            Ok((_, OperationResult::Connect)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // Issue n sends.
        for i in 0..nrounds {
            // Push data.
            let qt: QToken = match test.libos.push2(sockqd, &sendbuf[..]) {
                Ok(qt) => qt,
                Err(e) => panic!("push failed: {:?}", e.cause),
            };
            match test.libos.wait2(qt) {
                Ok((_, OperationResult::Push)) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            // Pop data.
            let qtoken: QToken = match test.libos.pop(sockqd) {
                Ok(qt) => qt,
                Err(e) => panic!("pop failed: {:?}", e.cause),
            };
            // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
            let recvbuf = match test.libos.wait2(qtoken) {
                Ok((_, OperationResult::Pop(_, buf))) => buf,
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            // Sanity check received data.
            assert!(
                Test::bufcmp(&expectbuf, recvbuf.clone()),
                "server expectbuf != recvbuf"
            );

            println!("ping {:?}", i);
        }
    }

    // TODO: close socket when we get close working properly in catnip.
}
