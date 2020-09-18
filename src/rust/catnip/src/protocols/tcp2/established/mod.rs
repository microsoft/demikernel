mod background;
pub mod state;

use std::task::{Poll, Context};
use std::future::Future;
use std::pin::Pin;
use crate::protocols::tcp::segment::{TcpSegment};
use crate::fail::Fail;
use std::rc::Rc;
use crate::protocols::tcp2::runtime::Runtime;
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

    pub fn send(&self, buf: Vec<u8>) -> Result<(), Fail> {
        self.cb.sender.send(buf)
    }

    pub fn recv(&self) -> Result<Option<Vec<u8>>, Fail> {
        self.cb.receiver.recv()
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.cb.close()
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
