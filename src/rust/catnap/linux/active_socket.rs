// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::get_libc_err,
    collections::{
        async_queue::AsyncQueue,
        async_value::SharedAsyncValue,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        DemiRuntime,
    },
};
use ::socket2::Socket;
use ::std::{
    cmp::min,
    io,
    mem::MaybeUninit,
    net::SocketAddr,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure represents outgoing packets.
struct Outgoing {
    addr: Option<SocketAddr>,
    buf: DemiBuffer,
    result: SharedAsyncValue<Option<Result<(), Fail>>>,
}

/// This structure represents the metadata for an active established socket: the socket itself and the queue of
/// outgoing messages and incoming ones.
pub struct ActiveSocketData {
    socket: Socket,
    send_queue: AsyncQueue<Outgoing>,
    recv_queue: AsyncQueue<Result<(Option<SocketAddr>, DemiBuffer), Fail>>,
    closed: bool,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl ActiveSocketData {
    pub fn new(socket: Socket) -> Self {
        Self {
            socket,
            send_queue: AsyncQueue::default(),
            recv_queue: AsyncQueue::default(),
            closed: false,
        }
    }

    /// Polls the send queue on an outgoing epoll event and send out data if there is any pending. We use an empty
    /// buffer for write to indicate that we want to know when the socket is ready for writing but do not have data to
    /// write (i.e., to detect when connect finishes).
    pub fn poll_send(&mut self) {
        if let Some(Outgoing {
            addr,
            mut buf,
            mut result,
        }) = self.send_queue.try_pop()
        {
            // A dummy request to detect when the socket has connected.
            if buf.is_empty() {
                result.set(Some(Ok(())));
                return;
            }
            // Try to send the buffer.
            let io_result: Result<usize, io::Error> = match addr {
                Some(addr) => self.socket.send_to(&buf, &addr.clone().into()),
                None => self.socket.send(&buf),
            };
            match io_result {
                // Operation completed.
                Ok(nbytes) => {
                    trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                    buf.adjust(nbytes as usize)
                        .expect("OS should not have sent more bytes than in the buffer");
                    if buf.is_empty() {
                        // Done sending this buffer
                        result.set(Some(Ok(())));
                    } else {
                        // Only sent part of the buffer so try again later.
                        self.send_queue.push_front(Outgoing { addr, buf, result });
                    }
                },
                Err(e) => {
                    let errno: i32 = get_libc_err(e);
                    if DemiRuntime::should_retry(errno) {
                        // Put the buffer back and try again later.
                        self.send_queue.push_front(Outgoing { addr, buf, result });
                    } else {
                        let cause: String = format!("failed to send on socket: {:?}", errno);
                        error!("poll_send(): {}", cause);
                        result.set(Some(Err(Fail::new(errno, &cause))));
                    }
                },
            }
        }
    }

    /// Polls the socket for incoming data on an incoming epoll event. Inserts any received data into the incoming
    /// queue.
    /// TODO: Incoming queue should possibly be byte oriented.
    pub fn poll_recv(&mut self) {
        let mut buf: DemiBuffer = DemiBuffer::new(limits::POP_SIZE_MAX as u16);
        if self.closed {
            return;
        }
        match self
            .socket
            .recv_from(unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len()) })
        {
            // Operation completed.
            Ok((nbytes, socketaddr)) => {
                if let Err(e) = buf.trim(buf.len() - nbytes as usize) {
                    self.recv_queue.push(Err(e));
                } else {
                    trace!("data popped ({:?} bytes)", nbytes);
                    if buf.len() == 0 {
                        self.closed = true;
                    }
                    self.recv_queue.push(Ok((socketaddr.as_socket(), buf)));
                }
            },
            Err(e) => {
                let errno: i32 = get_libc_err(e);
                if !DemiRuntime::should_retry(errno) {
                    let cause: String = format!("failed to receive on socket: {:?}", errno);
                    error!("poll_recv(): {}", cause);
                    self.recv_queue.push(Err(Fail::new(errno, &cause)));
                }
            },
        }
    }

    /// Pushes data to the socket. Blocks until completion.
    pub async fn push(&mut self, addr: Option<SocketAddr>, buf: DemiBuffer) -> Result<(), Fail> {
        let mut result: SharedAsyncValue<Option<Result<(), Fail>>> = SharedAsyncValue::new(None);
        self.send_queue.push(Outgoing {
            addr,
            buf,
            result: result.clone(),
        });
        loop {
            match result.get() {
                Some(result) => return result,
                None => {
                    result.wait_for_change(None).await?;
                    continue;
                },
            }
        }
    }

    /// Pops data from the socket. Blocks until some data is found but does not wait until the buf has reached [size].
    pub async fn pop(&mut self, buf: &mut DemiBuffer, size: usize) -> Result<Option<SocketAddr>, Fail> {
        let (addr, mut incoming_buf): (Option<SocketAddr>, DemiBuffer) = self.recv_queue.pop(None).await??;
        // Figure out how much data we got.
        let bytes_read: usize = min(incoming_buf.len(), size);
        // Trim the buffer down to the amount that we received.
        buf.trim(buf.len() - bytes_read)
            .expect("DemiBuffer must be bigger than size");
        // Move it if the buffer isn't empty.
        if !incoming_buf.is_empty() {
            buf.copy_from_slice(&incoming_buf[0..bytes_read]);
        }
        // Trim off everything that we moved.
        incoming_buf
            .adjust(bytes_read)
            .expect("bytes_read will be less than incoming buf len because it is a min of incoming buf len and size ");
        // We didn't consume all of the incoming data.
        if !incoming_buf.is_empty() {
            self.recv_queue.push_front(Ok((addr, incoming_buf)));
        }
        Ok(addr)
    }

    pub fn get_socket(&self) -> &Socket {
        &self.socket
    }

    pub fn get_mut_socket(&mut self) -> &mut Socket {
        &mut self.socket
    }
}
