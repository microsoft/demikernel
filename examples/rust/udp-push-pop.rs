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
    str::FromStr,
};

#[cfg(feature = "profiler")]
use ::runtime::perftools::profiler;

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

fn server(local: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
    let fill_char: u8 = 'a' as u8;
    let nsends: usize = 1000;
    let nreceives: usize = (10 * nsends) / 100;

    // Setup peer.
    let sockfd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockfd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Run peers.
    let expectbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);

    // Get at least nreceives.
    for i in 0..nreceives {
        // Receive data.
        let qt: QToken = match libos.pop(sockfd) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to pop: {:?}", e.cause),
        };
        // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
        let recvbuf = match libos.wait2(qt) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            _ => panic!("server failed to wait()"),
        };

        // Sanity received buffer.
        assert_eq!(expectbuf[..], recvbuf[..], "server expectbuf != recvbuf");
        println!("pop ({:?})", i);
    }

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
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
    let nsends: usize = 1000;

    // Setup peer.
    let sockfd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockfd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Run peers.
    let sendbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);

    // Issue n sends.
    for i in 0..nsends {
        // Send data.
        let qt: QToken = match libos.pushto2(sockfd, sendbuf.as_slice(), remote) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to push: {:?}", e.cause),
        };
        match libos.wait(qt) {
            Ok(_) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };
        println!("push ({:?})", i);
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
    println!("Usage: {} MODE local [remote]\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.\n");
    println!("  --server    Run program in server mode.\n");
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        let local: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
        if args[1] == "--server" {
            let ret: Result<()> = server(local);
            return ret;
        } else if args[1] == "--client" && args.len() == 4 {
            let remote: SocketAddrV4 = SocketAddrV4::from_str(&args[3])?;
            let ret: Result<()> = client(local, remote);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
