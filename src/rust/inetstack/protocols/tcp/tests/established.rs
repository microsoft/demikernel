// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        protocols::tcp::tests::setup::{
            advance_clock,
            connection_setup,
        },
        test_helpers::{
            self,
            SharedEngine,
        },
    },
    runtime::{
        memory::DemiBuffer,
        QDesc,
        QToken,
    },
};
use ::anyhow::Result;
use ::std::{
    collections::VecDeque,
    net::SocketAddrV4,
    time::Instant,
};

//======================================================================================================================
// Helper Functions
//======================================================================================================================

/// This function tests the TCP close protocol.
fn connection_hangup(
    now: &mut Instant,
    server: &mut SharedEngine,
    client: &mut SharedEngine,
    server_qd: QDesc,
    client_qd: QDesc,
) -> Result<()> {
    // Send FIN: Client -> Server
    let _client_qt: QToken = match client.tcp_async_close(client_qd) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("client tcp_close returned error: {:?}", e),
    };
    client.get_test_rig().poll_scheduler();
    let client_frames: VecDeque<DemiBuffer> = client.get_test_rig().pop_all_frames();
    advance_clock(Some(server), Some(client), now);

    // ACK FIN: Server -> Client
    for frame in client_frames {
        if let Err(e) = server.receive(frame) {
            anyhow::bail!("server receive returned error: {:?}", e);
        }
    }
    server.get_test_rig().poll_scheduler();
    let server_frames: VecDeque<DemiBuffer> = server.get_test_rig().pop_all_frames();
    advance_clock(Some(server), Some(client), now);

    // Receive ACK FIN
    for frame in server_frames {
        if let Err(e) = client.receive(frame) {
            anyhow::bail!("client receive (of ACK) returned error: {:?}", e);
        }
    }
    advance_clock(Some(server), Some(client), now);

    // Send FIN: Server -> Client
    let _server_qt: QToken = match server.tcp_async_close(server_qd) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("server tcp_close returned error: {:?}", e),
    };
    server.get_test_rig().poll_scheduler();
    let server_frames: VecDeque<DemiBuffer> = server.get_test_rig().pop_all_frames();
    advance_clock(Some(server), Some(client), now);

    // ACK FIN: Client -> Server
    for frame in server_frames {
        if let Err(e) = client.receive(frame) {
            anyhow::bail!("client receive (of FIN) returned error {:?}", e);
        }
    }
    client.get_test_rig().poll_scheduler();
    let client_frames: VecDeque<DemiBuffer> = client.get_test_rig().pop_all_frames();
    advance_clock(Some(server), Some(client), now);

    // Receive ACK FIN
    for frame in client_frames {
        if let Err(e) = server.receive(frame) {
            anyhow::bail!("server receive returned error: {:?}", e);
        }
    }

    advance_clock(Some(server), Some(client), now);

    client.get_test_rig().poll_scheduler();
    server.get_test_rig().poll_scheduler();

    // FIXME: harvest client and server qts once close works.

    Ok(())
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

/// This tests connect and closing of a TCP connection.
#[test]
fn test_connect_disconnect() -> Result<()> {
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: SharedEngine = test_helpers::new_bob2(now);
    let mut client: SharedEngine = test_helpers::new_alice2(now);

    let ((server_qd, addr), client_qd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    connection_hangup(&mut now, &mut server, &mut client, server_qd, client_qd)?;

    Ok(())
}
