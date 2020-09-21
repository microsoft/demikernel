mod background;
pub mod state;

use bytes::Bytes;
use std::task::{Poll, Context};
use std::future::Future;
use std::pin::Pin;
use crate::protocols::ipv4;
use crate::protocols::tcp::segment::{TcpSegment};
use crate::fail::Fail;
use std::rc::Rc;
use crate::protocols::tcp2::runtime::Runtime;
use std::time::Duration;
use self::state::ControlBlock;
use self::background::{background, BackgroundFuture};
use pin_project::pin_project;

#[pin_project]
pub struct EstablishedSocket<RT: Runtime> {
    pub cb: Rc<ControlBlock<RT>>,
    #[pin]
    background_work: BackgroundFuture<RT>,
}

impl<RT: Runtime> EstablishedSocket<RT> {
    pub fn new(cb: ControlBlock<RT>) -> Self {
        let cb = Rc::new(cb);
        Self {
            cb: cb.clone(),
            background_work: background(cb),
        }
    }

    pub fn receive_segment(&self, segment: TcpSegment) {
        self.cb.receive_segment(segment)
    }

    pub fn send(&self, buf: Bytes) -> Result<(), Fail> {
        self.cb.sender.send(buf)
    }

    pub fn peek(&self) -> Result<Bytes, Fail> {
        self.cb.receiver.peek()
    }

    pub fn recv(&self) -> Result<Option<Bytes>, Fail> {
        self.cb.receiver.recv()
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.cb.close()
    }

    pub fn remote_mss(&self) -> usize {
        self.cb.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.cb.current_rto()
    }

    pub fn endpoints(&self) -> (ipv4::Endpoint, ipv4::Endpoint) {
        (self.cb.local.clone(), self.cb.remote.clone())
    }
}

impl<RT: Runtime> Future for EstablishedSocket<RT> {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let self_ = self.project();
        assert!(Future::poll(self_.background_work, ctx).is_pending(), "TODO: Connection close");
        Poll::Pending
    }
}
