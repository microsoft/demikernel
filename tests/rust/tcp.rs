// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod common;

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::{
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        OperationResult,
        QDesc,
        QToken,
    },
};
use ::socket2::{
    Domain,
    Protocol,
    Type,
};
use common::{
    arp,
    libos::*,
    ALICE_IP,
    ALICE_IPV4,
    ALICE_MAC,
    BOB_IP,
    BOB_IPV4,
    BOB_MAC,
    PORT_BASE,
};
use crossbeam_channel::{
    self,
    Receiver,
    Sender,
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

/// A default amount of time to wait on an operation to complete. This was chosen arbitrarily to be high enough to
/// ensure most OS operations will complete.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

use std::{
    net::{
        IpAddr,
        Ipv4Addr,
        Ipv6Addr,
        SocketAddr,
        SocketAddrV6,
    },
    thread::{
        self,
        JoinHandle,
    },
    time::Duration,
};
#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

//======================================================================================================================
// Open/Close Passive Socket
//======================================================================================================================

/// Opens and closes a passive socket using a non-ephemeral port.
fn do_passive_connection_setup(mut libos: &mut DummyLibOS) -> Result<()> {
    let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_BASE);
    let sockqd: QDesc = safe_socket(&mut libos)?;
    safe_bind(&mut libos, sockqd, local)?;
    safe_listen(&mut libos, sockqd)?;
    safe_close_passive(&mut libos, sockqd)?;

    Ok(())
}

/// Opens and closes a passive socket using an ephemeral port.
fn do_passive_connection_setup_ephemeral(mut libos: &mut DummyLibOS) -> Result<()> {
    pub const PORT_EPHEMERAL_BASE: u16 = 49152;
    let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_EPHEMERAL_BASE);
    let sockqd: QDesc = safe_socket(&mut libos)?;
    safe_bind(&mut libos, sockqd, local)?;
    safe_listen(&mut libos, sockqd)?;
    safe_close_passive(&mut libos, sockqd)?;

    Ok(())
}

/// Tests if a passive socket may be successfully opened and closed.
#[test]
fn tcp_connection_setup() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp())?;

    do_passive_connection_setup(&mut libos)?;
    do_passive_connection_setup_ephemeral(&mut libos)?;

    Ok(())
}

//======================================================================================================================
// Establish Connection
//======================================================================================================================

/// Tests if connection may be successfully established by an unbound active socket.
#[test]
fn tcp_establish_connection_unbound() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_BASE);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;

        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let remote: SocketAddr = SocketAddr::new(ALICE_IP, PORT_BASE);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        Ok(())
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

/// Tests if connection may be successfully established by a bound active socket.
#[test]
fn tcp_establish_connection_bound() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_BASE);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;

        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let local: SocketAddr = SocketAddr::new(BOB_IP, PORT_BASE);
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, PORT_BASE);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        Ok(())
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

//======================================================================================================================
// Push
//======================================================================================================================

/// Tests if data can be pushed.
#[test]
fn tcp_push_remote() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let local: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Pop data.
        let qt: QToken = safe_pop(&mut libos, qd)?;
        let (qd, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() has has failed {:?}", qr)
            },
        }

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Cook some data and push.
        let buf = libos.cook_data(32)?;
        let qt: QToken = safe_push(&mut libos, sockqd, buf)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() has failed")
            },
        }

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        Ok(())
    });
    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

//======================================================================================================================
// Bad Socket
//======================================================================================================================

/// Tests for bad socket creation.
#[test]
fn tcp_bad_socket() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp()) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
    };

    #[cfg(target_os = "linux")]
    let domains: Vec<libc::c_int> = vec![
        libc::AF_ALG,
        libc::AF_APPLETALK,
        libc::AF_ASH,
        libc::AF_ATMPVC,
        libc::AF_ATMSVC,
        libc::AF_AX25,
        libc::AF_BLUETOOTH,
        libc::AF_BRIDGE,
        libc::AF_CAIF,
        libc::AF_CAN,
        libc::AF_DECnet,
        libc::AF_ECONET,
        libc::AF_IB,
        libc::AF_IEEE802154,
        // libc::AF_INET,
        libc::AF_INET6,
        libc::AF_IPX,
        libc::AF_IRDA,
        libc::AF_ISDN,
        libc::AF_IUCV,
        libc::AF_KEY,
        libc::AF_LLC,
        libc::AF_LOCAL,
        libc::AF_MPLS,
        libc::AF_NETBEUI,
        libc::AF_NETLINK,
        libc::AF_NETROM,
        libc::AF_NFC,
        libc::AF_PACKET,
        libc::AF_PHONET,
        libc::AF_PPPOX,
        libc::AF_RDS,
        libc::AF_ROSE,
        libc::AF_ROUTE,
        libc::AF_RXRPC,
        libc::AF_SECURITY,
        libc::AF_SNA,
        libc::AF_TIPC,
        libc::AF_UNIX,
        libc::AF_UNSPEC,
        libc::AF_VSOCK,
        libc::AF_WANPIPE,
        libc::AF_X25,
        libc::AF_XDP,
    ];

    #[cfg(target_os = "windows")]
    let domains: Vec<libc::c_int> = vec![
        WinSock::AF_APPLETALK as i32,
        WinSock::AF_DECnet as i32,
        // WinSock::AF_INET as i32,
        WinSock::AF_INET6.0 as i32,
        WinSock::AF_IPX as i32,
        WinSock::AF_IRDA as i32,
        WinSock::AF_SNA as i32,
        WinSock::AF_UNIX as i32,
        WinSock::AF_UNSPEC.0 as i32,
    ];

    #[cfg(target_os = "linux")]
    let socket_types: Vec<libc::c_int> = vec![
        libc::SOCK_DCCP,
        // libc::SOCK_DGRAM,
        libc::SOCK_PACKET,
        libc::SOCK_RAW,
        libc::SOCK_RDM,
        libc::SOCK_SEQPACKET,
        // libc::SOCK_STREAM,
    ];

    #[cfg(target_os = "windows")]
    let socket_types: Vec<libc::c_int> = vec![
        WinSock::SOCK_RAW.0 as i32,
        WinSock::SOCK_RDM.0 as i32,
        WinSock::SOCK_SEQPACKET.0 as i32,
    ];

    // Invalid domain.
    for d in domains {
        match libos.socket(d.into(), Type::STREAM, 0.into()) {
            Err(e) if e.errno == libc::ENOTSUP => (),
            _ => anyhow::bail!("invalid call to socket() should fail with ENOTSUP"),
        };
    }

    // Invalid socket tpe.
    for t in socket_types {
        match libos.socket(AF_INET.into(), t.into(), 0.into()) {
            Err(e) if e.errno == libc::ENOTSUP => (),
            _ => anyhow::bail!("invalid call to socket() should fail with ENOTSUP"),
        };
    }

    Ok(())
}

//======================================================================================================================
// Bad Bind
//======================================================================================================================

/// Tests bad calls for `bind()`.
#[test]
fn tcp_bad_bind() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp()) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
    };

    let port: u16 = PORT_BASE;
    let port2: u16 = PORT_BASE + 1;
    let local: SocketAddr = SocketAddr::new(ALICE_IP, port);
    let local2: SocketAddr = SocketAddr::new(ALICE_IP, port2);
    let localv6: SocketAddr = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, port, 0, 0));

    // IPv6 unsupported
    let sockqd: QDesc = safe_socket(&mut libos)?;
    match libos.bind(sockqd, localv6) {
        Err(e) if e.errno == libc::EINVAL => (),
        _ => anyhow::bail!("invalid call to bind should fail with EINVAL"),
    }

    // Can't re-bind an address
    let sockqd2: QDesc = safe_socket(&mut libos)?;
    safe_bind(&mut libos, sockqd, local)?;
    match libos.bind(sockqd2, local) {
        Err(e) if e.errno == libc::EADDRINUSE => (),
        _ => anyhow::bail!("invalid call to bind should fail with EADDRINUSE"),
    };

    // Can't bind a bound socket
    match libos.bind(sockqd, local2) {
        Err(e) if e.errno == libc::EINVAL => (),
        _ => anyhow::bail!("invalid call to bind should fail with EINVAL"),
    };

    Ok(())
}

//======================================================================================================================
// Bad Listen
//======================================================================================================================

/// Tests bad calls for `listen()`.
#[test]
fn tcp_bad_listen() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp()) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
    };

    let port: u16 = PORT_BASE;
    let port2: u16 = PORT_BASE + 1;
    let local: SocketAddr = SocketAddr::new(ALICE_IP, port);
    let local2: SocketAddr = SocketAddr::new(ALICE_IP, port2);

    // Invalid queue descriptor.
    match libos.listen(QDesc::from(0), 8) {
        Err(e) if e.errno == libc::EBADF => (),
        _ => {
            // Close socket if not error because this test cannot continue.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("invalid call to listen() should fail with EBADF")
        },
    };

    // Invalid backlog length
    let sockqd: QDesc = safe_socket(&mut libos)?;
    safe_bind(&mut libos, sockqd, local)?;
    match libos.listen(sockqd, 0) {
        Err(e) if e.errno == libc::EINVAL => (),
        _ => {
            // Close socket if not error because this test cannot continue.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("invalid call to listen() should fail with EINVAL")
        },
    };
    safe_close_active(&mut libos, sockqd)?;

    // Listen on an already listening socket.
    // TODO: use a single IP and port when we properly clean up bound addresses on close.
    let sockqd: QDesc = safe_socket(&mut libos)?;
    safe_bind(&mut libos, sockqd, local2)?;
    safe_listen(&mut libos, sockqd)?;
    match libos.listen(sockqd, 16) {
        Err(e) if e.errno == libc::EADDRINUSE => (),
        _ => {
            // Close socket if not error because this test cannot continue.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("listen() called on an already listening socket should fail with EADDRINUSE")
        },
    };
    safe_close_passive(&mut libos, sockqd)?;

    // TODO: Add unit test for "Listen on an in-use address/port pair." (see issue #178).

    // Listen on unbound socket.
    let sockqd: QDesc = safe_socket(&mut libos)?;
    match libos.listen(sockqd, 16) {
        Err(e) if e.errno == libc::EDESTADDRREQ => (),
        Err(e) => {
            // Close socket if not the correct error because this test cannot continue.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("listen() to unbound address should fail with EDESTADDRREQ {:?}", e)
        },
        _ => {
            // Close socket if not error because this test cannot continue.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("should fail")
        },
    };
    safe_close_active(&mut libos, sockqd)?;

    Ok(())
}

//======================================================================================================================
// Bad Accept
//======================================================================================================================

/// Tests bad calls for `accept()`.
#[test]
fn tcp_bad_accept() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp()) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
    };

    // Invalid queue descriptor.
    match libos.accept(QDesc::from(0)) {
        Err(e) if e.errno == libc::EBADF => (),
        _ => {
            // Close socket if we somehow got a socket back?
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("invalid call to accept() should fail with EBADF")
        },
    };

    Ok(())
}

//======================================================================================================================
// Bad Connect
//======================================================================================================================

/// Tests if data can be successfully established.
#[test]
fn tcp_bad_connect() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };
        let port: u16 = PORT_BASE;
        let local: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Bad queue descriptor.
        match libos.connect(QDesc::from(0), remote) {
            Err(e) if e.errno == libc::EBADF => (),
            _ => {
                // Close socket if not error because this test cannot continue.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("invalid call to connect() should fail with EBADF")
            },
        };

        // Bad endpoint.
        let bad_remote: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, bad_remote)?;
        match libos.wait(qt, Some(Duration::from_millis(10))) {
            Err(e) if e.errno == libc::ETIMEDOUT => (),
            Ok((_, OperationResult::Connect)) => {
                // Close socket if not error because this test cannot continue.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() should have timed out")
            },
            _ => anyhow::bail!("connect() should have timed out"),
        }

        // Close connection.
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, port);
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket if not error because this test cannot continue.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        Ok(())
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

//======================================================================================================================
// Bad Close
//======================================================================================================================

/// Tests if bad calls t `close()`.
#[test]
fn tcp_bad_close() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let local: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Close bad queue descriptor.
        match libos.async_close(QDesc::from(2)) {
            Ok(_) => anyhow::bail!("close() invalid file descriptir should fail"),
            Err(_) => (),
        };

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        // Double close queue descriptor.
        match libos.async_close(qd) {
            Ok(_) => anyhow::bail!("double close() should fail"),
            Err(_) => (),
        };

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Close bad queue descriptor.
        match libos.async_close(QDesc::from(2)) {
            // Should not be able to close bad queue descriptor.
            Ok(_) => anyhow::bail!("close() invalid queue descriptor should fail"),
            Err(_) => (),
        };

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        // Double close queue descriptor.
        match libos.async_close(sockqd) {
            // Should not be able to double close.
            Ok(_) => anyhow::bail!("double close() should fail"),
            Err(_) => (),
        };

        Ok(())
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

//======================================================================================================================
// Bad Push
//======================================================================================================================

/// Tests bad calls to `push()`.
#[test]
fn tcp_bad_push() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let local: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Pop data.
        let qt: QToken = safe_pop(&mut libos, qd)?;
        let (qd, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() has has failed {:?}", qr)
            },
        }

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Cook some data and push to a bad socket.
        let bytes = libos.cook_data(32)?;
        match libos.push(QDesc::from(2), &bytes) {
            Ok(_) => {
                // Close socket if not error because this test cannot continue.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push2() to bad socket should fail.")
            },
            Err(_) => (),
        };

        // Push bad data to socket.
        let zero_bytes: [u8; 0] = [];
        let buf: DemiBuffer = match DemiBuffer::from_slice(&zero_bytes) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!("(zero-byte) slice should fit in a DemiBuffer: {:?}", e),
        };
        let data: demi_sgarray_t = libos.get_transport().into_sgarray(buf)?;

        match libos.push(sockqd, &data) {
            Ok(_) =>
            // Close socket if not error because this test cannot continue.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            {
                anyhow::bail!("push2() zero-length slice should fail.")
            },
            Err(_) => (),
        };

        // Push data.
        let bytes = libos.cook_data(32)?;
        let qt: QToken = safe_push(&mut libos, sockqd, bytes)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() has failed")
            },
        }

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        Ok(())
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

//======================================================================================================================
// Bad Pop
//======================================================================================================================

/// Tests bad calls to `pop()`.
#[test]
fn tcp_bad_pop() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let local: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        safe_bind(&mut libos, sockqd, local)?;
        safe_listen(&mut libos, sockqd)?;
        let qt: QToken = safe_accept(&mut libos, sockqd)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let qd: QDesc = match qr {
            OperationResult::Accept((qd, addr)) if addr.ip() == &BOB_IPV4 => qd,
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("accept() has failed")
            },
        };

        // Pop from bad socket.
        match libos.pop(QDesc::from(2), None) {
            Ok(_) => {
                // Close socket if not error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() form bad socket should fail.")
            },
            Err(_) => (),
        };

        // Pop data.
        let qt: QToken = safe_pop(&mut libos, qd)?;
        let (qd, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() has has failed {:?}", qr)
            },
        }

        // Close connection.
        safe_close_active(&mut libos, qd)?;
        safe_close_passive(&mut libos, sockqd)?;

        Ok(())
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let port: u16 = PORT_BASE;
        let remote: SocketAddr = SocketAddr::new(ALICE_IP, port);

        // Open connection.
        let sockqd: QDesc = safe_socket(&mut libos)?;
        let qt: QToken = safe_connect(&mut libos, sockqd, remote)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Connect => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("connect() has failed")
            },
        }

        // Cook some data and push data.
        let bytes = libos.cook_data(32)?;
        let qt: QToken = safe_push(&mut libos, sockqd, bytes)?;
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket if error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() has failed")
            },
        }

        // Close connection.
        safe_close_active(&mut libos, sockqd)?;

        Ok(())
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Safe call to `socket()`.
fn safe_socket(libos: &mut DummyLibOS) -> Result<QDesc> {
    match libos.socket(Domain::IPV4, Type::STREAM, Protocol::TCP) {
        Ok(sockqd) => Ok(sockqd),
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    }
}

/// Safe call to `connect()`.
fn safe_connect(libos: &mut DummyLibOS, sockqd: QDesc, remote: SocketAddr) -> Result<QToken> {
    match libos.connect(sockqd, remote) {
        Ok(qt) => Ok(qt),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("failed to establish connection: {:?}", e)
        },
    }
}

/// Safe call to `bind()`.
fn safe_bind(libos: &mut DummyLibOS, sockqd: QDesc, local: SocketAddr) -> Result<()> {
    match libos.bind(sockqd, local) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("bind() failed: {:?}", e)
        },
    }
}

/// Safe call to `listen()`.
fn safe_listen(libos: &mut DummyLibOS, sockqd: QDesc) -> Result<()> {
    match libos.listen(sockqd, 8) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("listen() failed: {:?}", e)
        },
    }
}

/// Safe call to `accept()`.
fn safe_accept(libos: &mut DummyLibOS, sockqd: QDesc) -> Result<QToken> {
    match libos.accept(sockqd) {
        Ok(qt) => Ok(qt),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("accept() failed: {:?}", e)
        },
    }
}

/// Safe call to `pop()`.
fn safe_pop(libos: &mut DummyLibOS, qd: QDesc) -> Result<QToken> {
    match libos.pop(qd, None) {
        Ok(qt) => Ok(qt),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("pop() failed: {:?}", e)
        },
    }
}

/// Safe call to `push2()`
fn safe_push(libos: &mut DummyLibOS, sockqd: QDesc, bytes: demi_sgarray_t) -> Result<QToken> {
    match libos.push(sockqd, &bytes) {
        Ok(qt) => Ok(qt),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("failed to push: {:?}", e)
        },
    }
}

/// Safe call to `wait2()`.
fn safe_wait(libos: &mut DummyLibOS, qt: QToken) -> Result<(QDesc, OperationResult)> {
    // Set this to something reasonably high because it should eventually complete.
    match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
        Ok(result) => Ok(result),
        Err(e) => anyhow::bail!("wait failed: {:?}", e),
    }
}

/// Safe call to `close()` on passive socket.
fn safe_close_passive(libos: &mut DummyLibOS, sockqd: QDesc) -> Result<()> {
    let qt = libos.async_close(sockqd)?;
    match safe_wait(libos, qt)? {
        (_, OperationResult::Failed(_)) => Ok(()),
        _ => {
            anyhow::bail!("close() on listening socket should have failed (this is a known bug)")
        },
    }
}

/// Safe call to `close()` on active socket.
fn safe_close_active(libos: &mut DummyLibOS, qd: QDesc) -> Result<(QDesc, OperationResult)> {
    match libos.async_close(qd) {
        Ok(qt) => safe_wait(libos, qt),
        Err(_) => anyhow::bail!("close() on active socket has failed"),
    }
}
