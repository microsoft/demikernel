use crate::{
    engine::{
        Engine,
        Protocol,
    },
    fail::Fail,
    file_table::FileDescriptor,
    interop::{
        dmtr_qresult_t,
        dmtr_sgarray_t,
    },
    protocols::ipv4::Endpoint,
    runtime::Runtime,
    scheduler::{
        Operation,
        SchedulerHandle,
    },
    operations::OperationResult,
};
use must_let::must_let;
use libc::c_int;
use std::{
    time::Instant,
};
use tracy_client::static_span;

const TIMER_RESOLUTION: usize = 64;
const MAX_RECV_ITERS: usize = 2;

pub type QToken = u64;

pub struct LibOS<RT: Runtime> {
    engine: Engine<RT>,
    rt: RT,

    ts_iters: usize,
}

impl<RT: Runtime> LibOS<RT> {
    pub fn new(rt: RT) -> Result<Self, Fail> {
        let engine = Engine::new(rt.clone())?;
        Ok(Self {
            engine,
            rt,
            ts_iters: 0,
        })
    }

    pub fn rt(&self) -> &RT {
        &self.rt
    }

    pub fn socket(
        &mut self,
        domain: c_int,
        socket_type: c_int,
        _protocol: c_int,
    ) -> Result<FileDescriptor, Fail> {
        if domain != libc::AF_INET {
            return Err(Fail::Invalid {
                details: "Invalid domain",
            });
        }
        let engine_protocol = match socket_type {
            libc::SOCK_STREAM => Protocol::Tcp,
            libc::SOCK_DGRAM => Protocol::Udp,
            _ => {
                return Err(Fail::Invalid {
                    details: "Invalid socket type",
                })
            },
        };
        Ok(self.engine.socket(engine_protocol))
    }

    pub fn bind(&mut self, fd: FileDescriptor, endpoint: Endpoint) -> Result<(), Fail> {
        self.engine.bind(fd, endpoint)
    }

    pub fn listen(&mut self, fd: FileDescriptor, backlog: usize) -> Result<(), Fail> {
        self.engine.listen(fd, backlog)
    }

    pub fn accept(&mut self, fd: FileDescriptor) -> u64 {
        let future = self.engine.accept(fd);
        self.rt.scheduler().insert(future).into_raw()
    }

    pub fn connect(&mut self, fd: FileDescriptor, remote: Endpoint) -> QToken {
        let future = self.engine.connect(fd, remote);
        self.rt.scheduler().insert(future).into_raw()
    }

    pub fn close(&mut self, fd: FileDescriptor) -> Result<(), Fail> {
        self.engine.close(fd)
    }

    pub fn push(&mut self, fd: FileDescriptor, sga: &dmtr_sgarray_t) -> QToken {
        let _s = static_span!();
        let buf = self.rt.clone_sgarray(sga);
        let future = self.engine.push(fd, buf);
        self.rt.scheduler().insert(future).into_raw()
    }

    pub fn push2(&mut self, fd: FileDescriptor, buf: RT::Buf) -> QToken {
        let future = self.engine.push(fd, buf);
        self.rt.scheduler().insert(future).into_raw()
    }

    pub fn pushto(&mut self, fd: FileDescriptor, sga: &dmtr_sgarray_t, to: Endpoint) -> QToken {
        let _s = static_span!();
        let buf = self.rt.clone_sgarray(sga);
        let future = self.engine.pushto(fd, buf, to);
        self.rt.scheduler().insert(future).into_raw()
    }

    pub fn pushto2(&mut self, fd: FileDescriptor, buf: RT::Buf, to: Endpoint) -> QToken {
        let future = self.engine.pushto(fd, buf, to);
        self.rt.scheduler().insert(future).into_raw()
    }

    pub fn drop_qtoken(&mut self, qt: QToken) {
        drop(self.rt.scheduler().from_raw_handle(qt).unwrap());
    }

    pub fn pop(&mut self, fd: FileDescriptor) -> QToken {
        let _s = static_span!();
        let future = self.engine.pop(fd);
        self.rt.scheduler().insert(future).into_raw()
    }

    // If this returns a result, `qt` is no longer valid.
    pub fn poll(&mut self, qt: QToken) -> Option<dmtr_qresult_t> {
        self.poll_bg_work();
        let handle = match self.rt.scheduler().from_raw_handle(qt) {
            None => {
                panic!("Invalid handle {}", qt);
            },
            Some(h) => h,
        };
        if !handle.has_completed() {
            handle.into_raw();
            return None;
        }
        let (qd, r) = self.take_operation(handle);
        Some(dmtr_qresult_t::pack(&self.rt, r, qd, qt))
    }

    pub fn wait(&mut self, qt: QToken) -> dmtr_qresult_t {
        let (qd, r) = self.wait2(qt);
        dmtr_qresult_t::pack(&self.rt, r, qd, qt)
    }

    pub fn wait2(&mut self, qt: QToken) -> (FileDescriptor, OperationResult<RT>) {
        let handle = self.rt.scheduler().from_raw_handle(qt).unwrap();
        loop {
            self.poll_bg_work();
            if handle.has_completed() {
                return self.take_operation(handle);
            }
        }

    }

    pub fn wait_all_pushes(&mut self, qts: &mut Vec<QToken>) {
        self.poll_bg_work();
        for qt in qts.drain(..) {
            let handle = self.rt.scheduler().from_raw_handle(qt).unwrap();
            assert!(handle.has_completed());
            must_let!(let (_, OperationResult::Push) = self.take_operation(handle));
        }
    }

    pub fn wait_any(&mut self, qts: &[QToken]) -> (usize, dmtr_qresult_t) {
        let _s = static_span!();
        loop {
            self.poll_bg_work();
            for (i, &qt) in qts.iter().enumerate() {
                let handle = self.rt.scheduler().from_raw_handle(qt).unwrap();
                if handle.has_completed() {
                    let (qd, r) = self.take_operation(handle);
                    return (i, dmtr_qresult_t::pack(&self.rt, r, qd, qt));
                }
                handle.into_raw();
            }
        }
    }

    pub fn wait_any2(&mut self, qts: &[QToken]) -> (usize, FileDescriptor, OperationResult<RT>) {
        loop {
            self.poll_bg_work();
            for (i, &qt) in qts.iter().enumerate() {
                let handle = self.rt.scheduler().from_raw_handle(qt).unwrap();
                if handle.has_completed() {
                    let (qd, r) = self.take_operation(handle);
                    return (i, qd, r);
                }
                handle.into_raw();
            }
        }
    }

    pub fn is_qd_valid(&self, fd: FileDescriptor) -> bool {
        self.engine.is_qd_valid(fd)
    }

    fn take_operation(&mut self, handle: SchedulerHandle) -> (FileDescriptor, OperationResult<RT>) {
        match self.rt.scheduler().take(handle) {
            Operation::Tcp(f) => f.expect_result(),
            Operation::Udp(f) => f.expect_result(),
            Operation::Background(..) => panic!("Polled background operation"),
        }
    }

    fn poll_bg_work(&mut self) {
        let _s = static_span!();
        self.rt.scheduler().poll();
        for _ in 0..MAX_RECV_ITERS {
            let batch = self.rt.receive();
            if batch.is_empty() {
                break;
            }
            for pkt in batch {
                if let Err(e) = self.engine.receive(pkt) {
                    warn!("Dropped packet: {:?}", e);
                }
            }
        }
        if self.ts_iters == 0 {
            let _t = static_span!("advance_clock");
            self.rt.advance_clock(Instant::now());
        }
        self.ts_iters = (self.ts_iters + 1) % TIMER_RESOLUTION;
    }
}
