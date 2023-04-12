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
fn mksga(libos: &mut LibOS, size: usize, value: u8) -> Result<demi_sgarray_t> {
    // Allocate scatter-gather array.
    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    // If error, free scatter-gather array.
    // FIXME: https://github.com/demikernel/demikernel/issues/649
    assert_eq!(sga.sga_segs[0].sgaseg_len as usize, size);

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    let mut fill: u8 = value;
    for x in slice {
        *x = fill;
        fill = (fill % (u8::MAX - 1) + 1) as u8;
    }

    Ok(sga)
}

//======================================================================================================================
// accept_and_wait()
//======================================================================================================================

/// Accepts a connection on a socket and waits for the operation to complete.
fn accept_and_wait(libos: &mut LibOS, sockqd: QDesc) -> Result<QDesc> {
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("accept failed: {:?}", e.cause)
        },
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => Ok(unsafe { qr.qr_value.ares.qd.into() }),
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("operation failed: {:?}", e.cause)
        },
        _ => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("unexpected result")
        },
    }
}

//======================================================================================================================
// connect_and_wait()
//======================================================================================================================

/// Connects to a remote socket and wait for the operation to complete.
fn connect_and_wait(libos: &mut LibOS, sockqd: QDesc, remote: SocketAddrV4) -> Result<()> {
    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("connect failed: {:?}", e.cause)
        },
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("operation failed: {:?}", e)
        },
        _ => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("unexpected result")
        },
    };

    Ok(())
}

//======================================================================================================================
// push_and_wait()
//======================================================================================================================

/// Pushes a scatter-gather array to a remote socket and waits for the operation to complete.
fn push_and_wait(libos: &mut LibOS, qd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
    // Push data.
    let qt: QToken = match libos.push(qd, sga) {
        Ok(qt) => qt,
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("push failed: {:?}", e.cause)
        },
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("operation failed: {:?}", e.cause)
        },
        _ => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("unexpected result")
        },
    };

    Ok(())
}

//======================================================================================================================
// pop_and_wait()
//======================================================================================================================

/// Pops a scatter-gather array from a socket and waits for the operation to complete.
fn pop_and_wait(libos: &mut LibOS, qd: QDesc, recvbuf: &mut [u8]) -> Result<()> {
    let mut index: usize = 0;

    // Pop data.
    while index < recvbuf.len() {
        let qt: QToken = match libos.pop(qd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, free socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/649
                anyhow::bail!("pop failed: {:?}", e.cause)
            },
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Err(e) => {
                // If error, free socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/649
                anyhow::bail!("operation failed: {:?}", e.cause)
            },
            _ => {
                // If error, free socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/649
                anyhow::bail!("unexpected result")
            },
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
            Err(e) => {
                // If error, free socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/649
                anyhow::bail!("failed to release scatter-gather array: {:?}", e)
            },
        }
    }

    Ok(())
}

//======================================================================================================================
// server()
//======================================================================================================================

fn server(local: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };
    let nrounds: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("bind failed: {:?}", e.cause)
        },
    };

    // Mark as a passive one.
    match libos.listen(sockqd, 16) {
        Ok(()) => (),
        Err(e) => {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            anyhow::bail!("listen failed: {:?}", e.cause)
        },
    };

    // Accept incoming connections.
    // If error, free socket.
    // FIXME: https://github.com/demikernel/demikernel/issues/649
    let qd: QDesc = accept_and_wait(&mut libos, sockqd)?;

    // Perform multiple ping-pong rounds.
    for i in 0..nrounds {
        let mut fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Pop data, and sanity check it.
        {
            let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            pop_and_wait(&mut libos, qd, &mut recvbuf)?;
            for x in &recvbuf {
                assert_eq!(*x, fill_char);
                fill_char = (fill_char % (u8::MAX - 1) + 1) as u8;
            }
        }

        // Push data.
        {
            // If error, free socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/649
            let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, (i % (u8::MAX as usize - 1) + 1) as u8)?;
            push_and_wait(&mut libos, qd, &sga)?;
            match libos.sgafree(sga) {
                Ok(_) => {},
                Err(e) => {
                    // If error, free socket.
                    // FIXME: https://github.com/demikernel/demikernel/issues/649
                    anyhow::bail!("failed to release scatter-gather array: {:?}", e)
                },
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
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };
    let nrounds: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
    };

    connect_and_wait(&mut libos, sockqd, remote)?;

    // Issue n sends.
    for i in 0..nrounds {
        let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Push data.
        {
            let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, fill_char)?;
            push_and_wait(&mut libos, sockqd, &sga)?;
            match libos.sgafree(sga) {
                Ok(_) => {},
                Err(e) => {
                    // If error, free socket.
                    // FIXME: https://github.com/demikernel/demikernel/issues/649
                    anyhow::bail!("failed to release scatter-gather array: {:?}", e)
                },
            }
        }

        let mut fill_check: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Pop data, and sanity check it.
        {
            let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            pop_and_wait(&mut libos, sockqd, &mut recvbuf)?;
            for x in &recvbuf {
                // If error, free socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/649
                assert_eq!(*x, fill_check);
                fill_check = (fill_check % (u8::MAX - 1) + 1) as u8;
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
