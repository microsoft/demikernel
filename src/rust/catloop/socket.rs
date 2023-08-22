// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::{
        queue::CatmemQueue,
        CatmemLibOS,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::socket::{
            operation::SocketOp,
            state::SocketStateMachine,
        },
        queue::{
            IoQueueTable,
            QDesc,
            QToken,
        },
        types::{
            demi_opcode_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        OperationResult,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    slice,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Maximum number of connection attempts.
/// This was chosen arbitrarily.
const MAX_ACK_RECEIVED_ATTEMPTS: usize = 1024;

/// Magic payload used to identify connect requests.  It must be a single
/// byte to ensure atomicity while keeping the connection establishment
/// protocol. The rationale for this lies on the fact that a pipe in Catmem
/// LibOS operates atomically on bytes. If we used a longer byte sequence,
/// we would need to introduce additional logic to make sure that
/// concurrent processes would not be enabled to establish a connection, if
/// they sent connection bytes in an interleaved, but legit order.
pub const MAGIC_CONNECT: u8 = 0x1b;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A socket.
pub struct Socket {
    /// The state of the socket.
    state: SocketStateMachine,
    /// Underlying Catmem LibOS.
    catmem: Rc<RefCell<CatmemLibOS>>,
    /// Underlying shared memory pipe.
    catmem_qd: Option<QDesc>,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Socket {
    /// Creates a new socket that is not bound to an address.
    pub fn new(catmem: Rc<RefCell<CatmemLibOS>>) -> Result<Self, Fail> {
        Ok(Self {
            state: SocketStateMachine::new_unbound(libc::SOCK_STREAM),
            catmem,
            catmem_qd: None,
            local: None,
            remote: None,
        })
    }

    /// Allocates a new socket that is bound to [local].
    fn alloc(
        catmem: Rc<RefCell<CatmemLibOS>>,
        catmem_qd: QDesc,
        local: Option<SocketAddrV4>,
        remote: Option<SocketAddrV4>,
    ) -> Self {
        Self {
            state: SocketStateMachine::new_connected(),
            catmem,
            catmem_qd: Some(catmem_qd),
            local,
            remote,
        }
    }

    /// Binds the target socket to `local` address.
    /// TODO: Should probably move the create of the duplex pipe to listen.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Bind)?;
        // Create underlying memory channels.
        let ipv4: &Ipv4Addr = local.ip();
        let port: u16 = local.port();
        self.catmem_qd = match self.catmem.borrow_mut().create_pipe(&format_pipe_str(ipv4, port)) {
            Ok(qd) => {
                self.state.commit();
                Some(qd)
            },
            Err(e) => {
                self.state.rollback();
                return Err(e);
            },
        };
        self.local = Some(local);
        Ok(())
    }

    /// Enables this socket to accept incoming connections.
    pub fn listen(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Listen)?;
        self.state.commit();
        Ok(())
    }

    pub fn accept<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state.prepare(SocketOp::Accept)?;
        self.do_generic_sync_control_path_call(coroutine, yielder)
    }

    /// Attempts to accept a new connection on this socket. On success, returns a new Socket for the accepted connection.
    pub async fn do_accept(
        socket: Rc<RefCell<Self>>,
        ipv4: &Ipv4Addr,
        new_port: u16,
        yielder: &Yielder,
    ) -> Result<Self, Fail> {
        socket.borrow_mut().state.prepare(SocketOp::Accepted)?;
        let catmem: Rc<RefCell<CatmemLibOS>> = socket.borrow().catmem.clone();
        let catmem_qd: QDesc = socket
            .borrow()
            .catmem_qd
            .expect("Must be a catmem queue in this state.");
        loop {
            // Grab next request from the control duplex pipe.
            let new_qd: QDesc = match pop_magic_number(catmem.clone(), catmem_qd, &yielder).await {
                // Received a valid magic number so create the new connection. This involves create the new duplex pipe
                // and sending the port number to the remote.
                Ok(true) => match create_pipe(catmem.clone(), catmem_qd, &ipv4, new_port, &yielder).await {
                    Ok(new_qd) => new_qd,
                    Err(e) => {
                        socket.borrow_mut().state.rollback();
                        return Err(e);
                    },
                },
                // Invalid request.
                Ok(false) => continue,
                // Some error.
                Err(e) => {
                    socket.borrow_mut().state.rollback();
                    return Err(e);
                },
            };

            let new_socket: Self = Self::alloc(catmem.clone(), new_qd, None, Some(SocketAddrV4::new(*ipv4, new_port)));

            // Check that the remote has retrieved the port number and responded with a magic number.
            match pop_magic_number(catmem.clone(), new_qd, &yielder).await {
                // Valid response. Connection successfully established, so return new port and pipe to application.
                Ok(true) => {
                    socket.borrow_mut().state.commit();
                    return Ok(new_socket);
                },
                // Invalid response.
                Ok(false) => {
                    // Clean up newly allocated duplex pipe.
                    catmem.borrow().close(new_qd)?;
                    continue;
                },
                // Some error.
                Err(e) => {
                    socket.borrow_mut().state.rollback();
                    return Err(e);
                },
            };
        }
    }

    pub fn connect<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state.prepare(SocketOp::Connect)?;
        self.do_generic_sync_control_path_call(coroutine, yielder)
    }

    /// Connects this socket to [remote].
    pub async fn do_connect(socket: Rc<RefCell<Self>>, remote: SocketAddrV4, yielder: &Yielder) -> Result<(), Fail> {
        socket.borrow_mut().state.prepare(SocketOp::Connected)?;
        let ipv4: &Ipv4Addr = remote.ip();
        let port: u16 = remote.port().into();
        let catmem: Rc<RefCell<CatmemLibOS>> = socket.borrow().catmem.clone();

        // Gets the port for the new connection from the server by sending a connection request repeatedly until a port
        // comes back.
        let result: Result<(QDesc, SocketAddrV4), Fail> = {
            let new_port: u16 = match get_port(catmem.clone(), ipv4, port, yielder).await {
                Ok(new_port) => new_port,
                Err(e) => {
                    socket.borrow_mut().state.rollback();
                    return Err(e);
                },
            };

            // Open underlying pipes.
            let remote: SocketAddrV4 = SocketAddrV4::new(*ipv4, new_port);
            let new_qd: QDesc = match catmem.borrow().open_pipe(&format_pipe_str(ipv4, new_port)) {
                Ok(new_qd) => new_qd,
                Err(e) => {
                    socket.borrow_mut().state.rollback();
                    return Err(e);
                },
            };
            // Send an ack to the server over the new pipe.
            if let Err(e) = send_ack(catmem.clone(), new_qd, &yielder).await {
                socket.borrow_mut().state.rollback();
                return Err(e);
            }
            Ok((new_qd, remote))
        };

        match result {
            Ok((new_qd, remote)) => {
                let mut socket: RefMut<Socket> = socket.borrow_mut();
                socket.state.commit();
                socket.catmem_qd = Some(new_qd);
                socket.remote = Some(remote);
                Ok(())
            },
            Err(e) => {
                socket.borrow_mut().state.rollback();
                Err(e)
            },
        }
    }

    /// Closes this socket.
    pub fn close(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Close)?;
        self.state.commit();
        self.state.prepare(SocketOp::Closed)?;
        if let Some(qd) = self.catmem_qd {
            match self.catmem.borrow_mut().close(qd) {
                Ok(()) => {
                    self.state.commit();
                    return Ok(());
                },
                Err(e) => {
                    self.state.rollback();
                    return Err(e);
                },
            }
        };
        self.state.commit();
        Ok(())
    }

    /// Asynchronously closes this socket by allocating a coroutine.
    pub fn async_close<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state.prepare(SocketOp::Close)?;
        Ok(self.do_generic_sync_control_path_call(coroutine, yielder)?)
    }

    /// Closes `socket`.
    pub async fn do_close(socket: Rc<RefCell<Self>>, yielder: Yielder) -> Result<(QDesc, OperationResult), Fail> {
        // Should we assert that we're stil in the close state?
        let qtable: Rc<RefCell<IoQueueTable<CatmemQueue>>> = socket.borrow().catmem.borrow().get_qtable().clone();
        if let Some(qd) = socket.borrow().catmem_qd {
            let queue: CatmemQueue = socket.borrow().catmem.borrow().get_queue(qd)?;
            socket.borrow_mut().state.prepare(SocketOp::Closed)?;

            match CatmemLibOS::close_coroutine(qtable, queue, qd, yielder).await {
                (qd, OperationResult::Close) => {
                    socket.borrow_mut().state.commit();
                    Ok((qd, OperationResult::Close))
                },
                (qd, OperationResult::Failed(e)) => {
                    // Where to revert to?
                    socket.borrow_mut().state.rollback();
                    Ok((qd, OperationResult::Failed(e)))
                },
                _ => panic!("Should not return anything other than close or fail"),
            }
        } else {
            // We know that the queue descriptor will be replaced so it doesn't matter what we put here.
            Ok((QDesc::from(QDesc::MAX), OperationResult::Close))
        }
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(
        &self,
        coroutine: F,
        yielder: Yielder,
    ) -> Result<TaskHandle, Fail> {
        coroutine(yielder)
    }

    /// Asynchronous code for pushing to the underlying Catmem transport.
    pub async fn do_push(
        socket: Rc<RefCell<Self>>,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> Result<(QDesc, OperationResult), Fail> {
        socket.borrow().state.may_push()?;
        // It is safe to unwrap here, because we have just checked for the socket state
        // and by construction it should be connected. If not, the socket state machine
        // was not correctly driven.
        let qd: QDesc = socket.borrow().catmem_qd.expect("socket should be connected");
        let queue: CatmemQueue = socket.borrow().catmem.borrow().get_queue(qd)?;
        Ok(CatmemLibOS::push_coroutine(qd, queue, buf, yielder).await)
    }

    /// Schedule a coroutine to pop from the underlying Catmem queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the pop completes.
    pub fn pop<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(
        &self,
        coroutine: F,
        yielder: Yielder,
    ) -> Result<TaskHandle, Fail> {
        coroutine(yielder)
    }

    /// Asynchronous code for popping from the underlying Catmem transport.
    pub async fn do_pop(
        socket: Rc<RefCell<Self>>,
        size: Option<usize>,
        yielder: Yielder,
    ) -> Result<(QDesc, OperationResult), Fail> {
        socket.borrow().state.may_pop()?;
        // It is safe to unwrap here, because we have just checked for the socket state
        // and by construction it should be connected. If not, the socket state machine
        // was not correctly driven.
        let qd: QDesc = socket.borrow().catmem_qd.expect("socket should be connected");
        let queue: CatmemQueue = socket.borrow().catmem.borrow().get_queue(qd)?;
        match CatmemLibOS::pop_coroutine(qd, queue, size, yielder).await {
            (qd, OperationResult::Pop(_, buf)) => Ok((qd, OperationResult::Pop(socket.borrow().remote(), buf))),
            (qd, result) => Ok((qd, result)),
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

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        // Spawn coroutine.
        match coroutine(yielder) {
            // We successfully spawned the coroutine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.state.commit();
                Ok(handle)
            },
            // We failed to spawn the coroutine.
            Err(e) => {
                // Abort the operation on the socket.
                self.state.abort();
                Err(e)
            },
        }
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

// Gets the next connection request and checks if it is valid by ensuring the following:
//   - The completed I/O queue operation associated to the queue token qt
//   concerns a pop() operation that has completed.
//   - The payload received from that pop() operation is a valid and legit MAGIC_CONNECT message.
async fn pop_magic_number(catmem: Rc<RefCell<CatmemLibOS>>, catmem_qd: QDesc, yielder: &Yielder) -> Result<bool, Fail> {
    // Issue pop. Each magic connect represents a separate connection request, so we always bound the pop.
    let qt: QToken = catmem.borrow().pop(catmem_qd, Some(mem::size_of_val(&MAGIC_CONNECT)))?;
    let handle: TaskHandle = {
        // Get scheduler handle from the task id.
        catmem.borrow().from_task_id(qt)?
    };
    // Yield until pop completes.
    while !handle.has_completed() {
        if let Err(e) = yielder.yield_once().await {
            return Err(e);
        }
    }
    // Re-acquire mutable reference.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    // Retrieve operation result and check if it is what we expect.
    let qr: demi_qresult_t = catmem_.pack_result(handle, qt)?;
    match qr.qr_opcode {
        // We expect a successful completion for previous pop().
        demi_opcode_t::DEMI_OPC_POP => {},
        // We may get some error.
        demi_opcode_t::DEMI_OPC_FAILED => {
            let cause: String = format!(
                "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                qr.qr_qd, qt, qr.qr_ret
            );
            error!("poll(): {:?}", &cause);
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
    let passed: bool = is_magic_connect(&sga);
    catmem_.free_sgarray(sga)?;
    if !passed {
        warn!("failed to establish connection (invalid request)");
    }

    Ok(passed)
}

// Sends the port number to the peer process.
async fn create_pipe(
    catmem: Rc<RefCell<CatmemLibOS>>,
    catmem_qd: QDesc,
    ipv4: &Ipv4Addr,
    port: u16,
    yielder: &Yielder,
) -> Result<QDesc, Fail> {
    // Create underlying pipes before sending the port number through the
    // control duplex pipe. This prevents us from running into a race
    // condition were the remote makes progress faster than us and attempts
    // to open the duplex pipe before it is created.
    let new_qd = catmem.borrow().create_pipe(&format_pipe_str(ipv4, port))?;
    // Allocate a scatter-gather array and send the port number to the remote.
    let sga: demi_sgarray_t = catmem.borrow_mut().alloc_sgarray(mem::size_of_val(&port))?;
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.copy_from_slice(&port.to_ne_bytes());

    // Push the port number.
    let qt: QToken = catmem.borrow().push(catmem_qd, &sga)?;
    let handle: TaskHandle = {
        // Get the task handle from the task id.
        catmem.borrow().from_task_id(qt)?
    };

    // Wait for push to complete.
    while !handle.has_completed() {
        if let Err(e) = yielder.yield_once().await {
            return Err(e);
        }
    }
    // Re-acquire mutable reference.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    // Free the scatter-gather array.
    catmem_.free_sgarray(sga)?;

    // Retrieve operation result and check if it is what we expect.
    let qr: demi_qresult_t = catmem_.pack_result(handle, qt)?;
    match qr.qr_opcode {
        // We expect a successful completion for previous push().
        demi_opcode_t::DEMI_OPC_PUSH => Ok(new_qd),
        // We may get some error.
        demi_opcode_t::DEMI_OPC_FAILED => {
            let cause: String = format!(
                "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                qr.qr_qd, qt, qr.qr_ret
            );
            error!("connect(): {:?}", &cause);
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
    catmem: Rc<RefCell<CatmemLibOS>>,
    connect_qd: QDesc,
    yielder: &Yielder,
) -> Result<(), Fail> {
    // Create a message containing the magic number.
    let sga: demi_sgarray_t = cook_magic_connect(catmem.clone())?;

    // Send to server.
    let qt: QToken = catmem.borrow().push(connect_qd, &sga)?;
    trace!("Send connection request qtoken={:?}", qt);
    let handle: TaskHandle = {
        // Get scheduler handle from the task id.
        catmem.borrow().from_task_id(qt)?
    };

    // Yield until push completes.
    while !handle.has_completed() {
        if let Err(e) = yielder.yield_once().await {
            return Err(e);
        }
    }
    // Re-acquire reference to catmem libos.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    // Free the message buffer.
    catmem_.free_sgarray(sga)?;
    // Get the result of the push.
    let qr: demi_qresult_t = catmem_.pack_result(handle, qt)?;
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
    catmem: Rc<RefCell<CatmemLibOS>>,
    ipv4: &Ipv4Addr,
    port: u16,
    yielder: &Yielder,
) -> Result<u16, Fail> {
    // Issue receive operation to wait for connect request ack.
    let size: usize = mem::size_of::<u16>();
    // Open connection to server.
    let connect_qd: QDesc = match catmem.borrow().open_pipe(&format_pipe_str(ipv4, port)) {
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

    let qt: QToken = catmem.borrow().pop(connect_qd, Some(size))?;
    trace!("Read port qtoken={:?}", qt);

    let handle: TaskHandle = {
        // Get scheduler handle from the task id.
        catmem.borrow().from_task_id(qt)?
    };

    loop {
        // Send the connection request to the server.
        send_connection_request(catmem.clone(), connect_qd, &yielder).await?;

        // Wait on the pop for MAX_ACK_RECEIVED_ATTEMPTS
        if let Err(e) = yielder.yield_times(MAX_ACK_RECEIVED_ATTEMPTS).await {
            return Err(e);
        }

        // If we received a port back from the server, then unpack it. Otherwise, send the connection request again.
        if handle.has_completed() {
            // Re-acquire reference to catmem libos.
            let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
            // Get the result of the pop.
            let qr: demi_qresult_t = catmem_.pack_result(handle, qt)?;
            match qr.qr_opcode {
                // We expect a successful completion for previous pop().
                demi_opcode_t::DEMI_OPC_POP => {
                    // Extract scatter-gather array from operation result.
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                    // Extract port number.
                    let port: Result<u16, Fail> = extract_port_number(&sga);
                    catmem_.free_sgarray(sga)?;
                    return port;
                },
                // We may get some error.
                demi_opcode_t::DEMI_OPC_FAILED => {
                    // Shut down control duplex pipe as we can open the new pipe now.
                    catmem_.shutdown(connect_qd)?;

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
async fn send_ack(catmem: Rc<RefCell<CatmemLibOS>>, new_qd: QDesc, yielder: &Yielder) -> Result<(), Fail> {
    // Create message with magic connect.
    let sga: demi_sgarray_t = cook_magic_connect(catmem.clone())?;
    // Send to server through new pipe.
    let qt: QToken = catmem.borrow().push(new_qd, &sga)?;
    trace!("Send ack qtoken={:?}", qt);
    let handle: TaskHandle = {
        // Get scheduler handle from the task id.
        catmem.borrow().from_task_id(qt)?
    };

    // Yield until push completes.
    while !handle.has_completed() {
        if let Err(e) = yielder.yield_once().await {
            return Err(e);
        }
    }
    // Re-acquire reference to catmem libos.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    // Free the message buffer.
    catmem_.free_sgarray(sga)?;
    // Retrieve operation result and check if it is what we expect.
    let qr: demi_qresult_t = catmem_.pack_result(handle, qt)?;

    match qr.qr_opcode {
        // We expect a successful completion for previous push().
        demi_opcode_t::DEMI_OPC_PUSH => Ok(()),
        // We may get some error.
        demi_opcode_t::DEMI_OPC_FAILED => {
            let cause: String = format!(
                "failed to establish connection (qd={:?}, qt={:?}, errno={:?})",
                qr.qr_qd, qt, qr.qr_ret
            );
            error!("send_ack: {:?}", &cause);
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
pub fn cook_magic_connect(catmem: Rc<RefCell<CatmemLibOS>>) -> Result<demi_sgarray_t, Fail> {
    let sga: demi_sgarray_t = catmem.borrow_mut().alloc_sgarray(mem::size_of_val(&MAGIC_CONNECT))?;

    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    unsafe {
        *ptr = MAGIC_CONNECT;
    }

    Ok(sga)
}

/// Extracts port number from connect request ack message.
fn extract_port_number(sga: &demi_sgarray_t) -> Result<u16, Fail> {
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    if len != 2 {
        let e: Fail = Fail::new(libc::EAGAIN, "handshake failed");
        error!("failed to establish connection ({:?})", e);
        return Err(e);
    }
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    let array: [u8; 2] = [slice[0], slice[1]];
    Ok(u16::from_ne_bytes(array))
}

/// Checks for a magic connect message.
pub fn is_magic_connect(sga: &demi_sgarray_t) -> bool {
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    if len == mem::size_of_val(&MAGIC_CONNECT) {
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        let bytes = MAGIC_CONNECT.to_ne_bytes();
        if slice[..] == bytes[..] {
            return true;
        }
    }

    false
}

fn format_pipe_str(ip: &Ipv4Addr, port: u16) -> String {
    format!("{}:{}", ip, port)
}
