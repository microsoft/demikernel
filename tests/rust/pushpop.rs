// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod common;

//==============================================================================
// Imports
//==============================================================================

use self::common::Test;
use ::demikernel::{
    OperationResult,
    QDesc,
    QToken,
};
use ::std::{
    net::SocketAddrV4,
    panic,
};

#[cfg(feature = "profiler")]
use perftools::profiler;

//==============================================================================
// UDP Push Pop
//==============================================================================

#[test]
fn udp_push_pop() {
    let mut test = Test::new();
    let fill_char: u8 = 'a' as u8;
    let nsends: usize = 1000;
    let nreceives: usize = (10 * nsends) / 100;
    let local_addr: SocketAddrV4 = test.local_addr();
    let remote_addr: SocketAddrV4 = test.remote_addr();

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
        let expectbuf: Vec<u8> = test.mkbuf(fill_char);

        // Get at least nreceives.
        for i in 0..nreceives {
            // Receive data.
            let qt: QToken = match test.libos.pop(sockfd) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to pop: {:?}", e.cause),
            };
            // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
            let recvbuf = match test.libos.wait2(qt) {
                Ok((_, OperationResult::Pop(_, buf))) => buf,
                _ => panic!("server failed to wait()"),
            };

            // Sanity received buffer.
            assert_eq!(expectbuf[..], recvbuf[..], "server expectbuf != recvbuf");
            println!("pop ({:?})", i);
        }

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
    } else {
        let sendbuf: Vec<u8> = test.mkbuf(fill_char);

        // Issue n sends.
        for i in 0..nsends {
            // Send data.
            let qt: QToken = match test.libos.pushto2(sockfd, sendbuf.as_slice(), remote_addr) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to push: {:?}", e.cause),
            };
            match test.libos.wait(qt) {
                Ok(_) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
            };
            println!("push ({:?})", i);
        }

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
    }

    // TODO: close socket when we get close working properly in catnip.
}

//==============================================================================
// TCP Push Pop
//==============================================================================

#[test]
fn tcp_push_pop() {
    let mut test: Test = Test::new();
    let fill_char: u8 = 'a' as u8;
    let nbytes: usize = 64 * 1024;
    let local_addr: SocketAddrV4 = test.local_addr();
    let remote_addr: SocketAddrV4 = test.remote_addr();

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
        let mut i: usize = 0;
        while i < nbytes {
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

            i += recvbuf.len();
            println!("pop {:?}", i);
        }

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
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
        let mut i: usize = 0;
        while i < nbytes {
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

            i += sendbuf.len();
            println!("push {:?}", i);
        }

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
    }

    // TODO: close socket when we get close working properly in catnip.
}
