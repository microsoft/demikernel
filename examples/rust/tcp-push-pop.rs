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
const FILL_CHAR: u8 = 0x65;

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
    slice.fill(value);

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
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };
    let nbytes: usize = BUFFER_SIZE * 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(sockqd) => sockqd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("bind failed: {:?}", e.cause)
        },
    };

    // Mark as a passive one.
    match libos.listen(sockqd, 16) {
        Ok(()) => (),
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("listen failed: {:?}", e.cause)
        },
    };

    // Accept incoming connections.
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("accept failed: {:?}", e.cause)
        },
    };
    let sockqd: QDesc = match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => unsafe { qr.qr_value.ares.qd.into() },
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("operation failed: {:?}", e.cause)
        },
        Ok(_) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("unexpected result")
        },
    };

    // Perform multiple ping-pong rounds.
    let mut i: usize = 0;
    while i < nbytes {
        // Pop data.
        let qt: QToken = match libos.pop(sockqd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, free socket.
                close(&mut libos, sockqd);
                anyhow::bail!("pop failed: {:?}", e.cause)
            },
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Err(e) => {
                // If error, free socket.
                close(&mut libos, sockqd);
                anyhow::bail!("operation failed: {:?}", e.cause)
            },
            Ok(_) => {
                // If error, free socket.
                close(&mut libos, sockqd);
                anyhow::bail!("unexpected result")
            },
        };

        // Sanity check received data.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        for x in slice {
            // If error, free socket.
            if *x != FILL_CHAR {
                close(&mut libos, sockqd);
                anyhow::bail!("fill check failed: expected={:?} received={:?}", FILL_CHAR, *x);
            }
        }

        i += sga.sga_segs[0].sgaseg_len as usize;
        if let Err(e) = libos.sgafree(sga) {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("failed to release scatter-gather array: {:?}", e)
        }
        println!("pop {:?}", i);
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
    let nbytes: usize = BUFFER_SIZE * 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
        Ok(sockqd) => sockqd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
    };

    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("connect failed: {:?}", e.cause)
        },
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
        Err(e) => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("operation failed: {:?}", e)
        },
        _ => {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("unexpected result")
        },
    }

    // Issue n sends.
    let mut i: usize = 0;
    while i < nbytes {
        // If error, free socket.
        let sga: demi_sgarray_t = match mksga(&mut libos, BUFFER_SIZE, FILL_CHAR) {
            Ok(sga) => sga,
            Err(e) => {
                close(&mut libos, sockqd);
                anyhow::bail!("failed to allocate scatter-gather array: {:?}", e)
            },
        };

        // Push data.
        let qt: QToken = match libos.push(sockqd, &sga) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, free socket and scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("push failed: {:?}", e.cause)
            },
        };
        match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
            Ok(_) => {
                // If error, free socket and scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("unexpected result")
            },
            Err(e) => {
                // If error, free socket and scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("operation failed: {:?}", e.cause)
            },
        };
        i += sga.sga_segs[0].sgaseg_len as usize;
        // Do not silently fail if unable to free scatter-gather array.
        if let Err(e) = libos.sgafree(sga) {
            // If error, free socket.
            close(&mut libos, sockqd);
            anyhow::bail!("failed to release scatter-gather array: {:?}", e)
        }

        println!("push {:?}", i);
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
    println!("Usage: {} MODE address\n", program_name);
    println!("Modes:\n");
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
