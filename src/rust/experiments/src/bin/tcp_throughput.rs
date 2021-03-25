#![feature(try_blocks)]

use std::collections::HashMap;
use must_let::must_let;
use anyhow::{
    Error,
};
use dpdk_rs::load_mlx5_driver;
use std::env;
use catnip::{
    libos::LibOS,
    operations::OperationResult,
};
use experiments::{
    Experiment,
    print_histogram,
    ExperimentConfig,
};
use log::debug;
use std::time::{Duration, Instant};
use histogram::Histogram;

fn main() -> Result<(), Error> {
    load_mlx5_driver();

    let (config, runtime) = ExperimentConfig::initialize()?;
    let mut libos = LibOS::new(runtime)?;

    if env::var("ECHO_SERVER").is_ok() {
        let bind_addr = config.addr("server", "bind")?;

        let sockfd = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0)?;
        libos.bind(sockfd, bind_addr)?;
        libos.listen(sockfd, 10)?;

        let mut qtokens = vec![libos.accept(sockfd)];
        let mut last_log = Instant::now();
        let mut num_bytes = 0;
        loop {
            if last_log.elapsed() > Duration::from_secs(3) {
                println!("Throughput: {} Gbps", Experiment::throughput_gbps(num_bytes, last_log.elapsed()));
                last_log = Instant::now();
                num_bytes = 0;
            }

            let (i, fd, result) = libos.wait_any2(&qtokens[..]);
            qtokens.swap_remove(i);

            if fd == sockfd {
                must_let!(let OperationResult::Accept(new_fd) = result);
                debug!("Accepting new connection: {}", new_fd);
                qtokens.push(libos.pop(new_fd));
            } else {
                match result {
                    OperationResult::Pop(_, buf) => {
                        debug!("Popped {} bytes off {}", buf.len(), fd);
                        num_bytes += buf.len();
                        qtokens.push(libos.push2(fd, buf));
                    },
                    OperationResult::Push => {
                        debug!("Finished pushing {}", fd);
                        qtokens.push(libos.pop(fd));
                    },
                    _ => panic!("Unexpected OperationResult"),
                }
            }
        }
    } else if env::var("ECHO_CLIENT").is_ok() {
        let num_clients: usize = env::var("NUM_CLIENTS")?.parse()?;
        let connect_addr = config.addr("client", "connect_to")?;

        let bufs = config.body_buffers(libos.rt(), 'a');
        assert_eq!(bufs.len(), 1, "TODO: Implement messages greater than MSS");
        let mut qtokens = vec![];

        for _ in 0..num_clients {
            let sockfd = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0)?;
            qtokens.push(libos.connect(sockfd, connect_addr));
        }

        let mut starts = HashMap::new();
        let mut h = Histogram::configure().precision(4).build().unwrap();
        let mut last_log = Instant::now();
        loop {
            if last_log.elapsed() > Duration::from_secs(3) {
                println!("Client latency:");
                print_histogram(&h);
                println!("");
                h.clear();
                last_log = Instant::now();
            }

            let (i, fd, result) = libos.wait_any2(&qtokens[..]);
            qtokens.swap_remove(i);

            match result {
                OperationResult::Connect => {
                    debug!("Finished connecting {}", fd);
                    starts.insert(fd, Instant::now());
                    qtokens.push(libos.push2(fd, bufs[0].clone()));
                },
                OperationResult::Push => {
                    debug!("Finished pushing on {}", fd);
                    qtokens.push(libos.pop(fd));
                },
                OperationResult::Pop(_, buf) => {
                    let start = starts.remove(&fd).expect("Popped without start time");
                    h.increment(start.elapsed().as_nanos() as u64).unwrap();
                    debug!("Received {} bytes on {}", buf.len(), fd);
                    starts.insert(fd, Instant::now());
                    qtokens.push(libos.push2(fd, bufs[0].clone()));
                },
                _ => panic!("Unexpected OperationResult"),
            }
        }

    } else {
        panic!("Set either ECHO_SERVER or ECHO_CLIENT");
    }
}
