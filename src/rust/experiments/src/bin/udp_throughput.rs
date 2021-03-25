#![feature(maybe_uninit_uninit_array)]
#![feature(try_blocks)]

use histogram::Histogram;
use must_let::must_let;
use catnip::file_table::FileDescriptor;
use catnip_libos::runtime::DPDKRuntime;
use catnip_libos::memory::DPDKBuf;
use catnip::libos::QToken;
use anyhow::{
    Error,
};
use dpdk_rs::load_mlx5_driver;
use std::env;
use catnip::{
    libos::LibOS,
    operations::OperationResult,
    protocols::ip::Port,
};
use std::time::Instant;
use std::convert::TryFrom;
use experiments::{
    Experiment,
    ExperimentConfig,
    print_histogram,
};
use std::time::Duration;

struct ClientState {
    buffer_size: usize,
    next_buf: usize,
    buffers: Vec<DPDKBuf>,

    fd: FileDescriptor,
    bytes_received: usize,
    bytes_sent: usize,
    start: Instant,
}
impl ClientState {
    fn step(&mut self, libos: &mut LibOS<DPDKRuntime>, h: &mut Histogram) -> QToken {
        if self.bytes_sent < self.buffer_size {
            let i = self.next_buf;
            self.next_buf += 1;
            return libos.push2(self.fd, self.buffers[i].clone());
        }
        assert_eq!(self.bytes_sent, self.buffer_size);

        if self.bytes_received < self.buffer_size {
            return libos.pop(self.fd);
        }
        assert_eq!(self.bytes_received, self.buffer_size);

        h.increment(self.start.elapsed().as_nanos() as u64).unwrap();

        self.bytes_received = 0;
        self.bytes_sent = 0;
        self.next_buf = 0;
        self.start = Instant::now();
        return self.step(libos, h);
    }

    fn handle_push(&mut self) {
        let n = self.buffers[self.next_buf - 1].len();
        assert_eq!(self.bytes_received, 0);
        self.bytes_sent += n;
    }

    fn handle_pop(&mut self, bytes: DPDKBuf) {
        assert_eq!(self.bytes_sent, self.buffer_size);
        self.bytes_received += bytes.len();
    }
}

fn main() -> Result<(), Error> {
    load_mlx5_driver();

    let (config, runtime) = ExperimentConfig::initialize()?;
    let mut libos = LibOS::new(runtime)?;

    if env::var("ECHO_SERVER").is_ok() {
        let listen_addr = config.addr("server", "bind")?;

        let sockfd = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0)?;
        libos.bind(sockfd, listen_addr)?;

        let mut last_log = Instant::now();
        let mut num_bytes = 0;
        loop {
            let qtoken = libos.pop(sockfd);
            must_let!(let (_, OperationResult::Pop(Some(addr), buf)) = libos.wait2(qtoken));
            num_bytes += buf.len();
            let qtoken = libos.pushto2(sockfd, buf, addr);
            must_let!(let (_, OperationResult::Push) = libos.wait2(qtoken));

            if last_log.elapsed() > Duration::from_secs(3) {
                println!("Throughput: {} Gbps", Experiment::throughput_gbps(num_bytes, last_log.elapsed()));
                last_log = Instant::now();
                num_bytes = 0;
            }
        }
    } else if env::var("ECHO_CLIENT").is_ok() {
        let num_clients: usize = env::var("NUM_CLIENTS")?.parse()?;
        let connect_addr = config.addr("client", "connect_to")?;
        let client_addr = config.addr("client", "client")?;

        let bufs = config.body_buffers(libos.rt(), 'a');

        let mut clients = Vec::with_capacity(num_clients);
        let mut qtokens = Vec::with_capacity(num_clients);
        let mut h = Histogram::configure().precision(4).build().unwrap();
        for i in 0..num_clients {
            let sockfd = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0)?;
            let mut addr = client_addr.clone();
            let port: u16 = addr.port.into();
            addr.port = Port::try_from(port + i as u16)?;
            libos.bind(sockfd, addr)?;
            let qtoken = libos.connect(sockfd, connect_addr);
            must_let!(let (_, OperationResult::Connect) = libos.wait2(qtoken));
            let mut client = ClientState {
                buffer_size: config.buffer_size,
                next_buf: 0,
                buffers: bufs.clone(),

                fd: sockfd,
                bytes_received: 0,
                bytes_sent: 0,
                start: Instant::now(),
            };
            let qtoken = client.step(&mut libos, &mut h);
            qtokens.push(qtoken);
            clients.push(client);
        }
        let mut last_log = Instant::now();
        loop {
            let (i, _, result) = libos.wait_any2(&qtokens);
            match result {
                OperationResult::Push => {
                    clients[i].handle_push();
                    qtokens[i] = clients[i].step(&mut libos, &mut h);
                },
                OperationResult::Pop(_, bytes) => {
                    clients[i].handle_pop(bytes);
                    qtokens[i] = clients[i].step(&mut libos, &mut h);
                },
                _ => panic!("Unexpected result"),
            }

            if last_log.elapsed() > Duration::from_secs(3) {
                println!("Client latency:");
                print_histogram(&h);
                println!("");
                h.clear();
                last_log = Instant::now();
            }
        }

    } else {
        panic!("Set either ECHO_CLIENT or ECHO_SERVER");
    }
}
