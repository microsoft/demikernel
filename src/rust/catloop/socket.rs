// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::SharedCatmemLibOS,
    runtime::{
        conditional_yield_with_timeout,
        fail::Fail,
        memory::DemiBuffer,
        network::unwrap_socketaddr,
        queue::QDesc,
        OperationResult,
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
    time::Duration,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Seed number for generating request IDs.
#[cfg(debug_assertions)]
const REQUEST_ID_SEED: u64 = 95;
/// Amount of time to wait for the other end in milliseconds. This was chosen arbitrarily.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

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
    pub async fn accept(&mut self, new_port: u16, mut catmem: SharedCatmemLibOS) -> Result<(Self, SocketAddr), Fail> {
        // Allocate ephemeral port.
        let ipv4: Ipv4Addr = *self
            .local
            .expect("Should be bound to a local address to accept connections")
            .ip();
        let new_qd: QDesc = loop {
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
            let request_id: RequestId =
                pop_request_id(catmem.clone(), self.catmem_qd.expect("should be connected")).await?;

            // Received a request id so create the new connection. This involves create the new duplex pipe
            // and sending the port number to the remote.
            if self.pending_request_ids.contains(&request_id) {
                debug!("do_accept(): duplicate request (request_id={:?})", request_id.0);
                continue;
            } else {
                self.pending_request_ids.insert(request_id);
                break create_pipe(
                    self.catmem_qd.expect("pipe should have been created"),
                    catmem.clone(),
                    &ipv4,
                    new_port,
                )
                .await?;
            }
        };

        let new_addr: SocketAddrV4 = SocketAddrV4::new(ipv4, new_port);
        let new_socket: Self = Self::alloc(new_qd, Some(new_addr), None);

        // Check that the remote has retrieved the port number and responded with a valid request id.
        match pop_request_id(catmem.clone(), new_qd).await {
            // Valid response. Connection successfully established, so return new port and pipe to application.
            Ok(request_id) => {
                // If we've never seen this before, something has gone very wrong.
                assert!(self.pending_request_ids.contains(&request_id));
                self.pending_request_ids.remove(&request_id);
                Ok((new_socket, new_addr.into()))
            },
            // Some error.
            Err(e) => {
                // Clean up newly allocated duplex pipe.
                catmem.close(new_qd)?;
                Err(e)
            },
        }
    }

    /// Connects this socket to [remote].
    pub async fn connect(&mut self, mut catmem: SharedCatmemLibOS, remote: SocketAddr) -> Result<(), Fail> {
        let ipv4: Ipv4Addr = *unwrap_socketaddr(remote)?.ip();
        let port: u16 = remote.port().into();
        let request_id: RequestId = RequestId(self.rng.next_u64());

        // Gets the port for the new connection from the server by sending a connection request repeatedly until a port
        // comes back.
        let result: Result<(QDesc, SocketAddrV4), Fail> = {
            let new_port: u16 = match get_port(catmem.clone(), &ipv4, port, &request_id).await {
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
            if let Err(e) = send_ack(catmem.clone(), new_qd, &request_id).await {
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
    pub async fn close(&mut self, catmem: SharedCatmemLibOS) -> Result<(), Fail> {
        if let Some(qd) = self.catmem_qd {
            match catmem.close_coroutine(qd).await {
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
    pub async fn push(&mut self, catmem: SharedCatmemLibOS, buf: &mut DemiBuffer) -> Result<(), Fail> {
        // It is safe to unwrap here, because we have just checked for the socket state
        // and by construction it should be connected. If not, the socket state machine
        // was not correctly driven.
        let qd: QDesc = self.catmem_qd.expect("socket should be connected");
        // TODO: Remove the copy eventually.
        match catmem.push_coroutine(qd, buf.clone()).await {
            (_, OperationResult::Push) => {
                buf.trim(buf.len())?;
                Ok(())
            },
            (_, OperationResult::Failed(e)) => Err(e),
            _ => unreachable!("Should not return anything other than push or fail"),
        }
    }

    /// Asynchronous code for popping from the underlying Catmem transport.
    pub async fn pop(
        &mut self,
        catmem: SharedCatmemLibOS,
        buf: &mut DemiBuffer,
        size: usize,
    ) -> Result<Option<SocketAddr>, Fail> {
        // It is safe to unwrap here, because we have just checked for the socket state
        // and by construction it should be connected. If not, the socket state machine
        // was not correctly driven.
        let qd: QDesc = self.catmem_qd.expect("socket should be connected");
        match catmem.pop_coroutine(qd, Some(size)).await {
            (_, OperationResult::Pop(_, incoming)) => {
                let len: usize = incoming.len();
                // TODO: Remove this copy. Our API should support passing back a buffer without sending in a buffer.
                buf.trim(size - len)?;
                buf.copy_from_slice(&incoming[0..len]);
                // We do not keep a socket address for the remote socket, so none to return.
                Ok(None)
            },
            (_, OperationResult::Failed(e)) => Err(e),
            _ => unreachable!("Should not return anything other than push or fail"),
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
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Create a memory pipe and send the port number to the peer process.
async fn create_pipe(
    catmem_qd: QDesc,
    mut catmem: SharedCatmemLibOS,
    ipv4: &Ipv4Addr,
    port: u16,
) -> Result<QDesc, Fail> {
    // Create underlying pipes before sending the port number through the
    // control duplex pipe. This prevents us from running into a race
    // condition were the remote makes progress faster than us and attempts
    // to open the duplex pipe before it is created.
    let new_qd: QDesc = catmem.create_pipe(&format_pipe_str(ipv4, port))?;
    // Allocate a scatter-gather array and send the port number to the remote.
    let buf: DemiBuffer = DemiBuffer::from_slice(&port.to_ne_bytes())?;

    // Push the port number.
    match catmem.push_coroutine(catmem_qd, buf).await {
        (_, OperationResult::Push) => Ok(new_qd),
        (qd, OperationResult::Failed(e)) => {
            debug_assert_eq!(new_qd, qd);

            let cause: String = format!("failed to establish connection (qd={:?}, errno={:?})", qd, e);
            error!("create_pipe(): {:?}", &cause);
            Err(e)
        },
        _ => unreachable!("Should not return anything other than push or fail"),
    }
}

/// Gets the next connection request.
async fn pop_request_id(catmem: SharedCatmemLibOS, catmem_qd: QDesc) -> Result<RequestId, Fail> {
    // Issue pop. No need to bound the pop because we've quantized it already in the concurrent ring buffer.
    match catmem.pop_coroutine(catmem_qd, Some(mem::size_of::<RequestId>())).await {
        // We expect a successful completion for previous pop().
        (_, OperationResult::Pop(_, incoming)) => {
            // Parse and check request.
            let result: Result<RequestId, Fail> = get_connect_id(incoming);
            result
        },
        // We may get some error.
        (qd, OperationResult::Failed(e)) => {
            let cause: String = format!("failed to establish connection (qd={:?}, errno={:?})", qd, e);
            error!("pop_request_id(): {:?}", &cause);
            Err(e)
        },
        // We do not expect anything else.
        _ => {
            // The following statement is unreachable because we have issued a pop operation.
            // If we successfully complete a different operation, something really bad happen in the scheduler.
            unreachable!("unexpected operation on control duplex pipe")
        },
    }
}

// Send a request id through a pipe.
async fn send_connection_request(
    catmem: SharedCatmemLibOS,
    connect_qd: QDesc,
    request_id: &RequestId,
) -> Result<(), Fail> {
    // Create a message containing the magic number.
    // Create message with magic connect.
    let buf: DemiBuffer = DemiBuffer::from_slice(&request_id.0.to_ne_bytes())?;

    // Send to server.
    match catmem.push_coroutine(connect_qd, buf).await {
        (_, OperationResult::Push) => Ok(()),
        (qd, OperationResult::Failed(e)) => {
            debug_assert_eq!(connect_qd, qd);

            let cause: String = format!("failed to establish connection (qd={:?}, errno={:?})", qd, e);
            error!("create_pipe(): {:?}", &cause);
            Err(e)
        },
        _ => unreachable!("Should not return anything other than push or fail"),
    }
}

async fn get_port(
    mut catmem: SharedCatmemLibOS,
    ipv4: &Ipv4Addr,
    port: u16,
    request_id: &RequestId,
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

    // Send the connection request to the server.
    send_connection_request(catmem.clone(), connect_qd, request_id).await?;

    // Wait for response until some timeout.
    match conditional_yield_with_timeout(catmem.pop_coroutine(connect_qd, Some(size)), DEFAULT_TIMEOUT).await? {
        // We expect a successful completion for previous pop().
        (_, OperationResult::Pop(_, incoming)) => extract_port_number(incoming),
        // We may get some error.
        (qd, OperationResult::Failed(e)) => {
            let cause: String = format!("failed to establish connection (qd={:?}, errno={:?})", qd, e);
            error!("get_port(): {:?}", &cause);
            Err(e)
        },
        // We do not expect anything else.
        _ => {
            // The following statement is unreachable because we have issued a pop operation.
            // If we successfully complete a different operation, something really bad happen in the scheduler.
            unreachable!("unexpected operation on control duplex pipe")
        },
    }
}

// Send an ack through a new Catmem pipe.
async fn send_ack(catmem: SharedCatmemLibOS, new_qd: QDesc, request_id: &RequestId) -> Result<(), Fail> {
    // Create message with magic connect.
    let buf: DemiBuffer = DemiBuffer::from_slice(&request_id.0.to_ne_bytes())?;

    match catmem.push_coroutine(new_qd, buf).await {
        // We expect a successful completion for previous push().
        (_, OperationResult::Push) => Ok(()),
        (qd, OperationResult::Failed(e)) => {
            let cause: String = format!("failed to establish connection (qd={:?}, errno={:?})", qd, e);
            error!("send_ack(): {:?}", &cause);
            Err(e)
        },
        _ => {
            // The following statement is unreachable because we have issued a pop operation.
            // If we successfully complete a different operation, something really bad happen in the scheduler.
            unreachable!("unexpected operation on control duplex pipe")
        },
    }
}

/// Extracts port number from connect request ack message.
fn extract_port_number(buf: DemiBuffer) -> Result<u16, Fail> {
    match buf.len() {
        len if len == 0 => {
            let cause: String = format!("server closed connection");
            error!("extract_port_number(): {:?}", &cause);
            Err(Fail::new(libc::EBADF, &cause))
        },
        len if len == 2 => Ok(u16::from_ne_bytes(
            buf.to_vec().try_into().expect("should be the right size"),
        )),
        len => {
            let cause: String = format!("server sent invalid port number (len={:?})", len);
            error!("extract_port_number(): {:?}", &cause);
            Err(Fail::new(libc::EBADMSG, &cause))
        },
    }
}

/// Checks for a magic connect message.
fn get_connect_id(buf: DemiBuffer) -> Result<RequestId, Fail> {
    if buf.len() == mem::size_of::<RequestId>() {
        let ptr: *const u64 = buf.as_ptr() as *const u64;
        Ok(RequestId(unsafe { *ptr }))
    } else {
        let cause: String = format!("invalid connect request (len={:?})", buf.len());
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
