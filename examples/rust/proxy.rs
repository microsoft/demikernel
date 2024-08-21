// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(never_type)]
#![feature(extract_if)]
#![feature(hash_extract_if)]

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
use std::net::SocketAddr;
use ::std::{
    collections::HashMap,
    env,
    slice,
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};

//======================================================================================================================
// server()
//======================================================================================================================

struct TcpProxy {
    /// LibOS that handles incoming flow.
    catnap: LibOS,
    /// LibOS that handles outgoing flow.
    catloop: LibOS,
    /// Number of clients that are currently connected.
    nclients: usize,
    /// Remote socket address.
    remote_addr: SocketAddr,
    /// Socket for accepting incoming connections.
    local_socket: QDesc,
    /// Queue descriptors of incoming connections.
    incoming_qds: HashMap<QDesc, bool>,
    /// Maps a queue descriptor of an incoming connection to its respective outgoing connection.
    incoming_qds_map: HashMap<QDesc, QDesc>,
    /// Incoming operations that are pending.
    incoming_qts: Vec<QToken>,
    /// Maps a pending incoming operation to its respective queue descriptor.
    incoming_qts_map: HashMap<QToken, QDesc>,
    /// Queue descriptors of outgoing connections.
    outgoing_qds: HashMap<QDesc, bool>,
    /// Maps a queue descriptor of an outgoing connection to its respective incoming connection.
    outgoing_qds_map: HashMap<QDesc, QDesc>,
    /// Outgoing operations that are pending.
    outgoing_qts: Vec<QToken>,
    /// Maps a pending outgoing operation to its respective queue descriptor.
    outgoing_qts_map: HashMap<QToken, QDesc>,
}

impl TcpProxy {
    /// Expected length for the array of pending incoming operations.
    /// It controls the pre-allocated size of the array.
    /// Change this value accordingly so as to avoid allocations on the datapath.
    const INCOMING_LENGTH: usize = 1024;
    /// Expected length for the array of pending outgoing operations.
    /// It controls the pre-allocated size of the array.
    /// Change this value accordingly so as to avoid allocations on the datapath.
    const OUTGOING_LENGTH: usize = 1024;

    /// Instantiates a TCP proxy that accepts incoming flows from `local_addr` and forwards them to `remote_addr`.
    pub fn new(local_addr: SocketAddr, remote_addr: SocketAddr) -> Result<Self> {
        // Retrieve LibOS name from environment variables.
        // eprintln!("TEST");
       let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        // eprintln!("libos_name: {:?}", libos_name);

        // Instantiate LibOS for handling incoming flows.
        let mut catnap: LibOS = match LibOS::new(libos_name, None) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos (error={:?})", e),
        };

        // Instantiate LibOS for handling outgoing flows.
        let catloop: LibOS = match LibOS::new(LibOSName::Catnap, None) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos (error={:?})", e),
        };

        // Setup local socket.
        let local_socket: QDesc = Self::setup_local_socket(&mut catnap, local_addr)?;

        Ok(Self {
            catnap,
            catloop,
            nclients: 0,
            remote_addr,
            local_socket,
            incoming_qts: Vec::with_capacity(Self::INCOMING_LENGTH),
            incoming_qts_map: HashMap::default(),
            outgoing_qts: Vec::with_capacity(Self::OUTGOING_LENGTH),
            outgoing_qts_map: HashMap::default(),
            incoming_qds: HashMap::default(),
            incoming_qds_map: HashMap::default(),
            outgoing_qds: HashMap::default(),
            outgoing_qds_map: (HashMap::default()),
        })
    }

    /// Runs the target TCP proxy.
    pub fn run(&mut self) -> Result<!> {
        // Time interval for dumping logs and statistics.
        // This was arbitrarily set, but keep in mind that too short intervals may negatively impact performance.
        let log_interval: Option<Duration> = Some(Duration::from_secs(1));
        // Time stamp when last log was dumped.
        let mut last_log: Instant = Instant::now();
        // Timeout for polling incoming operations.This was intentionally set to zero to force no waiting.
        let timeout_incoming: Option<Duration> = Some(Duration::from_secs(0));
        // Timeout for polling outgoing operations. This was intentionally set to zero to force no waiting.
        let timeout_outgoing: Option<Duration> = Some(Duration::from_secs(0));

        // Accept incoming connections.
        self.issue_accept()?;

        loop {
            // Dump statistics.
            if let Some(log_interval) = log_interval {
                if last_log.elapsed() > log_interval {
                    // println!("INFO: {:?} clients connected", self.nclients);
                    last_log = Instant::now();
                }
            }

            // Poll incoming flows.
            if let Some(qr) = self.poll_incoming(timeout_incoming) {
                // Parse operation result.
                match qr.qr_opcode {
                    demi_opcode_t::DEMI_OPC_ACCEPT => self.handle_incoming_accept(&qr)?,
                    demi_opcode_t::DEMI_OPC_POP => self.handle_incoming_pop(&qr),
                    demi_opcode_t::DEMI_OPC_PUSH => self.handle_incoming_push(&qr),
                    demi_opcode_t::DEMI_OPC_FAILED => {
                        // Check if this is an unrecoverable error.
                        if qr.qr_ret != libc::ECONNRESET as i64 {
                            anyhow::bail!("[1] operation failed {:?}", qr.qr_opcode)
                        }
                        println!("WARN: client reset connection");
                        let catnap_qd: QDesc = qr.qr_qd.into();
                        // It is safe to expect here because the queue descriptor must have been registered.
                        // All queue descriptors are registered when the connection is established.
                        let catloop_qd: QDesc = *self
                            .outgoing_qds_map
                            .get(&catnap_qd)
                            .expect("queue descriptor not registered");
                        self.close_client(catnap_qd, catloop_qd);
                    },
                    _ => unreachable!(),
                };
            }

            // Poll outgoing flows.
            if let Some(qr) = self.poll_outgoing(timeout_outgoing) {
                // Parse operation result.
                match qr.qr_opcode {
                    demi_opcode_t::DEMI_OPC_CONNECT => self.handle_outgoing_connect(&qr),
                    demi_opcode_t::DEMI_OPC_POP => self.handle_outgoing_pop(&qr),
                    demi_opcode_t::DEMI_OPC_PUSH => self.handle_outgoing_push(&qr),
                    demi_opcode_t::DEMI_OPC_FAILED => {
                        // Check if this is an unrecoverable error.
                        if qr.qr_ret != libc::ECONNRESET as i64 {
                            anyhow::bail!("[2] operation failed {:?}", qr.qr_opcode)
                        }
                        println!("WARN: server reset connection");
                        let catloop_socket: QDesc = qr.qr_qd.into();
                        // It is safe to expect() here because the queue descriptor must have been registered.
                        // All queue descriptors are registered when the connection is established.
                        let catnap_socket: QDesc = *self
                            .incoming_qds_map
                            .get(&catloop_socket)
                            .expect("queue descriptor not registered");
                        self.close_client(catnap_socket, catloop_socket);
                    },
                    _ => unreachable!(),
                };
            }
        }
    }

    /// Registers an incoming operation that is waiting for completion (pending).
    /// This function fails if the operation is already registered in the table of pending incoming operations.
    fn register_incoming_operation(&mut self, qd: QDesc, qt: QToken) -> Result<()> {
        if self.incoming_qts_map.insert(qt, qd).is_some() {
            anyhow::bail!("incoming operation is already registered (qt={:?})", qt);
        }
        self.incoming_qts.push(qt);
        Ok(())
    }

    /// Registers an outgoing operation that is waiting for completion (pending).
    /// This function fails if the operation is already registered in the table of pending outgoing operations.
    fn register_outgoing_operation(&mut self, qd: QDesc, qt: QToken) -> Result<()> {
        if self.outgoing_qts_map.insert(qt, qd).is_some() {
            anyhow::bail!("outgoing operation is already registered (qt={:?})", qt);
        }
        self.outgoing_qts.push(qt);
        Ok(())
    }

    /// Issues an `accept()`operation.
    /// This function fails if the underlying `accept()` operation fails.
    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.catnap.accept(self.local_socket)?;
        self.register_incoming_operation(self.local_socket, qt)?;
        Ok(())
    }

    /// Issues a `push()` operation in an incoming flow.
    /// This function fails if the underlying `push()` operation fails.
    fn issue_incoming_push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
        let qt: QToken = self.catnap.push(qd, &sga)?;

        // It is safe to call except() here, because we just issued the `push()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_incoming_operation(qd, qt)
            .expect("incoming push() operration is already registered");

        Ok(())
    }

    /// Issues a `pop()` operation in an incoming flow.
    /// This function fails if the underlying `pop()` operation fails.
    fn issue_incoming_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.catnap.pop(qd, None)?;

        // It is safe to call except() here, because we just issued the `pop()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_incoming_operation(qd, qt)
            .expect("incoming pop() operration is already registered");

        // Set the flag to indicate that this flow has an inflight `pop()` operation.
        // It is safe to call except() here, because `qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let catnap_inflight_pop: &mut bool = self
            .incoming_qds
            .get_mut(&qd)
            .expect("queue descriptor should be registered");
        *catnap_inflight_pop = true;

        Ok(())
    }

    /// Issues a `push()` operation in an outgoing flow.
    /// This function fails if the underlying `push()` operation fails.
    fn issue_outgoing_push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
        let qt: QToken = self.catloop.push(qd, &sga)?;

        // It is safe to call except() here, because we just issued the `push()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_outgoing_operation(qd, qt)
            .expect("outgoing push() operration is already registered");

        Ok(())
    }

    /// Issues a `pop()` operation in an outgoing flow.
    /// This function fails if the underlying `pop()` operation fails.
    fn issue_outgoing_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.catloop.pop(qd, None)?;

        // It is safe to call except() here, because we just issued the `pop()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_outgoing_operation(qd, qt)
            .expect("outgoing pop() operration is already registered");

        // Set the flag to indicate that this flow has an inflight `pop()` operation.
        // It is safe to call except() here, because `qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let catloop_inflight_pop: &mut bool = self
            .outgoing_qds
            .get_mut(&qd)
            .expect("queue descriptor should be registered");
        *catloop_inflight_pop = true;

        Ok(())
    }

    /// Handles the completion of an `accept()` operation.
    /// This function fails if we we fail to setup a connection with the remote address.
    fn handle_incoming_accept(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let new_client_socket: QDesc = unsafe { qr.qr_value.ares.qd.into() };

        // Setup remote connection.
        let new_server_socket: QDesc = match self.catloop.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
        };

        // Connect to remote address.
        // eprintln!("ACCEPT => connecting to {}", self.remote_addr);
        match self.catloop.connect(new_server_socket, self.remote_addr) {
            // Operation succeeded, register outgoing operation.
            Ok(qt) => self.register_outgoing_operation(new_server_socket, qt)?,
            // Operation failed, close socket.
            Err(e) => {
                if let Err(e) = self.catloop.close(new_server_socket) {
                    // Failed to close socket, log error.
                    println!("ERROR: close failed (error={:?})", e);
                    println!("WARN: leaking socket descriptor (sockqd={:?})", new_server_socket);
                }
                anyhow::bail!("failed to connect socket: {:?}", e)
            },
        };

        // Accept another connection.
        if let Err(e) = self.issue_accept() {
            // Failed to issue accept operation, log error.
            println!("ERROR: accept failed (error={:?})", e);
        };

        self.incoming_qds.insert(new_client_socket, false);
        self.incoming_qds_map.insert(new_server_socket, new_client_socket);
        self.outgoing_qds.insert(new_server_socket, false);
        self.outgoing_qds_map.insert(new_client_socket, new_server_socket);

        Ok(())
    }

    /// Handles the completion of a `connect()` operation.
    fn handle_outgoing_connect(&mut self, qr: &demi_qresult_t) {
        // eprintln!("handle_outgoing_connect");
        let catloop_qd: QDesc = qr.qr_qd.into();

        // It is safe to call except() here, because `catloop_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let catnap_qd: QDesc = *self
            .incoming_qds_map
            .get(&catloop_qd)
            .expect("queue descriptor should be registered");

        // Issue a `pop()` operation in the outgoing flow.
        if let Err(e) = self.issue_incoming_pop(catnap_qd) {
            // Failed to issue pop operation, log error.
            println!("ERROR: pop failed (error={:?})", e);
        }

        self.nclients += 1;
        // println!("INFO: {:?} clients connected", self.nclients);
    }

    /// Handles the completion of a `pop()` operation on an incoming flow.
    fn handle_incoming_pop(&mut self, qr: &demi_qresult_t) {
        eprintln!("Received Request");
        let incoming_sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
        let catnap_qd: QDesc = qr.qr_qd.into();

        // It is safe to call except() here, because `catnap_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let catloop_qd: QDesc = *self
            .outgoing_qds_map
            .get(&catnap_qd)
            .expect("queue descriptor should be registered");

        // Check if client closed connection.
        if incoming_sga.sga_segs[0].sgaseg_len == 0 {
            // println!("INFO: client closed connection");
            self.close_client(catnap_qd, catloop_qd);
            return;
        }

        // Push SGA to concerned outgoing flow.
        let src: *mut libc::c_uchar = incoming_sga.sga_segs[0].sgaseg_buf as *mut libc::c_uchar;
        let len: usize = incoming_sga.sga_segs[0].sgaseg_len as usize;
        // eprintln!("SGA LEN: {}", len);
        if let Ok(outgoing_sga) = self.catloop.sgaalloc(len) {
            // Copy.
            let dest: *mut libc::c_uchar = outgoing_sga.sga_segs[0].sgaseg_buf as *mut libc::c_uchar;
            Self::copy(src, dest, len);

            // Issue `push()` operation.
            if let Err(e) = self.issue_outgoing_push(catloop_qd, &outgoing_sga) {
                // Failed to issue push operation, log error.
                println!("ERROR: push failed (error={:?})", e);
            }

            // Release outgoing SGA.
            if let Err(e) = self.catloop.sgafree(outgoing_sga) {
                // Failed to release SGA, log error.
                println!("ERROR: sgafree failed (error={:?})", e);
                println!("WARN: leaking outgoing sga");
            }
        }

        // Release incoming SGA.
        if let Err(e) = self.catnap.sgafree(incoming_sga) {
            // Failed to release SGA, log error.
            println!("ERROR: sgafree failed (error={:?})", e);
            println!("WARN: leaking incoming sga");
        }

        // Pop more data from incoming flow.
        if let Err(e) = self.issue_incoming_pop(catnap_qd) {
            // Failed to issue pop operation, log error.
            println!("ERROR: pop failed (error={:?})", e);
        }
    }

    /// Handles the completion of a `pop()` operation on an outgoing flow.
    fn handle_outgoing_pop(&mut self, qr: &demi_qresult_t) {
        // eprintln!("handle_outgoing_pop");
        let outgoing_sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
        let catloop_qd: QDesc = qr.qr_qd.into();

        // It is safe to call except() here, because `catloop_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let catnap_qd: QDesc = *self
            .incoming_qds_map
            .get(&catloop_qd)
            .expect("queue descriptor should be registered");

        // Check if server aborted connection.
        if outgoing_sga.sga_segs[0].sgaseg_len == 0 {
            unimplemented!("server aborted connection");
        }

        // Push SGA to concerned incoming flow.
        let src: *mut libc::c_uchar = outgoing_sga.sga_segs[0].sgaseg_buf as *mut libc::c_uchar;
        let len: usize = outgoing_sga.sga_segs[0].sgaseg_len as usize;
        // eprintln!("SGA LEN: {}", len);
        if let Ok(incoming_sga) = self.catnap.sgaalloc(len) {
            // Copy.
            let dest: *mut libc::c_uchar = incoming_sga.sga_segs[0].sgaseg_buf as *mut libc::c_uchar;
            Self::copy(src, dest, len);

            // Issue `push()` operation.
            if let Err(e) = self.issue_incoming_push(catnap_qd, &incoming_sga) {
                // Failed to issue push operation, log error.
                println!("ERROR: push failed (error={:?})", e);
            }

            // Release incoming SGA.
            if let Err(e) = self.catnap.sgafree(incoming_sga) {
                // Failed to release SGA, log error.
                println!("ERROR: sgafree failed (error={:?})", e);
                println!("WARN: leaking incoming sga");
            }
        }

        // Release outgoing SGA.
        if let Err(e) = self.catloop.sgafree(outgoing_sga) {
            // Failed to release SGA, log error.
            println!("ERROR: sgafree failed (error={:?})", e);
            println!("WARN: leaking outgoing sga");
        }

        // Pop data from outgoing flow.
        if let Err(e) = self.issue_outgoing_pop(catloop_qd) {
            // Failed to issue pop operation, log error.
            println!("ERROR: pop failed (error={:?})", e);
        }
    }

    /// Handles the completion of a `push()` operation on an incoming flow.
    /// This will issue a pop operation on the incoming connection, if none is inflight.
    fn handle_incoming_push(&mut self, qr: &demi_qresult_t) {
        eprintln!("Sent Reply");
        // Extract queue descriptor of incoming connection.
        let incoming_qd: QDesc = qr.qr_qd.into();

        // It is safe to call except() here, because `incoming_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let has_inflight_pop: bool = self
            .incoming_qds
            .get_mut(&incoming_qd)
            .expect("queue descriptor should be registered")
            .to_owned();

        // Issue a pop operation if none is inflight.
        if !has_inflight_pop {
            unreachable!("should have an incoming pop, but it hasn't (qd={:?})", incoming_qd);
        }
    }

    /// Handles the completion of a `push()` operation on an outgoing flow.
    /// This will issue a pop operation on the outgoing connection, if none is inflight.
    fn handle_outgoing_push(&mut self, qr: &demi_qresult_t) {
        // eprintln!("handle_outgoing_push");
        // Extract queue descriptor of outgoing connection.
        let outgoing_qd: QDesc = qr.qr_qd.into();

        // It is safe to call except() here, because `outgoing_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let has_inflight_pop: bool = self
            .outgoing_qds
            .get_mut(&outgoing_qd)
            .expect("queue descriptor should be registered")
            .to_owned();

        // Issue a pop operation if none is inflight.
        if !has_inflight_pop {
            // println!("INFO: issuing outgoing pop (qd={:?})", outgoing_qd);
            if let Err(e) = self.issue_outgoing_pop(outgoing_qd) {
                // Failed to issue pop operation, log error.
                println!("ERROR: pop failed (error={:?})", e);
            }
        }
    }

    // Closes an incoming flow.
    fn close_client(&mut self, catnap_socket: QDesc, catloop_socket: QDesc) {
        match self.catnap.close(catnap_socket) {
            Ok(_) => {
                println!("handle cancellation of tokens (catnap_socket={:?})", catnap_socket);
                self.incoming_qds.remove(&catnap_socket).unwrap();
                self.outgoing_qds_map.remove(&catnap_socket).unwrap();
                let qts_drained: HashMap<QToken, QDesc> = self.incoming_qts_map.extract_if(|_k, v| v == &catnap_socket).collect();
                let _: Vec<_> = self.incoming_qts.extract_if(|x| qts_drained.contains_key(x)).collect();
            },
            Err(e) => println!("ERROR: failed to close socket (error={:?})", e),
        }

        match self.catloop.close(catloop_socket) {
            Ok(_) => {
                println!("handle cancellation of tokens (catloop_socket={:?})", catloop_socket);
                self.outgoing_qds.remove(&catloop_socket).unwrap();
                self.incoming_qds_map.remove(&catloop_socket).unwrap();
                let qts_drained: HashMap<QToken, QDesc> = self.outgoing_qts_map.extract_if(|_k, v| v == &catloop_socket).collect();
                let _: Vec<_> = self.outgoing_qts.extract_if(|x| qts_drained.contains_key(x)).collect();
            },
            Err(e) => println!("ERROR: failed to close socket (error={:?})", e),
        }
        self.nclients -= 1;
    }

    /// Polls incoming operations that are pending, with a timeout.
    ///
    /// If any pending operation completes when polling, its result value is
    /// returned. If the timeout expires before an operation completes, or an
    /// error is encountered, None is returned instead.
    fn poll_incoming(&mut self, timeout: Option<Duration>) -> Option<demi_qresult_t> {
        match self.catnap.wait_any(&self.incoming_qts, timeout) {
            Ok((idx, qr)) => {
                let qt: QToken = self.incoming_qts.remove(idx);
                // It is safe to call except() here, because `qt` is ensured to be in the table of pending operations.
                // All queue tokens are registered in the table of pending operations when they are issued.
                self.incoming_qts_map
                    .remove(&qt)
                    .expect("queue token should be registered");
                Some(qr)
            },
            Err(e) if e.errno == libc::ETIMEDOUT => None,
            Err(e) => {
                println!("ERROR: unexpected error while polling incoming queue (error={:?})", e);
                None
            },
        }
    }

    /// Polls outgoing operations that are pending, with a timeout.
    ///
    /// If any pending operation completes when polling, its result value is
    /// returned. If the timeout expires before an operation completes, or an
    /// error is encountered, None is returned instead.
    fn poll_outgoing(&mut self, timeout: Option<Duration>) -> Option<demi_qresult_t> {
        match self.catloop.wait_any(&self.outgoing_qts, timeout) {
            Ok((idx, qr)) => {
                let qt: QToken = self.outgoing_qts.remove(idx);
                // It is safe to call except() here, because `qt` is ensured to be in the table of pending operations.
                // All queue tokens are registered in the table of pending operations when they are issued.
                self.outgoing_qts_map
                    .remove(&qt)
                    .expect("queue token should be registered");
                Some(qr)
            },
            Err(e) if e.errno == libc::ETIMEDOUT => None,
            Err(e) => {
                println!("ERROR: unexpected error while polling outgoing queue (error={:?})", e);
                None
            },
        }
    }

    /// Copies `len` bytes from `src` to `dest`.
    fn copy(src: *mut libc::c_uchar, dest: *mut libc::c_uchar, len: usize) {
        let src: &mut [u8] = unsafe { slice::from_raw_parts_mut(src, len) };
        let dest: &mut [u8] = unsafe { slice::from_raw_parts_mut(dest, len) };
        dest.clone_from_slice(src);
    }

    /// Setups local socket.
    fn setup_local_socket(catnap: &mut LibOS, local_addr: SocketAddr) -> Result<QDesc> {
        // Create local socket.
        let local_socket: QDesc = match catnap.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
        };

        // Bind socket to local address.
        if let Err(e) = catnap.bind(local_socket, local_addr) {
            // Bind failed, close socket.
            if let Err(e) = catnap.close(local_socket) {
                // Close failed, log error.
                println!("ERROR: close failed (error={:?})", e);
                println!("WARN: leaking socket descriptor (sockqd={:?})", local_socket);
            }
            anyhow::bail!("bind failed: {:?}", e.cause)
        };

        // Enable socket to accept incoming connections.
        if let Err(e) = catnap.listen(local_socket, 16) {
            // Listen failed, close socket.
            if let Err(e) = catnap.close(local_socket) {
                // Close failed, log error.
                println!("ERROR: close failed (error={:?})", e);
                println!("WARN: leaking socket descriptor (sockqd={:?})", local_socket);
            }
            anyhow::bail!("listen failed: {:?}", e.cause)
        }

        Ok(local_socket)
    }
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // Check command line arguments.
    if args.len() < 3 {
        println!("Usage: {} local-address remote-address\n", &args[0]);
        return Ok(());
    }

    let local_addr: SocketAddr = SocketAddr::from_str(&args[1])?;
    let remote_addr: SocketAddr = SocketAddr::from_str(&args[2])?;
    let mut proxy: TcpProxy = TcpProxy::new(local_addr, remote_addr)?;
    proxy.run()?;
}