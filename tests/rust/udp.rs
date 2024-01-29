// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod common;

//==============================================================================
// Imports
//==============================================================================

use ::anyhow::Result;
use ::demikernel::runtime::{
    memory::{
        DemiBuffer,
        MemoryRuntime,
    },
    OperationResult,
    QDesc,
    QToken,
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
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

/// A default amount of time to wait on an operation to complete. This was chosen arbitrarily to be high enough to
/// ensure most OS operations will complete.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

use ::socket2::{
    Domain,
    Protocol,
    Type,
};
use std::{
    net::SocketAddr,
    thread::{
        self,
        JoinHandle,
    },
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Connect
//==============================================================================

/// Opens and closes a socket using a non-ephemeral port.
fn do_udp_setup(libos: &mut DummyLibOS) -> Result<()> {
    let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_BASE);
    let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockfd, local) {
        Ok(_) => (),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("bind() failed: {:?}", e)
        },
    };

    match libos.async_close(sockfd) {
        Ok(qt) => {
            safe_wait(libos, qt)?;
            Ok(())
        },
        Err(e) => anyhow::bail!("close() failed: {:?}", e),
    }
}

/// Opens and closes a socket using an ephemeral port.
fn do_udp_setup_ephemeral(libos: &mut DummyLibOS) -> Result<()> {
    const PORT_EPHEMERAL_BASE: u16 = 49152;
    let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_EPHEMERAL_BASE);
    let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockfd, local) {
        Ok(_) => (),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("bind() failed: {:?}", e)
        },
    };

    match libos.async_close(sockfd) {
        Ok(qt) => {
            safe_wait(libos, qt)?;
            Ok(())
        },
        Err(e) => anyhow::bail!("close() failed: {:?}", e),
    }
}

/// Opens and closes a socket using wildcard ephemeral port.
fn do_udp_setup_wildcard_ephemeral(libos: &mut DummyLibOS) -> Result<()> {
    let local: SocketAddr = SocketAddr::new(ALICE_IP, 0);
    let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockfd, local) {
        Ok(_) => (),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("bind() failed: {:?}", e)
        },
    };

    match libos.async_close(sockfd) {
        Ok(qt) => {
            safe_wait(libos, qt)?;
            Ok(())
        },
        Err(e) => anyhow::bail!("close() failed: {:?}", e),
    }
}

/// Tests if a socket can be successfully setup.
#[test]
fn udp_setup() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp()) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
    };

    do_udp_setup(&mut libos)?;
    do_udp_setup_ephemeral(&mut libos)?;
    do_udp_setup_wildcard_ephemeral(&mut libos)?;

    Ok(())
}

/// Tests if a connection can be successfully established in loopback mode.
#[test]
fn udp_connect_loopback() -> Result<()> {
    let (tx, rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, tx, rx, arp()) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
    };

    let port: u16 = PORT_BASE;
    let local: SocketAddr = SocketAddr::new(ALICE_IP, port);

    // Open and close a connection.
    let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
        Ok(qd) => qd,
        Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
    };
    match libos.bind(sockfd, local) {
        Ok(_) => (),
        Err(e) => {
            // Close socket on error.
            // FIXME: https://github.com/demikernel/demikernel/issues/633
            anyhow::bail!("bind() failed: {:?}", e)
        },
    };

    match libos.async_close(sockfd) {
        Ok(qt) => {
            safe_wait(&mut libos, qt)?;
            Ok(())
        },
        Err(e) => anyhow::bail!("close() failed: {:?}", e),
    }
}

//==============================================================================
// Push
//==============================================================================

/// Tests if data can be successfully pushed/popped form a local endpoint to
/// itself.
#[test]
fn udp_push_remote() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let bob_port: u16 = PORT_BASE;
    let bob_addr: SocketAddr = SocketAddr::new(BOB_IP, bob_port);
    let alice_port: u16 = PORT_BASE;
    let alice_addr: SocketAddr = SocketAddr::new(ALICE_IP, alice_port);

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        // Open connection.
        let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };
        match libos.bind(sockfd, alice_addr) {
            Ok(_) => (),
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("bind() failed: {:?}", e)
            },
        }

        // Cook some data and push.
        let bytes = libos.cook_data(32)?;
        let qt: QToken = match libos.pushto(sockfd, &bytes, bob_addr) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("wait on push() failed")
            },
        }

        // Pop data.
        let qt: QToken = match libos.pop(sockfd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop()) failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("wait on pop() failed")
            },
        }

        // Close connection.
        match libos.async_close(sockfd) {
            Ok(qt) => {
                safe_wait(&mut libos, qt)?;
                Ok(())
            },
            Err(e) => anyhow::bail!("close() failed: {:?}", e),
        }
    });

    let bob: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(BOB_MAC, BOB_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        // Open connection.
        let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };
        match libos.bind(sockfd, bob_addr) {
            Ok(_) => (),
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("bind() failed: {:?}", e)
            },
        };

        // Pop data.
        let qt: QToken = match libos.pop(sockfd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let bytes: DemiBuffer = match qr {
            OperationResult::Pop(_, bytes) => bytes,
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("wait on pop() failed")
            },
        };

        // Push data.
        let buf = libos.get_transport().into_sgarray(bytes)?;
        let qt: QToken = match libos.pushto(sockfd, &buf, alice_addr) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("wait on push() failed")
            },
        }

        // Close connection.
        match libos.async_close(sockfd) {
            Ok(qt) => {
                safe_wait(&mut libos, qt)?;
                Ok(())
            },
            Err(e) => anyhow::bail!("close() failed: {:?}", e),
        }
    });

    // It is safe to use unwrap here because there should not be any reason that we can't join the thread and if there
    // is, there is nothing to clean up here on the main thread.
    alice.join().unwrap()?;
    bob.join().unwrap()?;

    Ok(())
}

/// Tests if data can be successfully pushed/popped in loopback mode.
#[test]
fn udp_loopback() -> Result<()> {
    let (alice_tx, alice_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();
    let (bob_tx, bob_rx): (Sender<DemiBuffer>, Receiver<DemiBuffer>) = crossbeam_channel::unbounded();

    let bob_port: u16 = PORT_BASE;
    let bob_addr: SocketAddr = SocketAddr::new(ALICE_IP, bob_port);
    let alice_port: u16 = PORT_BASE;
    let alice_addr: SocketAddr = SocketAddr::new(ALICE_IP, alice_port);

    let alice: JoinHandle<Result<()>> = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, alice_tx, bob_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        // Open connection.
        let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };
        match libos.bind(sockfd, alice_addr) {
            Ok(_) => (),
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("bind() failed: {:?}", e)
            },
        };
        // Cook some data and push.
        let bytes = libos.cook_data(32)?;
        let qt: QToken = match libos.pushto(sockfd, &bytes, bob_addr) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("wait on push() failed")
            },
        }

        // Pop data.
        let qt: QToken = match libos.pop(sockfd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Pop(_, _) => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("wait on pop() failed")
            },
        }

        // Close connection.
        match libos.async_close(sockfd) {
            Ok(qt) => {
                safe_wait(&mut libos, qt)?;
                Ok(())
            },
            Err(e) => anyhow::bail!("close() failed: {:?}", e),
        }
    });

    let bob = thread::spawn(move || {
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_MAC, ALICE_IPV4, bob_tx, alice_rx, arp()) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        // Open connection.
        let sockfd: QDesc = match libos.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };
        match libos.bind(sockfd, bob_addr) {
            Ok(_) => (),
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("bind() failed: {:?}", e)
            },
        };
        // Pop data.
        let qt: QToken = match libos.pop(sockfd, None) {
            Ok(qt) => qt,
            Err(e) => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() failed: {:?}", e)
            },
        };
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        let bytes: DemiBuffer = match qr {
            OperationResult::Pop(_, bytes) => bytes,
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("pop() failed")
            },
        };

        // Push data.
        let buf = libos.get_transport().into_sgarray(bytes)?;
        let qt: QToken = libos.pushto(sockfd, &buf, alice_addr).unwrap();
        let (_, qr): (QDesc, OperationResult) = safe_wait(&mut libos, qt)?;
        match qr {
            OperationResult::Push => (),
            _ => {
                // Close socket on error.
                // FIXME: https://github.com/demikernel/demikernel/issues/633
                anyhow::bail!("push() failed")
            },
        }

        // Close connection.
        match libos.async_close(sockfd) {
            Ok(qt) => {
                safe_wait(&mut libos, qt)?;
                Ok(())
            },
            Err(e) => anyhow::bail!("close() failed: {:?}", e),
        }
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

/// Safe call to `wait2()`.
fn safe_wait(libos: &mut DummyLibOS, qt: QToken) -> Result<(QDesc, OperationResult)> {
    // First check if the task has already completed.
    if let Some(result) = libos.get_runtime().get_completed_task(&qt) {
        return Ok(result);
    }

    // Otherwise, actually run the scheduler.
    // Put the QToken into a single element array.
    let qt_array: [QToken; 1] = [qt];
    let start: Instant = Instant::now();

    // Call run_any() until the task finishes.
    while Instant::now() <= start + DEFAULT_TIMEOUT {
        // Run for one quanta and if one of our queue tokens completed, then return.
        if let Some((offset, qd, qr)) = libos.get_runtime().run_any(&qt_array) {
            debug_assert_eq!(offset, 0);
            return Ok((qd, qr));
        }
    }

    anyhow::bail!("wait timed out")
}
