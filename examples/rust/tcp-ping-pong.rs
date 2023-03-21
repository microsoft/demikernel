// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    slice,
    str::FromStr,
    u8,
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;

//======================================================================================================================
// mksga()
//======================================================================================================================

// Makes a scatter-gather array.
fn mksga(libos: &mut LibOS, size: usize, value: u8) -> demi_sgarray_t {
    // Allocate scatter-gather array.
    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => panic!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    assert!(sga.sga_segs[0].sgaseg_len as usize == size);

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

    sga
}

//======================================================================================================================
// accept_and_wait()
//======================================================================================================================

/// Accepts a connection on a socket and waits for the operation to complete.
fn accept_and_wait(libos: &mut LibOS, sockqd: QDesc) -> QDesc {
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => unsafe { qr.qr_value.ares.qd.into() },
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => panic!("unexpected result"),
    }
}

//======================================================================================================================
// connect_and_wait()
//======================================================================================================================

/// Connects to a remote socket and wait for the operation to complete.
fn connect_and_wait(libos: &mut LibOS, sockqd: QDesc, remote: SocketAddrV4) {
    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => panic!("connect failed: {:?}", e.cause),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
        Err(e) => panic!("operation failed: {:?}", e),
        _ => panic!("unexpected result"),
    }
}

//======================================================================================================================
// push_and_wait()
//======================================================================================================================

/// Pushes a scatter-gather array to a remote socket and waits for the operation to complete.
fn push_and_wait(libos: &mut LibOS, qd: QDesc, sga: &demi_sgarray_t) {
    // Push data.
    let qt: QToken = match libos.push(qd, sga) {
        Ok(qt) => qt,
        Err(e) => panic!("push failed: {:?}", e.cause),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => panic!("unexpected result"),
    };
}

//======================================================================================================================
// pop_and_wait()
//======================================================================================================================

/// Pops a scatter-gather array from a socket and waits for the operation to complete.
fn pop_and_wait(libos: &mut LibOS, qd: QDesc, recvbuf: &mut [u8]) {
    let mut index: usize = 0;

    // Pop data.
    while index < recvbuf.len() {
        let qt: QToken = match libos.pop(qd, None) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => panic!("unexpected result"),
        };

        // Copy data.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        for x in slice {
            recvbuf[index] = *x;
            index += 1;
        }

        match libos.sgafree(sga) {
            Ok(_) => {},
            Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
        }
    }
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
    let nrounds: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Mark as a passive one.
    match libos.listen(sockqd, 16) {
        Ok(()) => (),
        Err(e) => panic!("listen failed: {:?}", e.cause),
    };

    // Accept incoming connections.
    let qd: QDesc = accept_and_wait(&mut libos, sockqd);

    // Perform multiple ping-pong rounds.
    for i in 0..nrounds {
        let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Pop data, and sanity check it.
        {
            let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            pop_and_wait(&mut libos, qd, &mut recvbuf);
            for x in &recvbuf {
                assert!(*x == fill_char);
            }
        }

        // Push data.
        {
            let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, fill_char);
            push_and_wait(&mut libos, qd, &sga);
            match libos.sgafree(sga) {
                Ok(_) => {},
                Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
            }
        }

        println!("pong {:?}", i);
    }

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.

    Ok(())
}

//======================================================================================================================
// client()
//======================================================================================================================

fn client(remote: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
    let nrounds: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };

    connect_and_wait(&mut libos, sockqd, remote);

    // Issue n sends.
    for i in 0..nrounds {
        let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Push data.
        {
            let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, fill_char);
            push_and_wait(&mut libos, sockqd, &sga);
            match libos.sgafree(sga) {
                Ok(_) => {},
                Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
            }
        }

        // Pop data, and sanity check it.
        {
            let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            pop_and_wait(&mut libos, sockqd, &mut recvbuf);
            for x in &recvbuf {
                assert!(*x == fill_char);
            }
        }

        println!("ping {:?}", i);
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
    println!("Usage: {} MODE address", program_name);
    println!("Modes:");
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
        if args[1] == "--server" {
            let ret: Result<()> = server(sockaddr);
            return ret;
        } else if args[1] == "--client" {
            let ret: Result<()> = client(sockaddr);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
