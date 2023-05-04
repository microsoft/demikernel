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
    if sga.sga_segs[0].sgaseg_len as usize != size {
        freesga(libos, sga);
        let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            size,
            seglen
        );
    }

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
// freesga()
//======================================================================================================================

/// Free scatter-gather array and warn on error.
fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        println!("ERROR: sgafree() failed (error={:?})", e);
        println!("WARN: leaking sga");
    }
}

//======================================================================================================================
// accept_and_wait()
//======================================================================================================================

/// Accepts a connection on a socket and waits for the operation to complete.
fn accept_and_wait(libos: &mut LibOS, sockqd: QDesc) -> Result<QDesc> {
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("accept failed: {:?}", e),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => Ok(unsafe { qr.qr_value.ares.qd.into() }),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    }
}

//======================================================================================================================
// connect_and_wait()
//======================================================================================================================

/// Connects to a remote socket and wait for the operation to complete.
fn connect_and_wait(libos: &mut LibOS, sockqd: QDesc, remote: SocketAddrV4) -> Result<()> {
    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("connect failed: {:?}", e),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    };

    Ok(())
}

//======================================================================================================================
// push_and_wait()
//======================================================================================================================

/// Pushes a scatter-gather array to a remote socket and waits for the operation to complete.
fn push_and_wait(libos: &mut LibOS, sockqd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
    // Push data.
    let qt: QToken = match libos.push(sockqd, sga) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("push failed: {:?}", e),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    };

    Ok(())
}

//======================================================================================================================
// pop_and_wait()
//======================================================================================================================

/// Pops a scatter-gather array from a socket and waits for the operation to complete.
fn pop_and_wait(libos: &mut LibOS, sockqd: QDesc, recvbuf: &mut [u8]) -> Result<()> {
    let mut index: usize = 0;

    // Pop data.
    while index < recvbuf.len() {
        let qt: QToken = match libos.pop(sockqd, None) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("pop failed: {:?}", e),
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Ok(_) => anyhow::bail!("unexpected result"),
            Err(e) => anyhow::bail!("operation failed: {:?}", e),
        };

        // Copy data.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        for x in slice {
            recvbuf[index] = *x;
            index += 1;
        }

        // Do not silently ignore if unable to free scatter-gather array.
        if let Err(e) = libos.sgafree(sga) {
            anyhow::bail!("failed to release scatter-gather array: {:?}", e);
        }
    }

    Ok(())
}

//======================================================================================================================
// close()
//======================================================================================================================

/// Closes a socket and warns if not successful.
fn close(libos: &mut LibOS, sockqd: QDesc) {
    if let Err(e) = libos.close(sockqd) {
        println!("ERROR: close() failed (error={:?})", e);
        println!("WARN: leaking sockqd={:?}", sockqd);
    }
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
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };
    let nrounds: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(sockqd) => sockqd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("bind failed: {:?}", e)
        },
    };

    // Mark as a passive one.
    match libos.listen(sockqd, 16) {
        Ok(()) => (),
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("listen failed: {:?}", e)
        },
    };

    // Accept incoming connections.
    // If error, free socket.
    let sockqd: QDesc = match accept_and_wait(&mut libos, sockqd) {
        Ok(sockqd) => sockqd,
        Err(e) => {
            close(&mut libos, sockqd);
            anyhow::bail!("accept failed: {:?}", e)
        },
    };

    // Perform multiple ping-pong rounds.
    for i in 0..nrounds {
        let mut fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Pop data, and sanity check it.
        {
            let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            // If error, free socket.
            if let Err(e) = pop_and_wait(&mut libos, sockqd, &mut recvbuf) {
                close(&mut libos, sockqd);
                anyhow::bail!("pop and wait failed: {:?}", e);
            }

            for x in &recvbuf {
                if *x != fill_char {
                    close(&mut libos, sockqd);
                    anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_char, *x);
                }
                fill_char = (fill_char % (u8::MAX - 1) + 1) as u8;
            }
        }

        // Push data.
        {
            // If error, free socket.
            let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, (i % (u8::MAX as usize - 1) + 1) as u8)?;
            if let Err(e) = push_and_wait(&mut libos, sockqd, &sga) {
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("push and wait failed: {:?}", e)
            }

            if let Err(e) = libos.sgafree(sga) {
                // If error, free socket.
                close(&mut libos, sockqd);
                anyhow::bail!("failed to release scatter-gather array: {:?}", e)
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
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };
    let nrounds: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(sockqd) => sockqd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };

    if let Err(e) = connect_and_wait(&mut libos, sockqd, remote) {
        close(&mut libos, sockqd);
        anyhow::bail!("connect and wait failed: {:?}", e);
    }

    // Issue n sends.
    for i in 0..nrounds {
        let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Push data.
        {
            let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, fill_char)?;
            if let Err(e) = push_and_wait(&mut libos, sockqd, &sga) {
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("push and wait failed: {:?}", e);
            }
            if let Err(e) = libos.sgafree(sga) {
                // If error, free socket.
                close(&mut libos, sockqd);
                anyhow::bail!("failed to release scatter-gather array: {:?}", e)
            }
        }

        let mut fill_check: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

        // Pop data, and sanity check it.
        {
            let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            if let Err(e) = pop_and_wait(&mut libos, sockqd, &mut recvbuf) {
                close(&mut libos, sockqd);
                anyhow::bail!("pop and wait failed: {:?}", e);
            }
            for x in &recvbuf {
                // If error, free socket.
                if *x != fill_check {
                    close(&mut libos, sockqd);
                    anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_check, *x);
                }
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
