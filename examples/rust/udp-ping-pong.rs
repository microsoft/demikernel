// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    process,
    str::FromStr,
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

#[cfg(feature = "profiler")]
use ::inetstack::runtime::perftools::profiler;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;

//======================================================================================================================
// mkbuf()
//======================================================================================================================

fn mkbuf(buffer_size: usize, fill_char: u8) -> Vec<u8> {
    let mut data: Vec<u8> = Vec::<u8>::with_capacity(buffer_size);

    for _ in 0..buffer_size {
        data.push(fill_char);
    }

    data
}

//======================================================================================================================
// server()
//======================================================================================================================

fn server(local: SocketAddrV4, remote: SocketAddrV4) -> ! {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
    let fill_char: u8 = 'a' as u8;

    // Setup peer.
    let sockfd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockfd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    let mut npongs: usize = 0;
    loop {
        let sendbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);
        let mut qt: QToken = match libos.pop(sockfd) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to pop: {:?}", e.cause),
        };

        // Spawn timeout thread.
        let (sender, receiver): (Sender<i32>, Receiver<i32>) = mpsc::channel();
        let t: JoinHandle<()> = thread::spawn(move || match receiver.recv_timeout(Duration::from_secs(60)) {
            Ok(_) => {},
            _ => process::exit(0),
        });

        // Wait for incoming data.
        // TODO: add type annotation to the following variable once we drop generics on OperationResult.
        let recvbuf = match libos.wait2(qt) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            _ => panic!("server failed to wait()"),
        };

        // Join timeout thread.
        sender.send(0).expect("failed to join timeout thread");
        t.join().expect("failed to join thread");

        // Sanity check contents of received buffer.
        assert_eq!(sendbuf[..], recvbuf[..], "server sendbuf != recevbuf");

        // Send data.
        qt = match libos.pushto2(sockfd, sendbuf.as_slice(), remote) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to push: {:?}", e.cause),
        };
        match libos.wait(qt) {
            Ok(_) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };

        npongs += 1;
        println!("pong {:?}", npongs);
    }

    // TODO: close socket when we get close working properly in catnip.
}

//======================================================================================================================
// client()
//======================================================================================================================

fn client(local: SocketAddrV4, remote: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
    let fill_char: u8 = 'a' as u8;

    // Setup peer.
    let sockfd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockfd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    let mut npongs: usize = 1000;
    let mut npings: usize = 0;
    let mut qtokens: Vec<QToken> = Vec::new();
    let sendbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);

    // Push pop first packet.
    let (qt_push, qt_pop): (QToken, QToken) = {
        let qt_push: QToken = match libos.pushto2(sockfd, sendbuf.as_slice(), remote) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to push: {:?}", e.cause),
        };
        let qt_pop: QToken = match libos.pop(sockfd) {
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
        let (ix, _, result) = match libos.wait_any2(&qtokens) {
            Ok(result) => result,
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        qtokens.swap_remove(ix);

        // Parse result.
        match result {
            OperationResult::Push => {
                let (qt_push, qt_pop): (QToken, QToken) = {
                    let qt_push: QToken = match libos.pushto2(sockfd, sendbuf.as_slice(), remote) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to push: {:?}", e.cause),
                    };
                    let qt_pop: QToken = match libos.pop(sockfd) {
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
                assert_eq!(sendbuf[..], recvbuf[..], "server expectbuf != recevbuf");
                npings += 1;
                npongs -= 1;
            },
            _ => panic!("unexpected result"),
        }
    }

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.

    Ok(())
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &String) {
    println!("Usage: {} MODE local remote\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.\n");
    println!("  --server    Run program in server mode.\n");
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 4 {
        let local: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
        let remote: SocketAddrV4 = SocketAddrV4::from_str(&args[3])?;
        if args[1] == "--server" {
            server(local, remote);
        } else if args[1] == "--client" {
            let ret: Result<()> = client(local, remote);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
