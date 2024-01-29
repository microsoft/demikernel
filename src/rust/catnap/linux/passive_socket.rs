// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::get_libc_err,
    collections::async_queue::AsyncQueue,
    runtime::{
        fail::Fail,
        DemiRuntime,
    },
};
use ::socket2::Socket;
use ::std::net::SocketAddr;

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure represents the metadata for a passive listening socket: the socket itself and the queue of incoming connections.
pub struct PassiveSocketData {
    socket: Socket,
    accept_queue: AsyncQueue<Result<(Socket, SocketAddr), Fail>>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

/// A passive listening socket that polls for incoming connections and accepts them.
impl PassiveSocketData {
    pub fn new(socket: Socket) -> Self {
        Self {
            socket,
            accept_queue: AsyncQueue::default(),
        }
    }

    /// Handle an accept event notification by calling accept and putting the new connection in the accept queue.
    pub fn poll_accept(&mut self) {
        match self.socket.accept() {
            // Operation completed.
            Ok((new_socket, saddr)) => {
                trace!("connection accepted ({:?})", new_socket);
                let addr: SocketAddr = saddr.as_socket().expect("not a SocketAddrV4");
                self.accept_queue.push(Ok((new_socket, addr)))
            },
            Err(e) => {
                // Check the return error code.
                let errno: i32 = get_libc_err(e);
                if !DemiRuntime::should_retry(errno) {
                    let cause: String = format!("failed to accept on socket: {:?}", errno);
                    error!("poll_accept(): {}", cause);
                    self.accept_queue.push(Err(Fail::new(errno, &cause)));
                }
            },
        }
    }

    /// Block until a new connection arrives.
    pub async fn accept(&mut self) -> Result<(Socket, SocketAddr), Fail> {
        self.accept_queue.pop(None).await?
    }

    pub fn get_socket(&self) -> &Socket {
        &self.socket
    }

    pub fn get_mut_socket(&mut self) -> &mut Socket {
        &mut self.socket
    }
}
