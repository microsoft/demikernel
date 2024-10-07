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
        crate::test!(bind_to_address_in_use(libos, local_ip_address))
    );

    crate::append_test_result!(
        test_results,
        crate::test!(bind_using_bad_file_descriptor(libos, local_ip_address))
    );

    test_results
}

fn bind_to_address_in_use(libos: &mut LibOS, local_ip_address: &IpAddr) -> Result<()> {
    let first_socket_qd: QDesc = libos.socket(AF_INET, SOCK_DGRAM, 0)?;
    let second_socket_qd: QDesc = libos.socket(AF_INET, SOCK_DGRAM, 0)?;

    let bind_address: SocketAddr = {
        let http_port: u16 = 8080;
        SocketAddr::new(*local_ip_address, http_port)
    };

    libos.bind(first_socket_qd, bind_address)?;

    // Must fail with EADDRINUSE.
    match libos.bind(second_socket_qd, bind_address) {
        Err(e) if e.errno == libc::EADDRINUSE => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() the same address to two sockets should fail"),
    };

    libos.close(first_socket_qd)?;
    libos.close(second_socket_qd)?;
    Ok(())
}

fn bind_using_bad_file_descriptor(libos: &mut LibOS, local_ip_address: &IpAddr) -> Result<()> {
    let bad_socket_qd: QDesc = QDesc::try_from(u32::MAX)?;
    let bind_address: SocketAddr = {
        let http_port: u16 = 8080;
        SocketAddr::new(*local_ip_address, http_port)
    };

    // Must fail with EBADF.
    match libos.bind(bad_socket_qd, bind_address) {
        Err(e) if e.errno == libc::EBADF => {},
        _ => anyhow::bail!("bind should have failed"),
    };

    Ok(())
}
