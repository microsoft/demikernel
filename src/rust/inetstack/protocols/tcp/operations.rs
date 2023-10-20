// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::runtime::{
    fail::Fail,
    QDesc,
};
use ::std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

pub struct PushFuture {
    pub qd: QDesc,
    pub err: Option<Fail>,
}

impl fmt::Debug for PushFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PushFuture({:?})", self.qd)
    }
}

impl Future for PushFuture {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context) -> Poll<Self::Output> {
        match self.get_mut().err.take() {
            None => Poll::Ready(Ok(())),
            Some(e) => Poll::Ready(Err(e)),
        }
    }
}
