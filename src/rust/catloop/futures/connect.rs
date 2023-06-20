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
    scheduler::{
        TaskHandle,
        Yielder,
    },
    QToken,
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

pub async fn connect_coroutine(
    catmem: Rc<RefCell<CatmemLibOS>>,
    remote: SocketAddrV4,
    yielder: Yielder,
) -> Result<(SocketAddrV4, Rc<DuplexPipe>), Fail> {
    let ipv4: &Ipv4Addr = remote.ip();
    let port: u16 = remote.port().into();

    // Open connection to server.
    let control_duplex_pipe: Rc<DuplexPipe> = Rc::new(DuplexPipe::open_duplex_pipe(catmem.clone(), ipv4, port)?);
    // Gets the port for the new connection from the server by sending a connection request repeatedly until a port
    // comes back.
    let new_port: u16 = {
        let result: Result<u16, Fail> = get_port(catmem.clone(), control_duplex_pipe.clone(), &yielder).await;
        // Shut down control duplex pipe as we can open the new pipe now.
        control_duplex_pipe.shutdown()?;
        result?
    };

    // Open underlying pipes.
    let remote: SocketAddrV4 = SocketAddrV4::new(*ipv4, new_port);
    let new_pipe: Rc<DuplexPipe> = Rc::new(DuplexPipe::open_duplex_pipe(catmem.clone(), ipv4, new_port)?);
    // Send an ack to the server over the new pipe.
    send_ack(catmem.clone(), new_pipe.clone(), &yielder).await?;
    Ok((remote, new_pipe.clone()))
}

async fn send_connection_request(
    catmem: Rc<RefCell<CatmemLibOS>>,
    control_duplex_pipe: Rc<DuplexPipe>,
    yielder: &Yielder,
) -> Result<(), Fail> {
    // Create a message containing the magic number.
    let sga: demi_sgarray_t = CatloopLibOS::cook_magic_connect(&catmem)?;
    // Send to server.
    let qt: QToken = control_duplex_pipe.push(&sga)?;
    trace!("Send connection request qtoken={:?}", qt);
    // Get scheduler handle from the task id.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    let handle: TaskHandle = catmem_.schedule(qt)?;
    // Drop the mutable reference because we might yield.
    drop(catmem_);

    // Yield until push completes.
    while !handle.has_completed() {
        if let Err(e) = yielder.yield_once().await {
            return Err(e);
        }
    }
    // Re-acquire reference to catmem libos.
    catmem_ = catmem.borrow_mut();
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
    control_duplex_pipe: Rc<DuplexPipe>,
    yielder: &Yielder,
) -> Result<u16, Fail> {
    // Issue receive operation to wait for connect request ack.
    let size: usize = mem::size_of::<u16>();
    let qt: QToken = control_duplex_pipe.pop(Some(size))?;
    trace!("Read port qtoken={:?}", qt);

    // Get scheduler handle from the task id.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    let handle: TaskHandle = catmem_.schedule(qt)?;
    drop(catmem_);

    loop {
        // Send the connection request to the server.
        send_connection_request(catmem.clone(), control_duplex_pipe.clone(), &yielder).await?;

        // Wait on the pop for MAX_ACK_RECEIVED_ATTEMPTS
        if let Err(e) = yielder.yield_times(MAX_ACK_RECEIVED_ATTEMPTS).await {
            return Err(e);
        }

        // If we received a port back from the server, then unpack it. Otherwise, send the connection request again.
        if handle.has_completed() {
            // Re-acquire reference to catmem libos.
            catmem_ = catmem.borrow_mut();
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

async fn send_ack(catmem: Rc<RefCell<CatmemLibOS>>, new_pipe: Rc<DuplexPipe>, yielder: &Yielder) -> Result<(), Fail> {
    // Create message with magic connect.
    let sga: demi_sgarray_t = CatloopLibOS::cook_magic_connect(&catmem)?;
    // Send to server through new pipe.
    let qt: QToken = new_pipe.push(&sga)?;
    trace!("Send ack qtoken={:?}", qt);
    // Get scheduler handle from the task id.
    let mut catmem_: RefMut<CatmemLibOS> = catmem.borrow_mut();
    let handle: TaskHandle = catmem_.schedule(qt)?;
    // Drop the mutable reference because we might yield.
    drop(catmem_);

    // Yield until push completes.
    while !handle.has_completed() {
        if let Err(e) = yielder.yield_once().await {
            return Err(e);
        }
    }
    // Re-acquire reference to catmem libos.
    catmem_ = catmem.borrow_mut();
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
