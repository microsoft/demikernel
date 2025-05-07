// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod common;

mod test {

    //==========================================================================
    // Imports
    //==========================================================================

    use crate::common::{libos::*, ALICE_CONFIG_PATH, ALICE_IP, BOB_CONFIG_PATH, BOB_IP, PORT_NUMBER};
    use ::anyhow::Result;
    use ::crossbeam_channel::{Receiver, Sender};
    use ::demikernel::runtime::{
        memory::{into_sgarray, DemiBuffer},
        OperationResult, QDesc, QToken,
    };

    /// A default amount of time to wait on an operation to complete. This was chosen arbitrarily to be high enough to
    /// ensure most OS operations will complete.
    const TIMEOUT_MILLISECONDS: Duration = Duration::from_millis(100);

    use ::socket2::{Domain, Protocol, Type};
    use std::{
        net::SocketAddr,
        sync::{Arc, Barrier},
        thread::{self, JoinHandle},
        time::Duration,
    };

    //==============================================================================
    // Connect
    //==============================================================================

    /// Opens and closes a socket using a non-ephemeral port.
    fn do_udp_setup(libos: &mut DummyLibOS) -> Result<()> {
        let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_NUMBER);
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
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_CONFIG_PATH, tx, rx) {
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
        let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_CONFIG_PATH, tx, rx) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
        };

        let local: SocketAddr = SocketAddr::new(ALICE_IP, PORT_NUMBER);

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

        let bob_addr: SocketAddr = SocketAddr::new(BOB_IP, PORT_NUMBER);
        let alice_addr: SocketAddr = SocketAddr::new(ALICE_IP, PORT_NUMBER);

        let bob_barrier: Arc<Barrier> = Arc::new(Barrier::new(2));
        let alice_barrier: Arc<Barrier> = bob_barrier.clone();

        let alice: JoinHandle<Result<()>> = thread::Builder::new().name(format!("alice")).spawn(move || {
            let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_CONFIG_PATH, alice_tx, bob_rx) {
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

            let bytes = libos.prepare_dummy_buffer(32)?;
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
            alice_barrier.wait();
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
                    alice_barrier.wait();
                    Ok(())
                },
                Err(e) => anyhow::bail!("close() failed: {:?}", e),
            }
        })?;

        let bob: JoinHandle<Result<()>> = thread::Builder::new().name(format!("bob")).spawn(move || {
            let mut libos: DummyLibOS = match DummyLibOS::new(BOB_CONFIG_PATH, bob_tx, alice_rx) {
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
            let buf = into_sgarray(bytes)?;
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

            bob_barrier.wait();
            // Close connection.
            match libos.async_close(sockfd) {
                Ok(qt) => {
                    safe_wait(&mut libos, qt)?;
                    bob_barrier.wait();
                    Ok(())
                },
                Err(e) => anyhow::bail!("close() failed: {:?}", e),
            }
        })?;

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

        let bob_addr: SocketAddr = SocketAddr::new(ALICE_IP, PORT_NUMBER);
        let alice_addr: SocketAddr = SocketAddr::new(ALICE_IP, PORT_NUMBER);

        let bob_barrier: Arc<Barrier> = Arc::new(Barrier::new(2));
        let alice_barrier: Arc<Barrier> = bob_barrier.clone();

        let alice: JoinHandle<Result<()>> = thread::Builder::new().name(format!("alice")).spawn(move || {
            let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_CONFIG_PATH, alice_tx, bob_rx) {
                Ok(libos) => libos,
                Err(e) => anyhow::bail!("Could not create inetstack: {:?}", e),
            };

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

            let bytes = libos.prepare_dummy_buffer(32)?;
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

            alice_barrier.wait();
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

            match libos.async_close(sockfd) {
                Ok(qt) => {
                    safe_wait(&mut libos, qt)?;
                    alice_barrier.wait();
                    Ok(())
                },
                Err(e) => anyhow::bail!("close() failed: {:?}", e),
            }
        })?;

        let bob = thread::Builder::new().name(format!("bob")).spawn(move || {
            let mut libos: DummyLibOS = match DummyLibOS::new(ALICE_CONFIG_PATH, bob_tx, alice_rx) {
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
            let buf = into_sgarray(bytes)?;
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
            bob_barrier.wait();

            // Close connection.
            match libos.async_close(sockfd) {
                Ok(qt) => {
                    safe_wait(&mut libos, qt)?;
                    bob_barrier.wait();
                    Ok(())
                },
                Err(e) => anyhow::bail!("close() failed: {:?}", e),
            }
        })?;

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
        match libos.wait(qt, TIMEOUT_MILLISECONDS) {
            Ok(result) => Ok(result),
            Err(_) => anyhow::bail!("wait timed out"),
        }
    }
}
