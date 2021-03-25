#![feature(maybe_uninit_uninit_array)]
#![feature(try_blocks)]

use must_let::must_let;
use catnip_libos::memory::DPDKBuf;
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
};

fn main() -> Result<(), Error> {
    load_mlx5_driver();

    let (config, runtime) = ExperimentConfig::initialize()?;
    let mut libos = LibOS::new(runtime)?;

    if env::var("ECHO_SERVER").is_ok() {
        let listen_addr = config.addr("server", "bind")?;
        let client_addr = config.addr("server", "client")?;

        let sockfd = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0)?;
        libos.bind(sockfd, listen_addr)?;
        let qtoken = libos.connect(sockfd, client_addr);
        must_let!(let (_, OperationResult::Connect) = libos.wait2(qtoken));

        config.experiment.run(|stats| {
            let mut bytes_transferred = 0;

            while bytes_transferred < config.buffer_size {
                let qtoken = libos.pop(sockfd);
                must_let!(let (_, OperationResult::Pop(Some(addr), buf)) = libos.wait2(qtoken));
                bytes_transferred += buf.len();
                let qtoken = libos.pushto2(sockfd, buf, addr);
                must_let!(let (_, OperationResult::Push) = libos.wait2(qtoken));
            }
            assert_eq!(bytes_transferred, config.buffer_size);
            stats.report_bytes(bytes_transferred);
        });
    }
    else if env::var("ECHO_CLIENT").is_ok() {
        let connect_addr = config.addr("client", "connect_to")?;
        let client_addr = config.addr("client", "client")?;

        let sockfd = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0)?;
        libos.bind(sockfd, client_addr)?;
        let qtoken = libos.connect(sockfd, connect_addr);
        must_let!(let (_, OperationResult::Connect) = libos.wait2(qtoken));

        let num_bufs = (config.buffer_size - 1) / config.mss + 1;
        let mut bufs = Vec::with_capacity(num_bufs);

        for i in 0..num_bufs {
            let start = i * config.mss;
            let end = std::cmp::min(start + config.mss, config.buffer_size);
            let len = end - start;

            let mut pktbuf = libos.rt().alloc_body_mbuf();
            assert!(len <= pktbuf.len(), "len {} (from mss {}), pktbuf len {}", len, config.mss, pktbuf.len());

            let pktbuf_slice = unsafe { pktbuf.slice_mut() };
            for j in 0..len {
                pktbuf_slice[j] = 'a' as u8;
            }
            drop(pktbuf_slice);
            pktbuf.trim(pktbuf.len() - len);
            bufs.push(DPDKBuf::Managed(pktbuf));
        }

        let mut push_tokens = Vec::with_capacity(num_bufs);

        config.experiment.run(|stats| {
            assert!(push_tokens.is_empty());
            for b in &bufs {
                let qtoken = libos.push2(sockfd, b.clone());
                push_tokens.push(qtoken);
            }
            libos.wait_all_pushes(&mut push_tokens);

            let mut bytes_popped = 0;
            while bytes_popped < config.buffer_size {
                let qtoken = libos.pop(sockfd);
                must_let!(let (_, OperationResult::Pop(_, popped_buf)) = libos.wait2(qtoken));
                bytes_popped += popped_buf.len();
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
