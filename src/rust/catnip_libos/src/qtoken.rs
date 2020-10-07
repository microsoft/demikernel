use bytes::Bytes;
use catnip::{
    fail::Fail,
    protocols::tcp::peer::{
        AcceptFuture,
        ConnectFuture,
        PopFuture,
        PushFuture,
        SocketDescriptor,
    },
    runtime::Runtime,
};
use hashbrown::{
    hash_map::Entry,
    HashMap,
};
use std::{
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

#[derive(Debug)]
pub enum UserOperationResult {
    Connect,
    Accept(SocketDescriptor),
    Push,
    Pop(Bytes),
    Failed(Fail),
    InvalidToken,
}

impl UserOperationResult {
    fn connect(r: Result<SocketDescriptor, Fail>) -> Self {
        match r {
            Ok(..) => UserOperationResult::Connect,
            Err(e) => UserOperationResult::Failed(e),
        }
    }

    fn accept(r: Result<SocketDescriptor, Fail>) -> Self {
        match r {
            Ok(qd) => UserOperationResult::Accept(qd),
            Err(e) => UserOperationResult::Failed(e),
        }
    }

    fn push(r: Result<(), Fail>) -> Self {
        match r {
            Ok(()) => UserOperationResult::Push,
            Err(e) => UserOperationResult::Failed(e),
        }
    }

    fn pop(r: Result<Bytes, Fail>) -> Self {
        match r {
            Ok(r) => UserOperationResult::Pop(r),
            Err(e) => UserOperationResult::Failed(e),
        }
    }
}
