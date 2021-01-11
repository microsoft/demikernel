use crate::{
    fail::Fail,
    file_table::FileDescriptor,
    protocols::ipv4,
    sync::Bytes,
};
use std::{
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

pub struct ResultFuture<F: Future> {
    pub future: F,
    pub done: Option<F::Output>,
}

impl<F: Future> ResultFuture<F> {
    pub fn new(future: F) -> Self {
        Self { future, done: None }
    }
}

impl<F: Future + Unpin> Future for ResultFuture<F>
where
    F::Output: Unpin,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let self_ = self.get_mut();
        if self_.done.is_some() {
            panic!("Polled after completion")
        }
        let result = match Future::poll(Pin::new(&mut self_.future), ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(r) => r,
        };
        self_.done = Some(result);
        Poll::Ready(())
    }
}

pub enum OperationResult {
    Connect,
    Accept(FileDescriptor),
    Push,
    Pop(Option<ipv4::Endpoint>, Bytes),
    Failed(Fail),
}
