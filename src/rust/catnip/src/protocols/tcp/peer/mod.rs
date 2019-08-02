mod isn_generator;
mod runtime;

#[cfg(test)]
mod tests;

use super::{
    connection::{TcpConnectionHandle, TcpConnectionId},
    segment::{TcpSegment, TcpSegmentDecoder},
};
use crate::{
    prelude::*,
    protocols::{arp, ip, ipv4},
    r#async::WhenAny,
};
use std::{
    cell::RefCell, convert::TryFrom, num::Wrapping, rc::Rc, time::Instant,
};

pub use runtime::TcpRuntime;

pub struct TcpPeer<'a> {
    async_work: Rc<RefCell<WhenAny<'a, ()>>>,
    tcp_rt: Rc<RefCell<TcpRuntime<'a>>>,
}

impl<'a> TcpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> TcpPeer<'a> {
        TcpPeer {
            async_work: Rc::new(RefCell::new(WhenAny::new())),
            tcp_rt: Rc::new(RefCell::new(TcpRuntime::new(rt, arp))),
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("TcpPeer::receive(...)");
        let decoder = TcpSegmentDecoder::try_from(datagram)?;
        let segment = TcpSegment::try_from(decoder)?;
        let local_ipv4_addr = segment.dest_ipv4_addr.unwrap();
        // i haven't yet seen anything that explicitly disallows categories of
        // IP addresses but it seems sensible to drop datagrams where the
        // source address does not really support a connection.
        let remote_ipv4_addr =
            segment.src_ipv4_addr.ok_or(Fail::Malformed {
                details: "source IPv4 address is missing",
            })?;
        if remote_ipv4_addr.is_broadcast()
            || remote_ipv4_addr.is_multicast()
            || remote_ipv4_addr.is_unspecified()
        {
            return Err(Fail::Malformed {
                details: "only unicast addresses are supported by TCP",
            });
        }

        let local_port = segment.dest_port.ok_or(Fail::Malformed {
            details: "destination port is zero",
        })?;

        let remote_port = segment.src_port.ok_or(Fail::Malformed {
            details: "source port is zero",
        })?;

        debug!("local_port => {:?}", local_port);
        if self.tcp_rt.borrow().is_port_open(local_port) {
            if segment.syn && !segment.ack && !segment.rst {
                self.async_work.borrow_mut().add(
                    TcpRuntime::new_passive_connection(&self.tcp_rt, segment),
                );
                return Ok(());
            }

            let cxnid = TcpConnectionId {
                local: ipv4::Endpoint::new(local_ipv4_addr, local_port),
                remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
            };

            self.tcp_rt.borrow_mut().enqueue_segment(cxnid, segment)?;
            return Ok(());
        }

        // `local_port` is not open; send the appropriate RST segment.
        let mut ack_num =
            segment.seq_num + Wrapping(u32::try_from(segment.payload.len())?);
        // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/TCP_IP+Illustrated,+Volume+1:+The+Protocols/9780132808200/ch13.html#ch13):
        // > Although there is no data in the arriving segment, the SYN
        // > bit logically occupies 1 byte of sequence number space;
        // > therefore, in this example the ACK number in the reset
        // > segment is set to the ISN, plus the data length (0), plus 1
        // > for the SYN bit.
        if segment.syn {
            ack_num += Wrapping(1);
        }

        self.async_work.borrow_mut().add(TcpRuntime::cast(
            &self.tcp_rt,
            TcpSegment::default()
                .dest_ipv4_addr(remote_ipv4_addr)
                .dest_port(remote_port)
                .src_port(local_port)
                .ack_num(ack_num)
                .rst(),
        ));
        Ok(())
    }

    pub fn connect(
        &self,
        remote_endpoint: ipv4::Endpoint,
    ) -> Future<'a, TcpConnectionHandle> {
        trace!("TcpPeer::connect({:?})", remote_endpoint);
        let rt = self.tcp_rt.borrow().rt().clone();
        let tcp_rt = self.tcp_rt.clone();
        let async_work = self.async_work.clone();
        rt.start_coroutine(move || {
            trace!("TcpPeer::connect({:?})::coroutine", remote_endpoint);
            let rt = tcp_rt.borrow().rt().clone();
            let options = rt.options();
            let cxnid = TcpConnectionId {
                local: ipv4::Endpoint::new(
                    options.my_ipv4_addr,
                    tcp_rt.borrow_mut().acquire_private_port()?,
                ),
                remote: remote_endpoint,
            };

            let error = match r#await!(
                TcpRuntime::new_active_connection(&tcp_rt, cxnid.clone()),
                rt.now()
            ) {
                Ok(cxnid) => {
                    let handle = {
                        let tcp_rt = tcp_rt.borrow();
                        let cxn = tcp_rt.get_connection(&cxnid).unwrap();
                        cxn.get_handle()
                    };

                    async_work.borrow_mut().add(
                        TcpRuntime::on_connection_established(&tcp_rt, cxnid),
                    );

                    return CoroutineOk(handle);
                }
                Err(e) => e,
            };

            let rst = match error {
                Fail::ConnectionRefused {} => false,
                _ => true,
            };

            let _ = r#await!(
                TcpRuntime::close_connection(&tcp_rt, cxnid.clone(), rst),
                rt.now()
            );

            let mut tcp_rt = tcp_rt.borrow_mut();
            tcp_rt.release_private_port(cxnid.local.port());
            Err(error)
        })
    }

    pub fn listen(&mut self, port: ip::Port) -> Result<()> {
        self.tcp_rt.borrow_mut().listen(port)
    }

    pub fn write(
        &mut self,
        handle: TcpConnectionHandle,
        bytes: IoVec,
    ) -> Result<()> {
        self.tcp_rt.borrow_mut().write(handle, bytes)
    }

    pub fn read(&mut self, handle: TcpConnectionHandle) -> Result<IoVec> {
        self.tcp_rt.borrow_mut().read(handle)
    }
}

impl<'a> Async<()> for TcpPeer<'a> {
    fn poll(&self, now: Instant) -> Option<Result<()>> {
        self.async_work.borrow().poll(now).map(|r| r.map(|_| ()))
    }
}
