use super::super::segment::TcpSegment;
use crate::{prelude::*, protocols::arp, r#async::Future};
use std::{any::Any, rc::Rc};

#[derive(Clone)]
pub struct TcpRuntime<'a> {
    arp: arp::Peer<'a>,
    rt: Runtime<'a>,
}

impl<'a> TcpRuntime<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> TcpRuntime<'a> {
        TcpRuntime { arp, rt }
    }

    pub fn rt(&self) -> &Runtime<'a> {
        &self.rt
    }

    pub fn cast(&self, mut segment: TcpSegment) -> Future<'a, ()> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        self.rt.start_coroutine(move || {
            trace!("TcpRuntime::cast({:?})", segment);
            let remote_ipv4_addr = segment.dest_ipv4_addr.unwrap();
            let remote_link_addr =
                r#await!(arp.query(remote_ipv4_addr), rt.now())?;
            let options = rt.options();
            segment.src_ipv4_addr = Some(options.my_ipv4_addr);
            segment.src_link_addr = Some(options.my_link_addr);
            segment.dest_link_addr = Some(remote_link_addr);
            rt.emit_effect(Effect::Transmit(Rc::new(segment.encode())));

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        })
    }
}
