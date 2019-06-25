use super::{
    datagram::{
        Icmpv4Datagram, Icmpv4Echo, Icmpv4EchoMut,
        Icmpv4EchoOp, Icmpv4Type,
    },
};
use crate::{
    prelude::*,
    protocols::{arp, ipv4},
    r#async::{Future, WhenAny},
};
use std::{
    any::Any,
    cell::RefCell,
    collections::HashSet,
    convert::TryFrom,
    net::Ipv4Addr,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Icmpv4Peer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
    bg: WhenAny<'a, ()>,
    outstanding_requests: Rc<RefCell<HashSet<(u16, u16)>>>,
}

impl<'a> Icmpv4Peer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> Icmpv4Peer<'a> {
        Icmpv4Peer {
            rt,
            arp,
            bg: WhenAny::default(),
            outstanding_requests: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("Icmpv4Peer::receive(...)");
        let options = self.rt.options();
        let datagram = Icmpv4Datagram::try_from(datagram)?;
        assert_eq!(
            datagram.ipv4().frame().header().dest_addr(),
            options.my_link_addr
        );
        assert_eq!(datagram.ipv4().header().dest_addr(), options.my_ipv4_addr);

        match datagram.header().r#type()? {
            Icmpv4Type::EchoRequest => {
                let dest_ipv4_addr = datagram.ipv4().header().src_addr();
                let datagram = Icmpv4Echo::try_from(datagram)?;
                self.reply_to_ping(
                    dest_ipv4_addr,
                    datagram.id(),
                    datagram.seq_num(),
                );
            }
            Icmpv4Type::EchoReply => {
                let datagram = Icmpv4Echo::try_from(datagram)?;
                let id = datagram.id();
                let seq_num = datagram.seq_num();
                let mut outstanding_requests =
                    self.outstanding_requests.borrow_mut();
                outstanding_requests.remove(&(id, seq_num));
            }
        }

        Ok(())
    }

    pub fn poll(&mut self, now: Instant) -> Result<()> {
        self.bg.poll(now).map(|_| ())
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> Future<'a, Duration> {
        let timeout = timeout.unwrap_or_else(|| Duration::from_millis(5000));
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        let outstanding_requests = self.outstanding_requests.clone();
        self.rt.start_coroutine(move || {
            let t0 = rt.now();
            let options = rt.options();
            debug!("initiating ARP query");
            let fut = arp.query(dest_ipv4_addr);
            let dest_link_addr = {
                let dest_link_addr;
                loop {
                    let x = fut.poll(rt.now());
                    match x {
                        Ok(a) => {
                            debug!(
                                "ARP query complete ({} -> {})",
                                dest_ipv4_addr, a
                            );
                            dest_link_addr = a;
                            break;
                        }
                        Err(Fail::TryAgain {}) => {
                            yield None;
                            continue;
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }

                dest_link_addr
            };

            // todo: these parameters need to be more carefully selected.
            let id = 0;
            let seq_num = 0;

            let mut bytes = Icmpv4EchoMut::new_bytes();
            let mut echo = Icmpv4EchoMut::from_bytes(&mut bytes)?;
            echo.r#type(Icmpv4EchoOp::Request);
            echo.id(id);
            echo.seq_num(seq_num);
            let mut ipv4_header = echo.icmpv4().ipv4().header();
            ipv4_header.src_addr(options.my_ipv4_addr);
            ipv4_header.dest_addr(dest_ipv4_addr);
            let mut frame_header = echo.icmpv4().ipv4().frame().header();
            frame_header.dest_addr(dest_link_addr);
            frame_header.src_addr(options.my_link_addr);
            let _ = echo.seal()?;
            rt.emit_effect(Effect::Transmit(Rc::new(bytes)));

            let key = (id, seq_num);
            {
                let mut outstanding_requests =
                    outstanding_requests.borrow_mut();
                assert!(outstanding_requests.insert(key));
            }

            loop {
                yield None;

                let dt = rt.now() - t0;
                if dt >= timeout {
                    return Err(Fail::Timeout {});
                }

                {
                    let outstanding_requests = outstanding_requests.borrow();
                    if !outstanding_requests.contains(&key) {
                        let x: Rc<Any> = Rc::new(dt);
                        return Ok(x);
                    }
                }
            }
        })
    }

    fn reply_to_ping(
        &mut self,
        dest_ipv4_addr: Ipv4Addr,
        id: u16,
        seq_num: u16,
    ) {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        let fut = self.rt.start_coroutine(move || {
            let options = rt.options();
            debug!("initiating ARP query");
            let fut = arp.query(dest_ipv4_addr);
            let dest_link_addr = {
                let dest_link_addr;
                loop {
                    let x = fut.poll(rt.now());
                    match x {
                        Ok(a) => {
                            debug!(
                                "ARP query complete ({} -> {})",
                                dest_ipv4_addr, a
                            );
                            dest_link_addr = a;
                            break;
                        }
                        Err(Fail::TryAgain {}) => {
                            yield None;
                            continue;
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }

                dest_link_addr
            };

            let mut bytes = Icmpv4EchoMut::new_bytes();
            let mut echo = Icmpv4EchoMut::from_bytes(&mut bytes)?;
            echo.r#type(Icmpv4EchoOp::Reply);
            echo.id(id);
            echo.seq_num(seq_num);
            let ipv4 = echo.icmpv4().ipv4();
            let mut ipv4_header = ipv4.header();
            ipv4_header.src_addr(options.my_ipv4_addr);
            ipv4_header.dest_addr(dest_ipv4_addr);
            let frame = ipv4.frame();
            let mut frame_header = frame.header();
            frame_header.src_addr(options.my_link_addr);
            frame_header.dest_addr(dest_link_addr);

            let _ = echo.seal()?;
            rt.emit_effect(Effect::Transmit(Rc::new(bytes)));
            let x: Rc<Any> = Rc::new(());
            Ok(x)
        });

        self.bg.add_future(fut);
    }
}
