// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::SharedCatmemLibOS,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::unwrap_socketaddr,
        queue::{
            QDesc,
            QToken,
        },
        scheduler::Yielder,
        types::{
            demi_opcode_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        OperationResult,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::rand::{
    rngs::SmallRng,
    RngCore,
    SeedableRng,
};
use ::std::{
    collections::HashSet,
    fmt::Debug,
    mem,
    net::{
        Ipv4Addr,
        SocketAddr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    slice,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Maximum number of connection attempts.
/// This was chosen arbitrarily.
const MAX_ACK_RECEIVED_ATTEMPTS: usize = 1024;
/// Seed number for generating request IDs.
#[cfg(debug_assertions)]
const REQUEST_ID_SEED: u64 = 95;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A socket.
pub struct MemorySocket {
    /// Underlying shared memory pipe.
    catmem_qd: Option<QDesc>,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
    /// Maximum backlog length for passive sockets.
    backlog: usize,
    /// Pending connect requests for passive sockets.
    pending_request_ids: HashSet<RequestId>,
    /// Random number generator for request ids.
    rng: SmallRng,
}

pub struct SharedMemorySocket(SharedObject<MemorySocket>);

/// Unique identifier for a request.
#[derive(Hash, Eq, PartialEq, Copy, Clone)]
pub struct RequestId(u64);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedMemorySocket {
    /// Creates a new socket that is not bound to an address.
    pub fn new() -> Self {
        Self(SharedObject::new(MemorySocket {
            catmem_qd: None,
            local: None,
            remote: None,
            backlog: 1,
            pending_request_ids: HashSet::<RequestId>::new(),
            #[cfg(debug_assertions)]
            rng: SmallRng::seed_from_u64(REQUEST_ID_SEED),
            #[cfg(not(debug_assertions))]
            rng: SmallRng::from_entropy(),
        }))
    }

    /// Allocates a new socket that is bound to [local].
    fn alloc(catmem_qd: QDesc, local: Option<SocketAddrV4>, remote: Option<SocketAddrV4>) -> Self {
        Self(SharedObject::new(MemorySocket {
            catmem_qd: Some(catmem_qd),
            local,
            remote,
            backlog: 1,
            pending_request_ids: HashSet::<RequestId>::new(),
            #[cfg(debug_assertions)]
            rng: SmallRng::seed_from_u64(REQUEST_ID_SEED),
            #[cfg(not(debug_assertions))]
            rng: SmallRng::from_entropy(),
        }))
    }

    /// Binds the target socket to `local` address.
    /// TODO: Should probably move the create of the duplex pipe to listen.
    pub fn bind(&mut self, local: SocketAddrV4, catmem: &mut SharedCatmemLibOS) -> Result<(), Fail> {
        // Create underlying memory channels.
        let ipv4: &Ipv4Addr = local.ip();
        let port: u16 = local.port();
        self.catmem_qd = Some(catmem.create_pipe(&format_pipe_str(ipv4, port))?);
        self.local = Some(local);
        Ok(())
    }

    /// Enables this socket to accept incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        self.backlog = backlog;
        Ok(())
    }

    /// Attempts to accept a new connection on this socket. On success, returns a new Socket for the accepted connection.
    pub async fn accept(
        &mut self,
        mut runtime: SharedDemiRuntime,
        mut catmem: SharedCatmemLibOS,
        yielder: &Yielder,
    ) -> Result<(Self, SocketAddr), Fail> {
        // Allocate ephemeral port.
        let new_port: u16 = runtime.alloc_ephemeral_port()?;
        let ipv4: Ipv4Addr = *self
            .local
            .expect("Should be bound to a local address to accept connections")
            .ip();
        loop {
            // Check if backlog is full.
            if self.pending_request_ids.len() >= self.backlog {
                // It is, thus just log a debug message, pending requests will still be queued anyways.
                let cause: String = format!(
                    "backlog is full (backlog={:?}), pending={:?}",
                    self.backlog,
                    self.pending_request_ids.len()
                );
                debug!("do_accept(): {:?}", &cause);
            }

            // Grab next request from the control duplex pipe.
            let new_qd: QDesc = match self
                .pop_request_id(
                    &mut runtime,
                    &mut catmem,
                    self.catmem_qd.expect("should be connected"),
                    &yielder,
                )
                .await
            {
                // Received a request id so create the new connection. This involves create the new duplex pipe
                // and sending the port number to the remote.
                Ok(request_id) => {
                    if self.pending_request_ids.contains(&request_id) {
                        debug!("do_accept(): duplicate request (request_id={:?})", request_id.0);
                        continue;
                    } else {
                        self.pending_request_ids.insert(request_id);
                        match self
                            .create_pipe(&mut runtime, &mut catmem, &ipv4, new_port, &yielder)
                            .await
                        {
                            Ok(new_qd) => new_qd,
                            Err(e) => {
                                runtime.free_ephemeral_port(new_port)?;
                                return Err(e);
                            },
                        }
                    }
                },
                // Some error.
                Err(e) => {
                    runtime.free_ephemeral_port(new_port)?;
                    return Err(e);
                },
            };

            let new_addr: SocketAddrV4 = SocketAddrV4::new(ipv4, new_port);
            let new_socket: Self = Self::alloc(new_qd, None, Some(new_addr));
            let result: Result<RequestId, Fail> =
                self.pop_request_id(&mut runtime, &mut catmem, new_qd, &yielder).await;

            // Check that the remote has retrieved the port number and responded with a valid request id.
            match result {
                // Valid response. Connection successfully established, so return new port and pipe to application.
                Ok(request_id) => {
                    // If we've never seen this before, something has gone very wrong.
                    assert!(self.pending_request_ids.contains(&request_id));
                    self.pending_request_ids.remove(&request_id);
                    return Ok((new_socket, new_addr.into()));
                },
                // Some error.
                Err(e) => {
                    // Clean up newly allocated duplex pipe.
                    catmem.close(new_qd)?;
                    runtime.free_ephemeral_port(new_port)?;
                    return Err(e);
                },
            };
        }
    }

    /// Connects this socket to [remote].
    pub async fn connect(
        &mut self,
        mut runtime: SharedDemiRuntime,
        mut catmem: SharedCatmemLibOS,
        remote: SocketAddr,
        yielder: &Yielder,
    ) -> Result<(), Fail> {
        let ipv4: Ipv4Addr = *unwrap_socketaddr(remote)?.ip();
        let port: u16 = remote.port().into();
        let request_id: RequestId = RequestId(self.rng.next_u64());

        // Gets the port for the new connection from the server by sending a connection request repeatedly until a port
        // comes back.
        let result: Result<(QDesc, SocketAddrV4), Fail> = {
            let new_port: u16 = match self
                .get_port(&mut catmem, &mut runtime, &ipv4, port, &request_id, yielder)
                .await
            {
                Ok(new_port) => new_port,
                Err(e) => {
                    return Err(e);
                },
            };

            // Open underlying pipes.
            let remote: SocketAddrV4 = SocketAddrV4::new(ipv4, new_port);
            let new_qd: QDesc = match catmem.open_pipe(&format_pipe_str(&ipv4, new_port)) {
                Ok(new_qd) => new_qd,
                Err(e) => {
                    return Err(e);
                },
            };
            // Send an ack to the server over the new pipe.
            if let Err(e) = self
                .send_ack(&mut catmem, &mut runtime, new_qd, &request_id, &yielder)
                .await
            {
                return Err(e);
            }
            Ok((new_qd, remote))
        };

        match result {
            Ok((new_qd, remote)) => {
                self.catmem_qd = Some(new_qd);
                self.remote = Some(remote);
                Ok(())
            },
            Err(e) => Err(e),
        }
    }

    /// Closes `socket`.
    pub async fn close(&mut self, catmem: SharedCatmemLibOS, yielder: Yielder) -> Result<(), Fail> {
        if let Some(qd) = self.catmem_qd {
            match catmem.close_coroutine(qd, yielder).await {
                (_, OperationResult::Close) => (),
                (_, OperationResult::Failed(e)) => return Err(e),
                _ => panic!("Should not return anything other than close or fail"),
            }
        };
        Ok(())
    }

    pub fn hard_close(&mut self, catmem: &mut SharedCatmemLibOS) -> Result<(), Fail> {
        if let Some(qd) = self.catmem_qd {
            catmem.close(qd)
        } else {
            Ok(())
        }
    }

    /// Asynchronous code for pushing to the underlying Catmem transport.
    pub async fn push(
        &mut self,
        catmem: SharedCatmemLibOS,
        buf: &mut DemiBuffer,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        // It is safe to unwrap here, because we have just checked for the socket state
        // and by construction it should be connected. If not, the socket state machine
        // was not correctly driven.
        let qd: QDesc = self.catmem_qd.expect("socket should be connected");
        // TODO: Remove the copy eventually.
        match catmem.push_coroutine(qd, buf.clone(), yielder).await {
            (_, OperationResult::Push) => {
                buf.trim(buf.len())?;
                Ok(())
            },
            (_, OperationResult::Failed(e)) => return Err(e),
            _ => panic!("Should not return anything other than push or fail"),
        }
    }

    /// Asynchronous code for popping from the underlying Catmem transport.
    pub async fn pop(
        &mut self,
        catmem: SharedCatmemLibOS,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: Yielder,
    ) -> Result<Option<SocketAddr>, Fail> {
        // It is safe to unwrap here, because we have just checked for the socket state
        // and by construction it should be connected. If not, the socket state machine
        // was not correctly driven.
        let qd: QDesc = self.catmem_qd.expect("socket should be connected");
        match catmem.pop_coroutine(qd, Some(size), yielder).await {
            (_, OperationResult::Pop(_, incoming)) => {
                let len: usize = incoming.len();
                // TODO: Remove this copy. Our API should support passing back a buffer without sending in a buffer.
                buf.trim(size - len)?;
                buf.copy_from_slice(&incoming[0..len]);

                let remote: SocketAddr = self.remote.expect("can only pop from a connected socket").into();
                Ok(Some(remote))
            },
            (_, OperationResult::Failed(e)) => Err(e),
            _ => panic!("Should not return anything other than push or fail"),
        }
    }

    /// Returns the `local` address to which [self] is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    /// Returns the `remote` address tot which [self] is connected.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }

    /// Gets the next connection request.
    async fn pop_request_id(
        &mut self,
        runtime: &mut SharedDemiRuntime,
        catmem: &mut SharedCatmemLibOS,
        catmem_qd: QDesc,
        yielder: &Yielder,
    ) -> Result<RequestId, Fail> {
        // Issue pop. No need to bound the pop because we've quantized it already in the concurrent ring buffer.
        let qt: QToken = catmem.pop(catmem_qd, Some(mem::size_of::<RequestId>()))?;
        // Yield until pop completes.
        while !runtime.has_completed(qt)? {
            if let Err(e) = yielder.yield_once().await {
                return Err(e);
            }
        }
        // Re-acquire mutable reference.
        // Retrieve operation result and check if it is what we expect.
        let qr: demi_qresult_t = runtime.remove_coroutine_and_get_result(qt)?;
        match qr.qr_opcode {
            // We expect a successful completion for previous pop().
            demi_opcode_t::DEMI_OPC_POP => {},
            // We may get some error.
            demi_opcode_t::DEMI_OPC_FAILED => {
                let cause: String = format!(
                    "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                    qr.qr_qd, qt, qr.qr_ret
                );
                error!("pop_request_id(): {:?}", &cause);
                return Err(Fail::new(qr.qr_ret as i32, &cause));
            },
            // We do not expect anything else.
            _ => {
                // The following statement is unreachable because we have issued a pop operation.
                // If we successfully complete a different operation, something really bad happen in the scheduler.
                unreachable!("unexpected operation on control duplex pipe")
            },
        }

        // Extract scatter-gather array from operation result.
        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

        // Parse and check request.
        let result: Result<RequestId, Fail> = get_connect_id(&sga);
        runtime.sgafree(sga)?;
        result
    }

    // Sends the port number to the peer process.
    async fn create_pipe(
        &mut self,
        runtime: &mut SharedDemiRuntime,
        catmem: &mut SharedCatmemLibOS,
        ipv4: &Ipv4Addr,
        port: u16,
        yielder: &Yielder,
    ) -> Result<QDesc, Fail> {
        let catmem_qd: QDesc = self.catmem_qd.expect("should be connected");
        // Create underlying pipes before sending the port number through the
        // control duplex pipe. This prevents us from running into a race
        // condition were the remote makes progress faster than us and attempts
        // to open the duplex pipe before it is created.
        let new_qd: QDesc = catmem.create_pipe(&format_pipe_str(ipv4, port))?;
        // Allocate a scatter-gather array and send the port number to the remote.
        let sga: demi_sgarray_t = runtime.sgaalloc(mem::size_of_val(&port))?;
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.copy_from_slice(&port.to_ne_bytes());

        // Push the port number.
        let qt: QToken = catmem.push(catmem_qd, &sga)?;

        // Wait for push to complete.
        while !runtime.has_completed(qt)? {
            if let Err(e) = yielder.yield_once().await {
                return Err(e);
            }
        }
        // Free the scatter-gather array.
        runtime.sgafree(sga)?;

        // Retrieve operation result and check if it is what we expect.
        let qr: demi_qresult_t = runtime.remove_coroutine_and_get_result(qt)?;
        match qr.qr_opcode {
            // We expect a successful completion for previous push().
            demi_opcode_t::DEMI_OPC_PUSH => Ok(new_qd),
            // We may get some error.
            demi_opcode_t::DEMI_OPC_FAILED => {
                let cause: String = format!(
                    "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                    qr.qr_qd, qt, qr.qr_ret
                );
                error!("create_pipe(): {:?}", &cause);
                Err(Fail::new(qr.qr_ret as i32, &cause))
            },
            // We do not expect anything else.
            _ => {
                // The following statement is unreachable because we have issued a pop operation.
                // If we successfully complete a different operation, something really bad happen in the scheduler.
                unreachable!("unexpected operation on control duplex pipe")
            },
        }
    }

    async fn send_connection_request(
        &mut self,
        catmem: &mut SharedCatmemLibOS,
        runtime: &mut SharedDemiRuntime,
        connect_qd: QDesc,
        request_id: &RequestId,
        yielder: &Yielder,
    ) -> Result<(), Fail> {
        // Create a message containing the magic number.
        let sga: demi_sgarray_t = self.send_connect_id(runtime, request_id)?;

        // Send to server.
        let qt: QToken = catmem.push(connect_qd, &sga)?;
        trace!("Send connection request qtoken={:?}", qt);
        // Yield until push completes.
        while !runtime.has_completed(qt)? {
            if let Err(e) = yielder.yield_once().await {
                return Err(e);
            }
        }
        // Free the message buffer.
        runtime.sgafree(sga)?;
        // Get the result of the push.
        let qr: demi_qresult_t = runtime.remove_coroutine_and_get_result(qt)?;
        match qr.qr_opcode {
            // We expect a successful completion for previous push().
            demi_opcode_t::DEMI_OPC_PUSH => Ok(()),
            // We may get some error.
            demi_opcode_t::DEMI_OPC_FAILED => {
                let cause: String = format!(
                    "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                    qr.qr_qd, qt, qr.qr_ret
                );
                error!("send_connection_request(): {:?}", &cause);
                Err(Fail::new(qr.qr_ret as i32, &cause))
            },
            // We do not expect anything else.
            _ => {
                // The following statement is unreachable because we have issued a pop operation.
                // If we successfully complete a different operation, something really bad happen in the scheduler.
                unreachable!("unexpected operation on control duplex pipe")
            },
        }
    }

    async fn get_port(
        &mut self,
        catmem: &mut SharedCatmemLibOS,
        runtime: &mut SharedDemiRuntime,
        ipv4: &Ipv4Addr,
        port: u16,
        request_id: &RequestId,
        yielder: &Yielder,
    ) -> Result<u16, Fail> {
        // Issue receive operation to wait for connect request ack.
        let size: usize = mem::size_of::<u16>();
        // Open connection to server.
        let connect_qd: QDesc = match catmem.open_pipe(&format_pipe_str(ipv4, port)) {
            Ok(qd) => qd,
            Err(e) => {
                // Interpose error.
                if e.errno == libc::ENOENT {
                    let cause: String = format!("failed to establish connection (ipv4={:?}, port={:?})", ipv4, port);
                    error!("get_port(): {:?}", &cause);
                    return Err(Fail::new(libc::ECONNREFUSED, &cause));
                }

                return Err(e);
            },
        };

        let qt: QToken = catmem.pop(connect_qd, Some(size))?;
        trace!("Read port qtoken={:?}", qt);

        loop {
            // Send the connection request to the server.
            self.send_connection_request(catmem, runtime, connect_qd, request_id, &yielder)
                .await?;

            // Wait on the pop for MAX_ACK_RECEIVED_ATTEMPTS
            for _ in 0..MAX_ACK_RECEIVED_ATTEMPTS {
                match yielder.yield_once().await {
                    Ok(()) if runtime.has_completed(qt)? => break,
                    Ok(()) => continue,
                    Err(e) => return Err(e),
                }
            }
            // If we received a port back from the server, then unpack it. Otherwise, send the connection request again.
            if runtime.has_completed(qt)? {
                // Re-acquire reference to Catmem libos.
                // Get the result of the pop.
                let qr: demi_qresult_t = runtime.remove_coroutine_and_get_result(qt)?;
                match qr.qr_opcode {
                    // We expect a successful completion for previous pop().
                    demi_opcode_t::DEMI_OPC_POP => {
                        // Extract scatter-gather array from operation result.
                        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                        // Extract port number.
                        let port: Result<u16, Fail> = extract_port_number(&sga);
                        runtime.sgafree(sga)?;
                        return port;
                    },
                    // We may get some error.
                    demi_opcode_t::DEMI_OPC_FAILED => {
                        // Shut down control duplex pipe as we can open the new pipe now.
                        catmem.shutdown(connect_qd)?;

                        let cause: String = format!(
                            "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                            qr.qr_qd, qt, qr.qr_ret
                        );
                        error!("get_port(): {:?}", &cause);
                        return Err(Fail::new(qr.qr_ret as i32, &cause));
                    },
                    // We do not expect anything else.
                    _ => {
                        // The following statement is unreachable because we have issued a pop operation.
                        // If we successfully complete a different operation, something really bad happen in the scheduler.
                        unreachable!("unexpected operation on control duplex pipe")
                    },
                }
            }
        }
    }

    // Send an ack through a new Catmem pipe.
    async fn send_ack(
        &mut self,
        catmem: &mut SharedCatmemLibOS,
        runtime: &mut SharedDemiRuntime,
        new_qd: QDesc,
        request_id: &RequestId,
        yielder: &Yielder,
    ) -> Result<(), Fail> {
        // Create message with magic connect.
        let sga: demi_sgarray_t = self.send_connect_id(runtime, request_id)?;
        // Send to server through new pipe.
        let qt: QToken = catmem.push(new_qd, &sga)?;
        trace!("Send ack qtoken={:?}", qt);

        // Yield until push completes.
        while !runtime.has_completed(qt)? {
            if let Err(e) = yielder.yield_once().await {
                return Err(e);
            }
        }
        // Free the message buffer.
        runtime.sgafree(sga)?;
        // Retrieve operation result and check if it is what we expect.
        let qr: demi_qresult_t = runtime.remove_coroutine_and_get_result(qt)?;

        match qr.qr_opcode {
            // We expect a successful completion for previous push().
            demi_opcode_t::DEMI_OPC_PUSH => Ok(()),
            // We may get some error.
            demi_opcode_t::DEMI_OPC_FAILED => {
                let cause: String = format!(
                    "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                    qr.qr_qd, qt, qr.qr_ret
                );
                error!("send_ack(): {:?}", &cause);
                Err(Fail::new(qr.qr_ret as i32, &cause))
            },
            // We do not expect anything else.
            _ => {
                // The following statement is unreachable because we have issued a pop operation.
                // If we successfully complete a different operation, something really bad happen in the scheduler.
                unreachable!("unexpected operation on control duplex pipe")
            },
        }
    }

    /// Creates a magic connect message.
    pub fn send_connect_id(
        &mut self,
        runtime: &mut SharedDemiRuntime,
        request_id: &RequestId,
    ) -> Result<demi_sgarray_t, Fail> {
        let sga: demi_sgarray_t = runtime.sgaalloc(mem::size_of_val(&request_id))?;
        let ptr: *mut u64 = sga.sga_segs[0].sgaseg_buf as *mut u64;
        unsafe {
            *ptr = request_id.0;
        }
        Ok(sga)
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Extracts port number from connect request ack message.
fn extract_port_number(sga: &demi_sgarray_t) -> Result<u16, Fail> {
    match sga.sga_segs[0].sgaseg_len as usize {
        len if len == 0 => {
            let cause: String = format!("server closed connection");
            error!("extract_port_number(): {:?}", &cause);
            Err(Fail::new(libc::EBADF, &cause))
        },
        len if len == 2 => {
            let data_ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(data_ptr, len) };
            let array: [u8; 2] = [slice[0], slice[1]];
            Ok(u16::from_ne_bytes(array))
        },
        len => {
            let cause: String = format!("server sent invalid port number (len={:?})", len);
            error!("extract_port_number(): {:?}", &cause);
            Err(Fail::new(libc::EBADMSG, &cause))
        },
    }
}

/// Checks for a magic connect message.
fn get_connect_id(sga: &demi_sgarray_t) -> Result<RequestId, Fail> {
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    if len == mem::size_of::<RequestId>() {
        let ptr: *mut u64 = sga.sga_segs[0].sgaseg_buf as *mut u64;
        Ok(RequestId(unsafe { *ptr }))
    } else {
        let cause: String = format!("invalid connect request (len={:?})", len);
        error!("get_connect_id(): {:?}", &cause);
        Err(Fail::new(libc::ECONNREFUSED, &cause))
    }
}

fn format_pipe_str(ip: &Ipv4Addr, port: u16) -> String {
    format!("{}:{}", ip, port)
}

impl Deref for SharedMemorySocket {
    type Target = MemorySocket;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedMemorySocket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl Debug for SharedMemorySocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Memory socket: local={:?} remote={:?} catmem_qd={:?}",
            self.local, self.remote, self.catmem_qd
        )
    }
}
