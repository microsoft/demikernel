mod background;
pub mod state;

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
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use futures::FutureExt;
use futures::future::{self, Either};
use futures::StreamExt;

use self::state::ControlBlock;
use self::background::{background, BackgroundFuture};

pub struct EstablishedSocket {
    cb: Rc<ControlBlock>,
    background_work: BackgroundFuture,
}

impl EstablishedSocket {
    pub fn new(cb: ControlBlock) -> Self {
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
