// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catloop::{
        CatloopLibOS,
        DuplexPipe,
    },
    catmem::CatmemLibOS,
    demi_sgarray_t,
    runtime::{
        fail::Fail,
        types::{
            demi_opcode_t,
            demi_qresult_t,
        },
    },
    scheduler::SchedulerHandle,
    QToken,
};
use ::std::{
    cell::RefCell,
    future::Future,
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    pin::Pin,
    rc::Rc,
    slice,
    task::{
        Context,
        Poll,
    },
};

//======================================================================================================================
// Enumerations
//======================================================================================================================

enum ServerState {
    ListenAndAccept {
        qt_rx: QToken,
    },
    Connect {
        qt_tx: QToken,
        duplex_pipe: Rc<DuplexPipe>,
    },
    Connected {
        qt_close: QToken,
        remote: SocketAddrV4,
        duplex_pipe: Rc<DuplexPipe>,
    },
}

//======================================================================================================================
// Structures
//======================================================================================================================

/// Descriptor for accept operation.
pub struct AcceptFuture {
    catmem: Rc<RefCell<CatmemLibOS>>,
    /// Local IPv4 address.
    ipv4: Ipv4Addr,
    /// Control duplex pipe used for establishing a the connection.
    control_duplex_pipe: Rc<DuplexPipe>,
    /// Port number new connection.
    new_port: u16,
    // State in the connection establishment protocol.
    state: ServerState,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(
        ipv4: &Ipv4Addr,
        catmem: Rc<RefCell<CatmemLibOS>>,
        control_duplex_pipe: Rc<DuplexPipe>,
        new_port: u16,
    ) -> Result<Self, Fail> {
        // Issue first pop. Note that we intentionally issue an unbound
        // pop() because the connection establishment protocol requires that
        // only one connection request is accepted at a time.
        let qt_rx: QToken = control_duplex_pipe.pop(None)?;
        Ok(AcceptFuture {
            catmem,
            ipv4: ipv4.clone(),
            control_duplex_pipe,
            new_port,
            state: ServerState::ListenAndAccept { qt_rx },
        })
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Future for AcceptFuture {
    type Output = Result<(SocketAddrV4, Rc<DuplexPipe>), Fail>;

    /// Polls the target [AcceptFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut AcceptFuture = self.get_mut();

        // Poll Catmem to make progress on ongoing operations.
        self_.catmem.borrow_mut().poll();

        // Act according to the state in the connection establishment protocol.
        match &self_.state {
            ServerState::ListenAndAccept { qt_rx } => listen_and_accept(self_, ctx, *qt_rx),
            ServerState::Connect { qt_tx, duplex_pipe } => connect(self_, ctx, *qt_tx, duplex_pipe.clone()),
            ServerState::Connected {
                qt_close,
                remote,
                duplex_pipe,
            } => {
                if let Some(handle) = DuplexPipe::poll(&self_.catmem, *qt_close)? {
                    check_connect_request(&self_.catmem, handle, *qt_close).expect("magic connect");
                    debug!("connection accepted!");
                    return Poll::Ready(Ok((*remote, duplex_pipe.clone())));
                }
                ctx.waker().wake_by_ref();
                return Poll::Pending;
            },
        }
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

// Checks if a connection request is valid by ensuring the following:
//   - The completed I/O queue operation associated to the queue token qt
//   concerns a pop() operation that has completed.
//   - The payload received from that pop() operation is a valid and legit MAGIC_CONNECT message.
fn check_connect_request(catmem: &Rc<RefCell<CatmemLibOS>>, handle: SchedulerHandle, qt: QToken) -> Result<(), Fail> {
    // Parse and check request.
    let passed: bool = {
        let qr: demi_qresult_t = catmem.borrow_mut().pack_result(handle, qt)?;
        let sga: demi_sgarray_t = match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            _ => panic!("unxpected operation on control duplex pipe"),
        };
        let passed: bool = CatloopLibOS::is_magic_connect(&sga);
        catmem.borrow_mut().free_sgarray(sga)?;
        passed
    };

    if !passed {
        let e: Fail = Fail::new(libc::EPROTO, "invalid request");
        error!("failed to establish connection ({:?})", e);
        Err(e)
    } else {
        Ok(())
    }
}

// Sends the port number to the peer process.
fn send_port_number(
    catmem: &Rc<RefCell<CatmemLibOS>>,
    control_duplex_pipe: Rc<DuplexPipe>,
    port: u16,
) -> Result<QToken, Fail> {
    let sga: demi_sgarray_t = catmem.borrow_mut().alloc_sgarray(mem::size_of_val(&port))?;
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.copy_from_slice(&port.to_ne_bytes());
    let qt_tx: QToken = control_duplex_pipe.push(&sga)?;
    catmem.borrow_mut().free_sgarray(sga)?;
    Ok(qt_tx)
}

/// Waits for a connection request to arrive.
fn listen_and_accept(
    self_: &mut AcceptFuture,
    ctx: &mut Context<'_>,
    qt_rx: QToken,
) -> Poll<Result<(SocketAddrV4, Rc<DuplexPipe>), Fail>> {
    // Check if a connection request arrived.
    if let Some(handle) = DuplexPipe::poll(&self_.catmem, qt_rx)? {
        // Check if this is a valid connection request.
        if !check_connect_request(&self_.catmem, handle, qt_rx).is_err() {
            // Create underlying pipes before sending the port number through the
            // control duplex pipe. This prevents us from running into a race
            // condition were the remote makes progress faster than us and attempts
            // to open the duplex pipe before it is created.
            let duplex_pipe: Rc<DuplexPipe> = Rc::new(DuplexPipe::create_duplex_pipe(
                self_.catmem.clone(),
                &self_.ipv4,
                self_.new_port,
            )?);

            // Send port number.
            let qt_tx: QToken = send_port_number(&self_.catmem, self_.control_duplex_pipe.clone(), self_.new_port)?;

            // Advance to next state in the connection establishment protocol.
            self_.state = ServerState::Connect {
                qt_tx,
                duplex_pipe: duplex_pipe.clone(),
            };
        } else {
            // Re-issue accept pop. Note that we intentionally issue an unbound
            // pop() because the connection establishment protocol requires that
            // only one connection request is accepted at a time.
            let qt_rx: QToken = self_.control_duplex_pipe.pop(None)?;
            self_.state = ServerState::ListenAndAccept { qt_rx };
        }
    }

    // Re-schedule co-routine for later execution.
    ctx.waker().wake_by_ref();
    return Poll::Pending;
}

// Waits for connect ack to be sent and advances to the connected state.
fn connect(
    self_: &mut AcceptFuture,
    ctx: &mut Context<'_>,
    qt_tx: QToken,
    duplex_pipe: Rc<DuplexPipe>,
) -> Poll<Result<(SocketAddrV4, Rc<DuplexPipe>), Fail>> {
    if let Some(_) = DuplexPipe::poll(&self_.catmem, qt_tx)? {
        let remote: SocketAddrV4 = SocketAddrV4::new(self_.ipv4, self_.new_port);
        let size: usize = mem::size_of_val(&CatloopLibOS::MAGIC_CONNECT);
        let qt_close: QToken = duplex_pipe.pop(Some(size))?;
        self_.state = ServerState::Connected {
            qt_close,
            remote,
            duplex_pipe: duplex_pipe.clone(),
        }
    }

    // Re-schedule co-routine for later execution.
    ctx.waker().wake_by_ref();
    return Poll::Pending;
}
