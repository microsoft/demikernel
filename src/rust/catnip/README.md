Catnip
=======
_Catnip_ is a TCP/IP stack written in [Rust](https://www.rust-lang.org/) that focuses on being an
embeddable, low-latency solution for user-space networking.

Development
----------
Be sure to have Cargo installed via [rustup](rustup.rs) and not a system package manager. Then, the
`rust-toolchain` file in the `rust/` directory will instruct rustup to install and use the pinned
Rust version.

Run library tests with `cargo test --lib`. The TCP stack has integration tests with a simulated
runtime under `src/protocols/tcp/tests`. There are also two test binaries, `tcp_loop` and
`udp_loop`, that simulate echo servers with the determinized runtime. They both take a
`SEND_RECV_ITERS` environment variable to support multiple echo iterations.

There's an initial benchmark `udp_echo` that simulates an echo server on two different threads. It
does not simulate DPDK, but it can be useful for profiling to find hotspots within our protocol
stack or scheduler.


Usage Statement
---------------
_catnip_ is prototype code. As such, we provide no guarantees that it will work and you are assuming
any risks with using the code.  We welcome comments and feedback. Please send any questions or
comments to _irene dot zhang at microsoft dot com_. By sending feedback, you are consenting to your
feedback being used in the further development of this project.
