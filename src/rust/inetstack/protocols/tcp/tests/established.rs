// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        protocols::tcp::{
            tests::{
                check_packet_data,
                check_packet_pure_ack,
                setup::{
                    advance_clock,
                    connection_setup,
                },
            },
            SeqNumber,
        },
        test_helpers::{
            self,
            SharedEngine,
        },
    },
    runtime::{
        memory::DemiBuffer,
        network::consts::RECEIVE_BATCH_SIZE,
        Operation,
        OperationResult,
        QDesc,
    },
    scheduler::TaskHandle,
};
use ::anyhow::Result;
use ::rand;
use ::std::{
    collections::VecDeque,
    net::SocketAddrV4,
    pin::Pin,
    time::Instant,
};

//======================================================================================================================
// Helper Functions
//======================================================================================================================

/// Cooks a buffer.
fn cook_buffer(size: usize, stamp: Option<u8>) -> DemiBuffer {
    assert!(size < u16::MAX as usize);
    let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
    for i in 0..size {
        buf[i] = stamp.unwrap_or(i as u8);
    }
    buf
}

/// This function pushes a DemiBuffer to the test engine and returns the emitted packets.
fn send_data<const N: usize>(
    now: &mut Instant,
    receiver: &mut SharedEngine<N>,
    sender: &mut SharedEngine<N>,
    sender_qd: QDesc,
    window_size: u16,
    seq_no: SeqNumber,
    ack_num: Option<SeqNumber>,
    bytes: DemiBuffer,
) -> Result<VecDeque<DemiBuffer>> {
    trace!(
        "send_data ====> push: {:?} -> {:?}",
        sender.get_test_rig().get_ip_addr(),
        receiver.get_test_rig().get_ip_addr()
    );

    // Push data.
    let push_coroutine: Pin<Box<Operation>> = sender.tcp_push(sender_qd, bytes.clone());
    let handle: TaskHandle = sender
        .get_test_rig()
        .get_runtime()
        .insert_coroutine("test::send_data::push_coroutine", push_coroutine)?;

    // Poll the coroutine.
    sender.get_test_rig().poll_scheduler();

    // Grab all frames emited by the sender's inetstack.
    let frames: VecDeque<DemiBuffer> = sender.get_test_rig().pop_all_frames();
    crate::ensure_neq!(frames.len(), 0);
    let mut outgoing_frames: VecDeque<DemiBuffer> = VecDeque::<DemiBuffer>::new();
    // Make sure that the last packet is the actual data.
    for frame in frames {
        let (_, retransmit): (usize, bool) = check_packet_data(
            frame.clone(),
            sender.get_test_rig().get_link_addr(),
            receiver.get_test_rig().get_link_addr(),
            sender.get_test_rig().get_ip_addr(),
            receiver.get_test_rig().get_ip_addr(),
            window_size,
            seq_no,
            ack_num,
        )?;
        // Just drop retransmissions.
        // FIXME: https://github.com/microsoft/demikernel/issues/979
        if !retransmit {
            outgoing_frames.push_back(frame.clone());
        }
    }

    advance_clock(Some(receiver), Some(sender), now);

    // Push completes.
    match sender
        .get_test_rig()
        .get_runtime()
        .remove_coroutine(&handle)
        .get_result()
    {
        Some((_, OperationResult::Push)) => {
            trace!("send_data ====> push completed");
            Ok(outgoing_frames)
        },
        Some((_, result)) => anyhow::bail!("push did not complete successfully: {:?}", result),
        None => anyhow::bail!("push should have completed"),
    }
}

/// This function processes an incoming data packet.
fn recv_data<const N: usize>(
    receiver: &mut SharedEngine<N>,
    sender: &mut SharedEngine<N>,
    receiver_qd: QDesc,
    bytes: DemiBuffer,
) -> Result<()> {
    trace!(
        "recv_data ====> pop: {:?} -> {:?}",
        sender.get_test_rig().get_ip_addr(),
        receiver.get_test_rig().get_ip_addr()
    );

    // Pop data.
    let pop_coroutine: Pin<Box<Operation>> = receiver.tcp_pop(receiver_qd);
    let handle: TaskHandle = receiver
        .get_test_rig()
        .get_runtime()
        .insert_coroutine("test::recv_data::pop_coroutine", pop_coroutine)?;

    // Deliver data.
    if let Err(e) = receiver.receive(bytes.clone()) {
        anyhow::bail!("receive returned error: {:?}", e);
    }

    // Poll the coroutine.
    receiver.get_test_rig().poll_scheduler();

    // Pop completes
    match receiver
        .get_test_rig()
        .get_runtime()
        .remove_coroutine(&handle)
        .get_result()
    {
        Some((_, OperationResult::Pop(_, _))) => {
            trace!("recv_data ====> pop completed");
            Ok(())
        },
        Some((_, result)) => anyhow::bail!("pop did not complete successfully: {:?}", result),
        None => anyhow::bail!("pop should have completed"),
    }
}

/// This function checks and processes an ack packet.
fn recv_pure_ack<const N: usize>(
    _: &mut Instant,
    sender: &mut SharedEngine<N>,
    receiver: &mut SharedEngine<N>,
    ack_num: SeqNumber,
    bytes: DemiBuffer,
) -> Result<()> {
    trace!(
        "recv_pure_ack ====> ack: {:?} -> {:?}",
        sender.get_test_rig().get_ip_addr(),
        receiver.get_test_rig().get_ip_addr()
    );

    sender.get_test_rig().poll_scheduler();

    // Pop pure ACK
    check_packet_pure_ack(
        bytes.clone(),
        sender.get_test_rig().get_link_addr(),
        receiver.get_test_rig().get_link_addr(),
        sender.get_test_rig().get_ip_addr(),
        receiver.get_test_rig().get_ip_addr(),
        ack_num,
    )?;
    match receiver.receive(bytes.clone()) {
        Ok(()) => trace!("recv_pure_ack ====> ack completed"),
        Err(e) => anyhow::bail!("did not receive an ack: {:?}", e),
    }

    Ok(())
}

/// This function sends and receives a single DemiBuffer between the client and server.
fn send_recv<const N: usize>(
    now: &mut Instant,
    server: &mut SharedEngine<N>,
    client: &mut SharedEngine<N>,
    server_qd: QDesc,
    client_qd: QDesc,
    window_size: u16,
    seq_no: SeqNumber,
    bytes: DemiBuffer,
) -> Result<()> {
    // Send data from the client to the server.
    let bufsize: usize = bytes.len();
    let frames: VecDeque<DemiBuffer> =
        send_data(now, server, client, client_qd, window_size, seq_no, None, bytes.clone())?;
    // Pop packets on the server.
    for frame in frames {
        if frame.len() > 0 {
            // Receive data.
            recv_data(server, client, server_qd, frame.clone())?;
        } else {
            // Receive a pure ACK.
            recv_pure_ack(now, server, client, seq_no + SeqNumber::from(bufsize as u32), frame)?;
        }
    }

    Ok(())
}

/// This function sends a DemiBuffer between the client and server and then back from the server to the client.
fn send_recv_round<const N: usize>(
    now: &mut Instant,
    server: &mut SharedEngine<N>,
    client: &mut SharedEngine<N>,
    server_qd: QDesc,
    client_qd: QDesc,
    window_size: u16,
    seq_no: SeqNumber,
    bytes: DemiBuffer,
) -> Result<()> {
    // Send the outgoing buffer from client to server.
    let bufsize: usize = bytes.len();

    // Push data from the client.
    let frames: VecDeque<DemiBuffer> = send_data(now, server, client, client_qd, window_size, seq_no, None, bytes)?;

    // Pop packets on the server
    for frame in frames {
        if frame.len() > 0 {
            // Pop data.
            recv_data(server, client, server_qd, frame.clone())?;
        } else {
            // Pop pure ACK.
            recv_pure_ack(now, server, client, seq_no + SeqNumber::from(bufsize as u32), frame)?;
        }
    }

    // Send a buffer from server to client
    let bytes: DemiBuffer = cook_buffer(bufsize, None);
    let frames: VecDeque<DemiBuffer> = send_data(
        now,
        client,
        server,
        server_qd,
        window_size,
        seq_no,
        Some(seq_no + SeqNumber::from(bufsize as u32)),
        bytes.clone(),
    )?;

    // Receive data on the client from the server.
    for frame in frames {
        if frame.len() > 0 {
            // Pop data.
            recv_data(client, server, client_qd, frame)?;
        } else {
            recv_pure_ack(now, client, server, seq_no + SeqNumber::from(bufsize as u32), frame)?;
        }
    }

    Ok(())
}

/// This function tests the TCP close protocol.
fn connection_hangup<const N: usize>(
    now: &mut Instant,
    server: &mut SharedEngine<N>,
    client: &mut SharedEngine<N>,
    server_qd: QDesc,
    client_qd: QDesc,
) -> Result<()> {
    // Send FIN: Client -> Server
    if let Err(e) = client.tcp_close(client_qd) {
        anyhow::bail!("client tcp_close returned error: {:?}", e);
    }
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
    if let Err(e) = server.tcp_close(server_qd) {
        anyhow::bail!("server tcp_close returned error: {:?}", e);
    }
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

    Ok(())
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

/// Tests one way communication. This should force the receiving peer to send
/// pure ACKs to the sender.
#[test]
pub fn test_send_recv_loop() -> Result<()> {
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);
    let window_scale: u8 = client.get_test_rig().get_tcp_config().get_window_scale();
    let max_window_size: u32 = match (client.get_test_rig().get_tcp_config().get_receive_window_size() as u32)
        .checked_shl(window_scale as u32)
    {
        Some(shift) => shift,
        None => anyhow::bail!("incorrect receive window"),
    };

    let ((server_qd, addr), client_qd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    let bufsize: u32 = 64;
    let buf: DemiBuffer = cook_buffer(bufsize as usize, None);

    for i in 0..((max_window_size + 1) / bufsize) {
        send_recv(
            &mut now,
            &mut server,
            &mut client,
            server_qd,
            client_qd,
            max_window_size as u16,
            SeqNumber::from(1 + i * bufsize),
            buf.clone(),
        )?;
    }

    Ok(())
}

/// This tests an echo between the client and the server in a loop.
#[test]
pub fn test_send_recv_round_loop() -> Result<()> {
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);
    let window_scale: u8 = client.get_test_rig().get_tcp_config().get_window_scale();
    let max_window_size: u32 = match (client.get_test_rig().get_tcp_config().get_receive_window_size() as u32)
        .checked_shl(window_scale as u32)
    {
        Some(shift) => shift,
        None => anyhow::bail!("incorrect receive window"),
    };
    let ((server_qd, addr), client_qd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    let bufsize: u32 = 64;
    let buf: DemiBuffer = cook_buffer(bufsize as usize, None);

    for i in 0..((max_window_size + 1) / bufsize) {
        send_recv_round(
            &mut now,
            &mut server,
            &mut client,
            server_qd,
            client_qd,
            max_window_size as u16,
            SeqNumber::from(1 + i * bufsize),
            buf.clone(),
        )?;
    }

    Ok(())
}

/// Tests one way communication, with some random transmission delay. This
/// should force the receiving peer to send pure ACKs to the sender, as well as
/// the sender side to trigger the RTO calculation logic.
#[test]
pub fn test_send_recv_with_delay() -> Result<()> {
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);
    let window_scale: u8 = client.get_test_rig().get_tcp_config().get_window_scale();
    let max_window_size: u32 = match (client.get_test_rig().get_tcp_config().get_receive_window_size() as u32)
        .checked_shl(window_scale as u32)
    {
        Some(shift) => shift,
        None => anyhow::bail!("incorrect receive window"),
    };

    let ((server_qd, addr), client_qd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    let bufsize: u32 = 64;
    let buf: DemiBuffer = cook_buffer(bufsize as usize, None);
    let mut recv_seq_no: SeqNumber = SeqNumber::from(1);
    let mut seq_no: SeqNumber = SeqNumber::from(1);
    let mut inflight = VecDeque::<DemiBuffer>::new();

    for _ in 0..((max_window_size + 1) / bufsize) {
        // Push data.
        let frames: VecDeque<DemiBuffer> = send_data(
            &mut now,
            &mut server,
            &mut client,
            client_qd,
            max_window_size as u16,
            seq_no,
            None,
            buf.clone(),
        )?;

        seq_no = seq_no + SeqNumber::from(bufsize);

        for frame in frames {
            if frame.len() > 0 {
                inflight.push_back(frame);
            } else {
                // Check pure ack.
                // FIXME: https://github.com/demikernel/demikernel/issues/680
                recv_pure_ack(&mut now, &mut server, &mut client, recv_seq_no, frame)?;
            }
        }
        // Pop data oftentimes.
        if rand::random() {
            if let Some(bytes) = inflight.pop_front() {
                recv_data(&mut server, &mut client, server_qd, bytes.clone())?;
                recv_seq_no = recv_seq_no + SeqNumber::from(bufsize as u32);
            }
        }
    }

    // Pop inflight packets.
    while let Some(bytes) = inflight.pop_front() {
        // Pop data.
        recv_data(&mut server, &mut client, server_qd, bytes.clone())?;
        recv_seq_no = recv_seq_no + SeqNumber::from(bufsize as u32);
    }

    Ok(())
}

/// This tests connect and closing of a TCP connection.
#[test]
fn test_connect_disconnect() -> Result<()> {
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: SharedEngine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);

    let ((server_qd, addr), client_qd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    connection_hangup(&mut now, &mut server, &mut client, server_qd, client_qd)?;

    Ok(())
}
