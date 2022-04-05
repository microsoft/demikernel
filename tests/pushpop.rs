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
use ::std::panic;

//==============================================================================
// Push Pop
//==============================================================================

#[test]
fn udp_push_pop() {
    let mut test = Test::new();
    let fill_char: u8 = 'a' as u8;
    let nsends: usize = 1000;
    let nreceives: usize = (10 * nsends) / 100;
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
            assert!(
                Test::bufcmp(&expectbuf, recvbuf),
                "server expectbuf != recvbuf"
            );
            println!("pop ({:?})", i);
        }
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
    }

    // TODO: close socket when we get close working properly in catnip.
}
