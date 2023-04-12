// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
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
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

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
    // FIXME: https://github.com/demikernel/demikernel/issues/651
    assert_eq!(sga.sga_segs[0].sgaseg_len as usize, size);

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

    Ok(sga)
}

//======================================================================================================================
// server()
//======================================================================================================================

fn server(local: SocketAddrV4, remote: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };

    // Setup peer.
    let qd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(qd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, close socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/651
            anyhow::bail!("bind failed: {:?}", e.cause)
        },
    };

    let mut i: usize = 0;
    loop {
        // Pop data.
        let qt: QToken = match libos.pop(qd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("pop failed: {:?}", e.cause)
            },
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Err(e) => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("operation failed: {:?}", e.cause)
            },
            _ => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("unexpected result")
            },
        };

        // Sanity check received data.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        for x in slice {
            assert_eq!(*x, FILL_CHAR);
        }

        // Push data.
        let qt: QToken = match libos.pushto(qd, &sga, remote) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("push failed: {:?}", e.cause)
            },
        };
        match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
            Err(e) => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("operation failed: {:?}", e.cause)
            },
            _ => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("unexpected result")
            },
        };
        match libos.sgafree(sga) {
            Ok(_) => {},
            Err(e) => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("failed to release scatter-gather array: {:?}", e)
            },
        }

        i += 1;
        println!("pong {:?}", i);
    }
}

//======================================================================================================================
// client()
//======================================================================================================================

fn client(local: SocketAddrV4, remote: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };
    let npings: usize = 64;
    let mut qts: Vec<QToken> = Vec::new();

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, close socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/651
            anyhow::bail!("bind failed: {:?}", e.cause)
        },
    };

    // Push and pop data.
    let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, FILL_CHAR)?;
    match libos.pushto(sockqd, &sga, remote) {
        Ok(qt) => qts.push(qt),
        Err(e) => {
            // If error, close socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/651
            anyhow::bail!("push failed: {:?}", e.cause)
        },
    };
    match libos.sgafree(sga) {
        Ok(_) => {},
        Err(e) => {
            // If error, close socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/651
            anyhow::bail!("failed to release scatter-gather array: {:?}", e)
        },
    }
    match libos.pop(sockqd, None) {
        Ok(qt) => qts.push(qt),
        Err(e) => {
            // If error, close socket.
            // FIXME: https://github.com/demikernel/demikernel/issues/651
            anyhow::bail!("pop failed: {:?}", e.cause)
        },
    };

    // Send packets.
    let mut i: usize = 0;
    while i < npings {
        let (index, qr): (usize, demi_qresult_t) = match libos.wait_any(&qts, None) {
            Ok((index, qr)) => (index, qr),
            Err(e) => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("operation failed: {:?}", e.cause)
            },
        };
        qts.remove(index);

        match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_POP => {
                let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                // Sanity check received data.
                let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
                let len: usize = sga.sga_segs[0].sgaseg_len as usize;
                let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
                for x in slice {
                    assert_eq!(*x, FILL_CHAR);
                }

                i += 1;
                println!("ping {:?}", i);
                match libos.sgafree(sga) {
                    Ok(_) => {},
                    Err(e) => {
                        // If error, close socket.
                        // FIXME: https://github.com/demikernel/demikernel/issues/651
                        anyhow::bail!("failed to release scatter-gather array: {:?}", e)
                    },
                }
                match libos.pop(sockqd, None) {
                    Ok(qt) => qts.push(qt),
                    Err(e) => {
                        // If error, close socket.
                        // FIXME: https://github.com/demikernel/demikernel/issues/651
                        anyhow::bail!("pop failed: {:?}", e.cause)
                    },
                };
            },
            demi_opcode_t::DEMI_OPC_PUSH => {
                let sga: demi_sgarray_t = mksga(&mut libos, BUFFER_SIZE, FILL_CHAR)?;
                match libos.pushto(sockqd, &sga, remote) {
                    Ok(qt) => qts.push(qt),
                    Err(e) => {
                        // If error, close socket.
                        // FIXME: https://github.com/demikernel/demikernel/issues/651
                        anyhow::bail!("push failed: {:?}", e.cause)
                    },
                };
                match libos.sgafree(sga) {
                    Ok(_) => {},
                    Err(e) => {
                        // If error, close socket.
                        // FIXME: https://github.com/demikernel/demikernel/issues/651
                        anyhow::bail!("failed to release scatter-gather array: {:?}", e)
                    },
                }
            },
            _ => {
                // If error, close socket.
                // FIXME: https://github.com/demikernel/demikernel/issues/651
                anyhow::bail!("unexpected operation result")
            },
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
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
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
            return server(local, remote);
        } else if args[1] == "--client" {
            return client(local, remote);
        }
    }

    usage(&args[0]);

    Ok(())
}
