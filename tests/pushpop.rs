// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(try_blocks)]

mod common;

//==============================================================================
// Imports
//==============================================================================

use self::common::Test;
use ::catnip::{
    operations::OperationResult,
    protocols::ipv4::Ipv4Endpoint,
};
use ::std::panic;

//==============================================================================
// Push Pop
//==============================================================================

#[test]
fn udp_push_pop() {
    let mut test = Test::new();
    let payload: u8 = 'a' as u8;
    let nsends: usize = 1000;
    let nreceives: usize = (10 * nsends) / 100;
    let local_addr: Ipv4Endpoint = test.local_addr();
    let remote_addr: Ipv4Endpoint = test.remote_addr();

    // Setup peer.
    let sockfd = test
        .libos
        .socket(libc::AF_INET, libc::SOCK_DGRAM, 0)
        .unwrap();
    test.libos.bind(sockfd, local_addr).unwrap();

    // Run peers.
    if test.is_server() {
        let expectbuf = test.mkbuf(payload);

        // Get at least nreceives.
        for i in 0..nreceives {
            // Receive data.
            let qtoken = test.libos.pop(sockfd).expect("server failed to pop()");
            let recvbuf = match test.libos.wait2(qtoken) {
                (_, OperationResult::Pop(_, buf)) => buf,
                _ => panic!("server failed to wait()"),
            };

            // Sanity received buffer.
            assert!(
                Test::bufcmp(expectbuf.clone(), recvbuf),
                "server expectbuf != recevbuf"
            );
            println!("pop ({:?})", i);
        }
    } else {
        let sendbuf = test.mkbuf(payload);

        // Issue n sends.
        for i in 0..nsends {
            // Send data.
            let qtoken = test
                .libos
                .pushto2(sockfd, sendbuf.clone(), remote_addr)
                .expect("client failed to pushto2()");
            test.libos.wait(qtoken);
            println!("push ({:?})", i);
        }
    }
}
