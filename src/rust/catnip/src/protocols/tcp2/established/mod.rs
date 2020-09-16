mod background;
pub mod state;

use std::task::{Poll, Context};
use crate::protocols::tcp2::SeqNumber;
use crate::protocols::{arp, ip, ipv4};
use std::cmp;
use std::convert::TryInto;
use std::future::Future;
use std::pin::Pin;
use crate::collections::watched::WatchedValue;
use std::collections::VecDeque;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use futures::channel::mpsc;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use futures::FutureExt;
use futures::future::{self, Either};
use futures::StreamExt;
use crate::protocols::tcp2::peer::Runtime;

use self::state::ControlBlock;
use self::background::{background, BackgroundFuture};

pub struct EstablishedSocket<RT: Runtime> {
    pub cb: Rc<ControlBlock<RT>>,
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

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        unsafe {
            let self_ = self.get_unchecked_mut();
            assert!(Future::poll(Pin::new_unchecked(&mut self_.background_work), ctx).is_pending(), "TODO");
            Poll::Pending
        }
    }
}
