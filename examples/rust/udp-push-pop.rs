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
        Err(e) => panic!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    // If error, free scatter-gather array.
    if sga.sga_segs[0].sgaseg_len as usize != size {
        freesga(libos, sga);
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            size,
            sga.sga_segs[0].sgaseg_len
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
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };
    let nsends: usize = 1024;
    let nreceives: usize = (8 * nsends) / 128;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
        Ok(sockqd) => sockqd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, close socket.
            close(&mut libos, sockqd);
            anyhow::bail!("bind failed: {:?}", e)
        },
    };

    // Get at least nreceives.
    for i in 0..nreceives {
        // Receive data.
        let qt: QToken = match libos.pop(sockqd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, close socket.
                close(&mut libos, sockqd);
                anyhow::bail!("pop failed: {:?}", e)
            },
        };
        let sga: demi_sgarray_t = match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Ok(_) => {
                // If error, close socket.
                close(&mut libos, sockqd);
                anyhow::bail!("unexpected result")
            },
            Err(e) => {
                // If error, close socket.
                close(&mut libos, sockqd);
                anyhow::bail!("operation failed: {:?}", e)
            },
        };

        // Sanity check received data.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        for x in slice {
            if *x != FILL_CHAR {
                // If not matching, close socket and free scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("fill check failed: expected={:?} received={:?}", FILL_CHAR, *x);
            }
        }

        if let Err(e) = libos.sgafree(sga) {
            // If error, close socket.
            close(&mut libos, sockqd);
            anyhow::bail!("failed to release scatter-gather array: {:?}", e)
        }
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
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
    };
    let nsends: usize = 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
        Ok(sockqd) => sockqd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => {
            // If error, close socket.
            close(&mut libos, sockqd);
            anyhow::bail!("bind failed: {:?}", e)
        },
    };

    // Issue n sends.
    for i in 0..nsends {
        let sga: demi_sgarray_t = match mksga(&mut libos, BUFFER_SIZE, FILL_CHAR) {
            Ok(sga) => sga,
            Err(e) => {
                // If error, close socket and free scatter-gather array.
                close(&mut libos, sockqd);
                anyhow::bail!("failed to allocate scatter-gather array: {:?}", e);
            },
        };

        // Send data.
        let qt: QToken = match libos.pushto(sockqd, &sga, remote) {
            Ok(qt) => qt,
            Err(e) => {
                // If error, close socket and free scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("push failed: {:?}", e)
            },
        };
        match libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
            Err(e) => {
                // If error, close socket and free scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("operation failed: {:?}", e)
            },
            _ => {
                // If error, close socket and free scatter-gather array.
                freesga(&mut libos, sga);
                close(&mut libos, sockqd);
                anyhow::bail!("unexpected result")
            },
        };
        if let Err(e) = libos.sgafree(sga) {
            // If error, close socket.
            close(&mut libos, sockqd);
            anyhow::bail!("failed to release scatter-gather array: {:?}", e)
        }

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
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
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
