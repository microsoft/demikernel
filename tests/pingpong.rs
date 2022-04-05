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
// Ping Pong
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
            let (ix, _, result) = test.libos.wait_any2(&qtokens);
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
