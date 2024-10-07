// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use anyhow::Result;
use demikernel::{LibOS, QDesc};
use std::net::{IpAddr, SocketAddr};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

pub fn run_tests(libos: &mut LibOS, local_ip_address: &IpAddr) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut test_results: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    crate::append_test_result!(
        test_results,
        crate::test!(close_bad_file_descriptor(libos, local_ip_address))
    );

    test_results
}

fn close_bad_file_descriptor(libos: &mut LibOS, local_ip_address: &IpAddr) -> Result<()> {
    let bind_address: SocketAddr = {
        let http_port: u16 = 8080;
        SocketAddr::new(*local_ip_address, http_port)
    };
    let socket_qd = libos.socket(AF_INET, SOCK_DGRAM, 0)?;

    libos.bind(socket_qd, bind_address)?;

    // Try to close a bad file descriptor. Must fail with EBADF.
    match libos.close(QDesc::try_from(u32::MAX)?) {
        Err(e) if e.errno == libc::EBADF => {},
        _ => anyhow::bail!("close should have failed"),
    };

    // Try to udp_close Bob two times.
    libos.close(socket_qd)?;

    // Must fail with EBADF.
    match libos.close(socket_qd) {
        Err(e) if e.errno == libc::EBADF => {},
        _ => anyhow::bail!("close should have failed"),
    };

    Ok(())
}
