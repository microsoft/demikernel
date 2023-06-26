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
// Coroutine Functions
//======================================================================================================================

pub async fn accept_coroutine(
    ipv4: &Ipv4Addr,
    catmem: Rc<RefCell<CatmemLibOS>>,
    control_duplex_pipe: Rc<DuplexPipe>,
    new_port: u16,
    yielder: Yielder,
) -> Result<(SocketAddrV4, Rc<DuplexPipe>), Fail> {
    loop {
        // Grab next request from the control duplex pipe.
        let new_duplex_pipe: Rc<DuplexPipe> = match pop_magic_number(
            &catmem,
            control_duplex_pipe.clone(),
            Some(mem::size_of_val(&CatloopLibOS::MAGIC_CONNECT)),
            &yielder,
        )
        .await
        {
            // Recieved a valid magic number so create the new connection. This involves create the new duplex pipe
            // and sending the port number to the remote.
            Ok(true) => create_pipe(&catmem, control_duplex_pipe.clone(), ipv4, new_port, &yielder).await?,
            // Invalid request.
            Ok(false) => continue,
            // Some error.
            Err(e) => return Err(e),
        };

        // Check that the remote has retrieved the port number and responded with a magic number.
        match pop_magic_number(
            &catmem,
            new_duplex_pipe.clone(),
            Some(mem::size_of_val(&CatloopLibOS::MAGIC_CONNECT)),
            &yielder,
        )
        .await
        {
            // Valid response. Connection successfully established, so return new port and pipe to application.
            Ok(true) => return Ok((SocketAddrV4::new(ipv4.clone(), new_port), new_duplex_pipe.clone())),
            // Invalid reponse.
            Ok(false) => {
                // Clean up newly allocated duplex pipe.
                // FIXME: https://github.com/microsoft/demikernel/issues/778
                continue;
            },
            // Some error.
            Err(e) => return Err(e),
        };
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

// Gets the next connection request and checks if it is valid by ensuring the following:
//   - The completed I/O queue operation associated to the queue token qt
//   concerns a pop() operation that has completed.
//   - The payload received from that pop() operation is a valid and legit MAGIC_CONNECT message.
async fn pop_magic_number(
    catmem: &Rc<RefCell<CatmemLibOS>>,
    duplex_pipe: Rc<DuplexPipe>,
    bound: Option<usize>,
    yielder: &Yielder,
) -> Result<bool, Fail> {
    // Issue pop. Note that we intentionally issue an unbounded size
    // pop() because the connection establishment protocol requires that
    // only one connection request is accepted at a time.
    let qt: QToken = duplex_pipe.pop(bound)?;
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
    let passed: bool = CatloopLibOS::is_magic_connect(&sga);
    catmem_.free_sgarray(sga)?;
    if !passed {
        warn!("failed to establish connection (invalid request)");
    }

    Ok(passed)
}

// Sends the port number to the peer process.
async fn create_pipe(
    catmem: &Rc<RefCell<CatmemLibOS>>,
    control_duplex_pipe: Rc<DuplexPipe>,
    ipv4: &Ipv4Addr,
    port: u16,
    yielder: &Yielder,
) -> Result<Rc<DuplexPipe>, Fail> {
    // Create underlying pipes before sending the port number through the
    // control duplex pipe. This prevents us from running into a race
    // condition were the remote makes progress faster than us and attempts
    // to open the duplex pipe before it is created.
    let new_duplex_pipe: Rc<DuplexPipe> = Rc::new(DuplexPipe::create_duplex_pipe(catmem.clone(), ipv4, port)?);
    // Allocate a scatter-gather array and send the port number to the remote.
    let sga: demi_sgarray_t = catmem.borrow_mut().alloc_sgarray(mem::size_of_val(&port))?;
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.copy_from_slice(&port.to_ne_bytes());

    // Push the port number.
    let qt: QToken = control_duplex_pipe.push(&sga)?;
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
        demi_opcode_t::DEMI_OPC_PUSH => Ok(new_duplex_pipe),
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
