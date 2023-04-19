// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::{
        protocols::tcp::{
            operations::PushFuture,
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
            Engine,
        },
    },
    runtime::{
        memory::DemiBuffer,
        QDesc,
    },
};
use ::anyhow::Result;
use ::futures::task::noop_waker_ref;
use ::rand;
use ::std::{
    collections::VecDeque,
    future::Future,
    net::SocketAddrV4,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
    time::Instant,
};

//=============================================================================

/// Cooks a buffer.
fn cook_buffer(size: usize, stamp: Option<u8>) -> DemiBuffer {
    assert!(size < u16::MAX as usize);
    let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
    for i in 0..size {
        buf[i] = stamp.unwrap_or(i as u8);
    }
    buf
}

//=============================================================================

fn send_data(
    ctx: &mut Context,
    now: &mut Instant,
    receiver: &mut Engine,
    sender: &mut Engine,
    sender_fd: QDesc,
    window_size: u16,
    seq_no: SeqNumber,
    ack_num: Option<SeqNumber>,
    bytes: DemiBuffer,
) -> Result<(DemiBuffer, usize)> {
    trace!(
        "send_data ====> push: {:?} -> {:?}",
        sender.rt.ipv4_addr,
        receiver.rt.ipv4_addr
    );

    // Push data.
    let mut push_future: PushFuture = sender.tcp_push(sender_fd, bytes);

    let bytes: DemiBuffer = sender.rt.pop_frame();
    let bufsize: usize = check_packet_data(
        bytes.clone(),
        sender.rt.link_addr,
        receiver.rt.link_addr,
        sender.rt.ipv4_addr,
        receiver.rt.ipv4_addr,
        window_size,
        seq_no,
        ack_num,
    )?;

    advance_clock(Some(receiver), Some(sender), now);

    // Push completes.
    match Future::poll(Pin::new(&mut push_future), ctx) {
        Poll::Ready(Ok(())) => {
            trace!("send_data ====> push completed");

            Ok((bytes, bufsize))
        },
        _ => anyhow::bail!("push should have completed successfully"),
    }
}

//=============================================================================

fn recv_data(
    ctx: &mut Context,
    receiver: &mut Engine,
    sender: &mut Engine,
    receiver_fd: QDesc,
    bytes: DemiBuffer,
) -> Result<()> {
    trace!(
        "recv_data ====> pop: {:?} -> {:?}",
        sender.rt.ipv4_addr,
        receiver.rt.ipv4_addr
    );

    // Pop data.
    let mut pop_future = receiver.tcp_pop(receiver_fd);
    if let Err(e) = receiver.receive(bytes) {
        anyhow::bail!("receive returned error: {:?}", e);
    }

    // Pop completes
    match Future::poll(Pin::new(&mut pop_future), ctx) {
        Poll::Ready(Ok(_)) => {
            trace!("recv_data ====> pop completed");
            Ok(())
        },
        _ => anyhow::bail!("pop should have completed"),
    }
}

//=============================================================================

fn recv_pure_ack(now: &mut Instant, sender: &mut Engine, receiver: &mut Engine, ack_num: SeqNumber) -> Result<()> {
    trace!(
        "recv_pure_ack ====> ack: {:?} -> {:?}",
        sender.rt.ipv4_addr,
        receiver.rt.ipv4_addr
    );

    advance_clock(Some(sender), Some(receiver), now);
    sender.rt.poll_scheduler();

    // Pop pure ACK
    if let Some(bytes) = sender.rt.pop_frame_unchecked() {
        check_packet_pure_ack(
            bytes.clone(),
            sender.rt.link_addr,
            receiver.rt.link_addr,
            sender.rt.ipv4_addr,
            receiver.rt.ipv4_addr,
            ack_num,
        )?;
        match receiver.receive(bytes) {
            Ok(()) => trace!("recv_pure_ack ====> ack completed"),
            Err(e) => anyhow::bail!("did not receive an ack: {:?}", e),
        }
    };

    Ok(())
}

//=============================================================================

fn send_recv(
    ctx: &mut Context,
    now: &mut Instant,
    server: &mut Engine,
    client: &mut Engine,
    server_fd: QDesc,
    client_fd: QDesc,
    window_size: u16,
    seq_no: SeqNumber,
    bytes: DemiBuffer,
) -> Result<()> {
    let bufsize: usize = bytes.len();

    // Push data.
    let (bytes, _): (DemiBuffer, usize) = send_data(
        ctx,
        now,
        server,
        client,
        client_fd,
        window_size,
        seq_no,
        None,
        bytes.clone(),
    )?;

    // Pop data.
    recv_data(ctx, server, client, server_fd, bytes)?;

    // Pop pure ACK.
    recv_pure_ack(now, server, client, seq_no + SeqNumber::from(bufsize as u32))?;

    Ok(())
}

//=============================================================================

fn send_recv_round(
    ctx: &mut Context,
    now: &mut Instant,
    server: &mut Engine,
    client: &mut Engine,
    server_fd: QDesc,
    client_fd: QDesc,
    window_size: u16,
    seq_no: SeqNumber,
    bytes: DemiBuffer,
) -> Result<()> {
    // Push Data: Client -> Server
    let (bytes, bufsize): (DemiBuffer, usize) =
        send_data(ctx, now, server, client, client_fd, window_size, seq_no, None, bytes)?;

    // Pop data.
    recv_data(ctx, server, client, server_fd, bytes.clone())?;

    // Push Data: Server -> Client
    let bytes: DemiBuffer = cook_buffer(bufsize, None);
    let (bytes, _): (DemiBuffer, usize) = send_data(
        ctx,
        now,
        client,
        server,
        server_fd,
        window_size,
        seq_no,
        Some(seq_no + SeqNumber::from(bufsize as u32)),
        bytes,
    )?;

    // Pop data.
    recv_data(ctx, client, server, client_fd, bytes.clone())?;

    Ok(())
}

//=============================================================================

fn connection_hangup(
    _ctx: &mut Context,
    now: &mut Instant,
    server: &mut Engine,
    client: &mut Engine,
    server_fd: QDesc,
    client_fd: QDesc,
) -> Result<()> {
    // Send FIN: Client -> Server
    if let Err(e) = client.tcp_close(client_fd) {
        anyhow::bail!("client tcp_close returned error: {:?}", e);
    }
    client.rt.poll_scheduler();
    let bytes: DemiBuffer = client.rt.pop_frame();
    advance_clock(Some(server), Some(client), now);

    // ACK FIN: Server -> Client
    if let Err(e) = server.receive(bytes) {
        anyhow::bail!("server receive returned error: {:?}", e);
    }
    server.rt.poll_scheduler();
    let bytes: DemiBuffer = server.rt.pop_frame();
    advance_clock(Some(server), Some(client), now);

    // Receive ACK FIN
    if let Err(e) = client.receive(bytes) {
        anyhow::bail!("client receive (of ACK) returned error: {:?}", e);
    }
    advance_clock(Some(server), Some(client), now);

    // Send FIN: Server -> Client
    if let Err(e) = server.tcp_close(server_fd) {
        anyhow::bail!("server tcp_close returned error: {:?}", e);
    }
    server.rt.poll_scheduler();
    let bytes: DemiBuffer = server.rt.pop_frame();
    advance_clock(Some(server), Some(client), now);

    // ACK FIN: Client -> Server
    if let Err(e) = client.receive(bytes) {
        anyhow::bail!("client receive (of FIN) returned error {:?}", e);
    }
    client.rt.poll_scheduler();
    let bytes: DemiBuffer = client.rt.pop_frame();
    advance_clock(Some(server), Some(client), now);

    // Receive ACK FIN
    if let Err(e) = server.receive(bytes) {
        anyhow::bail!("server receive returned error: {:?}", e);
    }

    advance_clock(Some(server), Some(client), now);

    client.rt.poll_scheduler();
    server.rt.poll_scheduler();

    Ok(())
}

//=============================================================================

/// Tests one way communication. This should force the receiving peer to send
/// pure ACKs to the sender.
#[test]
pub fn test_send_recv_loop() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine = test_helpers::new_bob2(now);
    let mut client: Engine = test_helpers::new_alice2(now);
    let window_scale: u8 = client.rt.tcp_config.get_window_scale();
    let max_window_size: u32 =
        match (client.rt.tcp_config.get_receive_window_size() as u32).checked_shl(window_scale as u32) {
            Some(shift) => shift,
            None => anyhow::bail!("incorrect receive window"),
        };

    let ((server_fd, addr), client_fd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut ctx, &mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    let bufsize: u32 = 64;
    let buf: DemiBuffer = cook_buffer(bufsize as usize, None);

    for i in 0..((max_window_size + 1) / bufsize) {
        send_recv(
            &mut ctx,
            &mut now,
            &mut server,
            &mut client,
            server_fd,
            client_fd,
            max_window_size as u16,
            SeqNumber::from(1 + i * bufsize),
            buf.clone(),
        )?;
    }

    Ok(())
}

//=============================================================================

#[test]
pub fn test_send_recv_round_loop() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine = test_helpers::new_bob2(now);
    let mut client: Engine = test_helpers::new_alice2(now);
    let window_scale: u8 = client.rt.tcp_config.get_window_scale();
    let max_window_size: u32 =
        match (client.rt.tcp_config.get_receive_window_size() as u32).checked_shl(window_scale as u32) {
            Some(shift) => shift,
            None => anyhow::bail!("incorrect receive window"),
        };
    let ((server_fd, addr), client_fd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut ctx, &mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    let bufsize: u32 = 64;
    let buf: DemiBuffer = cook_buffer(bufsize as usize, None);

    for i in 0..((max_window_size + 1) / bufsize) {
        send_recv_round(
            &mut ctx,
            &mut now,
            &mut server,
            &mut client,
            server_fd,
            client_fd,
            max_window_size as u16,
            SeqNumber::from(1 + i * bufsize),
            buf.clone(),
        )?;
    }

    Ok(())
}

//=============================================================================

/// Tests one way communication, with some random transmission delay. This
/// should force the receiving peer to send pure ACKs to the sender, as well as
/// the sender side to trigger the RTO calculation logic.
#[test]
pub fn test_send_recv_with_delay() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine = test_helpers::new_bob2(now);
    let mut client: Engine = test_helpers::new_alice2(now);
    let window_scale: u8 = client.rt.tcp_config.get_window_scale();
    let max_window_size: u32 =
        match (client.rt.tcp_config.get_receive_window_size() as u32).checked_shl(window_scale as u32) {
            Some(shift) => shift,
            None => anyhow::bail!("incorrect receive window"),
        };

    let ((server_fd, addr), client_fd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut ctx, &mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    let bufsize: u32 = 64;
    let buf: DemiBuffer = cook_buffer(bufsize as usize, None);
    let mut recv_seq_no: SeqNumber = SeqNumber::from(1);
    let mut seq_no: SeqNumber = SeqNumber::from(1);
    let mut inflight = VecDeque::<DemiBuffer>::new();

    for _ in 0..((max_window_size + 1) / bufsize) {
        // Push data.
        let (bytes, _): (DemiBuffer, usize) = send_data(
            &mut ctx,
            &mut now,
            &mut server,
            &mut client,
            client_fd,
            max_window_size as u16,
            seq_no,
            None,
            buf.clone(),
        )?;

        seq_no = seq_no + SeqNumber::from(bufsize);

        inflight.push_back(bytes);

        // Pop data oftentimes.
        if rand::random() {
            if let Some(bytes) = inflight.pop_front() {
                recv_data(&mut ctx, &mut server, &mut client, server_fd, bytes.clone())?;
                recv_seq_no = recv_seq_no + SeqNumber::from(bufsize as u32);
            }
        }

        // Pop pure ACK
        recv_pure_ack(&mut now, &mut server, &mut client, recv_seq_no)?;
    }

    // Pop inflight packets.
    while let Some(bytes) = inflight.pop_front() {
        // Pop data.
        recv_data(&mut ctx, &mut server, &mut client, server_fd, bytes.clone())?;
        recv_seq_no = recv_seq_no + SeqNumber::from(bufsize as u32);

        // Recv pure ack (should also account for piggybacked ack).
        // FIXME: https://github.com/demikernel/demikernel/issues/680
        recv_pure_ack(&mut now, &mut server, &mut client, recv_seq_no)?;
    }

    Ok(())
}

//=============================================================================

#[test]
fn test_connect_disconnect() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine = test_helpers::new_bob2(now);
    let mut client: Engine = test_helpers::new_alice2(now);

    let ((server_fd, addr), client_fd): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut ctx, &mut now, &mut server, &mut client, listen_port, listen_addr)?;
    crate::ensure_eq!(addr.ip(), &test_helpers::ALICE_IPV4);

    connection_hangup(&mut ctx, &mut now, &mut server, &mut client, server_fd, client_fd)?;

    Ok(())
}
