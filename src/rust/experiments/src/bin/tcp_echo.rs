#![feature(try_blocks)]

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
    ExperimentConfig,
    Experiment,
    Latency,
};
use log::debug;

fn main() -> Result<(), Error> {
    load_mlx5_driver();

    let (config, runtime) = ExperimentConfig::initialize()?;
    let mut libos = LibOS::new(runtime)?;

    if env::var("ECHO_SERVER").is_ok() {
        let bind_addr = config.addr("server", "bind")?;

        let sockfd = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0)?;
        libos.bind(sockfd, bind_addr)?;
        libos.listen(sockfd, 10)?;

        let qtoken = libos.accept(sockfd);
        must_let!(let (_, OperationResult::Accept(fd)) = libos.wait2(qtoken));
        println!("Accepted connection!");

        let mut push_tokens = Vec::with_capacity(config.buffer_size / 1000 + 1);

        let (mut push, mut pop) = match config.experiment {
            Experiment::Continuous => (None, None),
            Experiment::Finite { num_iters } => {
                let push = Latency::new("push", num_iters);
                let pop = Latency::new("pop", num_iters);
                (Some(push), Some(pop))
            },
        };
        config.experiment.run(|stats| {
            let mut bytes_received = 0;
            while bytes_received < config.buffer_size {
                let r = pop.as_mut().map(|p| p.record());
                let qtoken = libos.pop(fd);
                drop(r);
                must_let!(let (_, OperationResult::Pop(_, buf)) = libos.wait2(qtoken));
                bytes_received += buf.len();
                let r = push.as_mut().map(|p| p.record());
                let qtoken = libos.push2(fd, buf);
                drop(r);
                push_tokens.push(qtoken);
            }
            assert_eq!(bytes_received, config.buffer_size);
            libos.wait_all_pushes(&mut push_tokens);
            assert_eq!(push_tokens.len(), 0);
            stats.report_bytes(bytes_received);
        });
        if let Some(push) = push {
            push.print();
        }
        if let Some(pop) = pop {
            pop.print();
        }

    }
    else if env::var("ECHO_CLIENT").is_ok() {
        let connect_addr = config.addr("client", "connect_to")?;

        let sockfd = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0)?;
        let qtoken = libos.connect(sockfd, connect_addr);
        must_let!(let (_, OperationResult::Connect) = libos.wait2(qtoken));

        let bufs = config.body_buffers(libos.rt(), 'a');
        let mut push_tokens = Vec::with_capacity(bufs.len());

        config.experiment.run(|stats| {
            assert!(push_tokens.is_empty());
            for b in &bufs {
                let qtoken = libos.push2(sockfd, b.clone());
                push_tokens.push(qtoken);
            }
            libos.wait_all_pushes(&mut push_tokens);
            debug!("Done pushing");

            let mut bytes_popped = 0;
            while bytes_popped < config.buffer_size {
                let qtoken = libos.pop(sockfd);
                must_let!(let (_, OperationResult::Pop(_, popped_buf)) = libos.wait2(qtoken));
                bytes_popped += popped_buf.len();
                if config.strict {
                    for &byte in &popped_buf[..] {
                        assert_eq!(byte, 'a' as u8);
                    }
                }
            }
            assert_eq!(bytes_popped, config.buffer_size);
            stats.report_bytes(bytes_popped);
        });
    }
    else {
        panic!("Set either ECHO_SERVER or ECHO_CLIENT");
    }

    Ok(())
}
