// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        protocols::{
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            ip::IpProtocol,
            ipv4::Ipv4Header,
            tcp::segment::{
                TcpHeader,
                TcpOptions2,
                TcpSegment,
                MAX_TCP_OPTIONS,
            },
            udp::{
                UdpDatagram,
                UdpHeader,
            },
        },
        test_helpers::{
            self,
            engine::{
                SharedEngine,
                DEFAULT_TIMEOUT,
            },
            runtime::SharedTestRuntime,
        },
    },
    runtime::{
        memory::DemiBuffer,
        network::PacketBuf,
        OperationResult,
    },
    MacAddress,
    QDesc,
    QToken,
};
use anyhow::Result;
use nettest::glue::{
    AcceptArgs,
    BindArgs,
    CloseArgs,
    ConnectArgs,
    Event,
    ListenArgs,
    PacketDirection,
    PacketEvent,
    PushArgs,
    PushToArgs,
    SocketArgs,
    SyscallEvent,
    TcpPacket,
    UdpPacket,
};
use std::{
    collections::VecDeque,
    env,
    fs::{
        DirEntry,
        File,
    },
    io::{
        BufRead,
        BufReader,
    },
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    path::{
        self,
        Path,
        PathBuf,
    },
    time::Instant,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Maximum number of retries for a pop operation.
/// This value was empirically chosen so as to have operations to successfully complete.
const MAX_POP_RETRIES: usize = 5;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

#[test]
/// Runs the network test suite.
fn test_simulation() -> Result<()> {
    let verbose: bool = false;
    let local_mac: MacAddress = test_helpers::ALICE_MAC;
    let remote_mac: MacAddress = test_helpers::BOB_MAC;
    let local_port: u16 = 12345;
    let local_ephemeral_port: u16 = 49152;
    let local_ipv4: Ipv4Addr = test_helpers::ALICE_IPV4;
    let remote_port: u16 = 23456;
    let remote_ephemeral_port: u16 = 49152;
    let remote_ipv4: Ipv4Addr = test_helpers::BOB_IPV4;

    let input_path: String = match env::var("INPUT") {
        Ok(config_path) => config_path,
        Err(e) => {
            let cause: String = format!("missing INPUT environment variable (err={:?})", e);
            warn!("test_simulation(): {:?}", cause);
            anyhow::bail!(cause);
        },
    };

    let tests: Vec<String> = collect_tests(&input_path)?;

    if tests.is_empty() {
        let cause: String = format!("no tests found under {:?}", input_path);
        warn!("test_simulation(): {:?}", cause);
        return Ok(());
    }

    for test in &tests {
        eprintln!("running test case: {:?}", test);

        let mut simulation: Simulation = Simulation::new(
            test,
            &local_mac,
            local_port,
            local_ephemeral_port,
            &local_ipv4,
            &remote_mac,
            remote_port,
            remote_ephemeral_port,
            &remote_ipv4,
        )?;
        simulation.run(verbose)?;
    }

    Ok(())
}

// Collect all files under 'test_path'.
fn collect_tests(test_path: &str) -> Result<Vec<String>> {
    let mut files: Vec<String> = Vec::new();
    let path: &Path = path::Path::new(test_path);
    // Check if path is a directory.
    if path.is_dir() {
        // It is, so recursively collect all files under it.
        let mut directories: Vec<String> = Vec::new();
        directories.push(test_path.to_string());
        // Recurse through all directories.
        while directories.len() > 0 {
            let directory: String = directories.pop().unwrap();
            for entry in std::fs::read_dir(&directory)? {
                let entry: DirEntry = entry?;
                let path: PathBuf = entry.path();
                // Check if path is a directory.
                if path.is_dir() {
                    // It is, so add it to the list of directories to be processed.
                    directories.push(path.to_str().unwrap().to_string());
                } else {
                    // It is not a directory, so just add the file to the list of files.
                    let filename: String = path.to_str().unwrap().to_string();
                    if filename.ends_with(".pkt") {
                        files.push(filename);
                    }
                }
            }
        }
        files.sort();
    } else {
        // It is not a directory, so just add the file.
        let filename: String = path.to_str().unwrap().to_string();
        files.push(filename);
    }
    Ok(files)
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// A simulation of the network stack.
struct Simulation {
    protocol: Option<IpProtocol>,
    local_mac: MacAddress,
    local_sockaddr: SocketAddrV4,
    local_port: u16,
    remote_mac: MacAddress,
    remote_sockaddr: SocketAddrV4,
    remote_port: u16,
    local_qd: Option<(u32, QDesc)>,
    remote_qd: Option<(u32, Option<QDesc>)>,
    engine: SharedEngine,
    now: Instant,
    inflight: Option<QToken>,
    steps: Vec<String>,
}

impl Simulation {
    /// Creates a new simulation.
    pub fn new(
        filename: &str,
        local_mac: &MacAddress,
        local_port: u16,
        local_ephemeral_port: u16,
        local_ipv4: &Ipv4Addr,
        remote_mac: &MacAddress,
        remote_port: u16,
        remote_ephemeral_port: u16,
        remote_ipv4: &Ipv4Addr,
    ) -> Result<Simulation> {
        let now: Instant = Instant::now();

        let test_rig: SharedTestRuntime = SharedTestRuntime::new_test(now);
        let local: SharedEngine = SharedEngine::new(test_helpers::ALICE_CONFIG_PATH, test_rig, now)?;

        info!("Local: sockaddr={:?}, macaddr={:?}", local_ipv4, local_mac);
        info!("Remote: sockaddr={:?}, macaddr={:?}", remote_ipv4, remote_mac);

        let steps: Vec<String> = Self::read_input_file(&filename)?;
        Ok(Simulation {
            protocol: None,
            local_mac: local_mac.clone(),
            remote_mac: remote_mac.clone(),
            engine: local,
            now,
            local_qd: None,
            remote_qd: None,
            inflight: None,
            local_sockaddr: SocketAddrV4::new(local_ipv4.clone(), local_ephemeral_port),
            local_port,
            remote_sockaddr: SocketAddrV4::new(remote_ipv4.clone(), remote_ephemeral_port),
            remote_port,
            steps,
        })
    }

    /// Reads the input file.
    fn read_input_file(filename: &str) -> Result<Vec<String>> {
        let mut lines: Vec<String> = Vec::new();
        let file: File = File::open(filename)?;
        let reader: BufReader<File> = BufReader::new(file);

        // Read all lines of the input file.
        for line in reader.lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
        }

        Ok(lines)
    }

    /// Runs the simulation.
    pub fn run(&mut self, verbose: bool) -> Result<()> {
        // Process all lines of the source file.
        for step in &self.steps.clone() {
            if verbose {
                info!("Line: {:?}", step);
            }

            if let Some(event) = nettest::run_parser(&step, verbose)? {
                self.run_event(&event)?;
            }
        }

        // Ensure that there are no more events to be processed.
        let frames: VecDeque<DemiBuffer> = self.engine.pop_all_frames();
        if !frames.is_empty() {
            for frame in &frames {
                info!("run(): {:?}", frame);
            }
            anyhow::bail!("run(): unexpected outgoing frames");
        }

        Ok(())
    }

    /// Runs an event.
    fn run_event(&mut self, event: &Event) -> Result<()> {
        self.now += event.time;
        self.engine.advance_clock(self.now);

        match &event.action {
            nettest::glue::Action::SyscallEvent(syscall) => self.run_syscall(syscall)?,
            nettest::glue::Action::PacketEvent(packet) => self.run_packet(packet)?,
        }

        Ok(())
    }

    /// Runs a system call.
    #[allow(unused_variables)]
    fn run_syscall(&mut self, syscall: &SyscallEvent) -> Result<()> {
        info!("{:?}: {:?}", self.now, syscall);
        match &syscall.syscall {
            // Issue demi_socket().
            nettest::glue::DemikernelSyscall::Socket(args, ret) => self.run_socket_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Bind(args, ret) => self.run_bind_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Listen(args, ret) => self.run_listen_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Accept(args, fd) => self.run_accept_syscall(args, fd.clone())?,
            nettest::glue::DemikernelSyscall::Connect(args, ret) => self.run_connect_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Push(args, ret) => self.run_push_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::PushTo(args, ret) => self.run_pushto_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Pop(args, ret) => self.run_pop_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Wait(args, ret) => self.run_wait_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Close(args, ret) => self.run_close_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Unsupported => {
                error!("Unsupported syscall");
            },
        }

        Ok(())
    }

    /// Runs a packet.
    fn run_packet(&mut self, packet: &PacketEvent) -> Result<()> {
        info!("{:?}: {:?}", self.now, packet);
        match packet {
            nettest::glue::PacketEvent::Tcp(direction, tcp_packet) => match direction {
                PacketDirection::Incoming => self.run_incoming_packet(tcp_packet)?,
                PacketDirection::Outgoing => self.run_outgoing_packet(tcp_packet)?,
            },
            nettest::glue::PacketEvent::Udp(direction, udp_packet) => match direction {
                PacketDirection::Incoming => self.run_incoming_udp_packet(udp_packet)?,
                PacketDirection::Outgoing => self.run_outgoing_udp_packet(udp_packet)?,
            },
        }

        Ok(())
    }

    /// Runs a socket system call.
    fn run_socket_syscall(&mut self, args: &SocketArgs, ret: i32) -> Result<()> {
        // Check for unsupported socket domain.
        if args.domain != nettest::glue::SocketDomain::AF_INET {
            let cause: String = format!("unsupported domain socket domain (domain={:?})", args.domain);
            info!("run_socket_syscall(): {:?}", cause);
            anyhow::bail!(cause);
        }

        // Issue demi_socket().
        match args.typ {
            // TCP Socket.
            nettest::glue::SocketType::SOCK_STREAM => {
                // Check for unsupported socket protocol.
                if args.protocol != nettest::glue::SocketProtocol::IPPROTO_TCP {
                    let cause: String = format!("unsupported socket protocol (protocol={:?})", args.protocol);
                    info!("run_socket_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                }

                // Issue demi_socket().
                match self.engine.tcp_socket() {
                    Ok(qd) => {
                        self.local_qd = Some((ret as u32, qd));
                        self.protocol = Some(IpProtocol::TCP);
                        Ok(())
                    },
                    Err(err) if ret as i32 == err.errno => Ok(()),
                    _ => {
                        let cause: String = format!("unexpected return for socket syscall");
                        info!("run_socket_syscall(): ret={:?}", ret);
                        anyhow::bail!(cause);
                    },
                }
            },
            // UDP Socket.
            nettest::glue::SocketType::SOCK_DGRAM => {
                // Check for unsupported socket protocol.
                if args.protocol != nettest::glue::SocketProtocol::IPPROTO_UDP {
                    let cause: String = format!("unsupported socket protocol (protocol={:?})", args.protocol);
                    info!("run_socket_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                }

                // Issue demi_socket().
                match self.engine.udp_socket() {
                    Ok(qd) => {
                        self.local_qd = Some((ret as u32, qd));
                        self.remote_qd = Some((ret as u32, None));
                        self.protocol = Some(IpProtocol::UDP);
                        Ok(())
                    },
                    Err(err) if ret as i32 == err.errno => Ok(()),
                    _ => {
                        let cause: String = format!("unexpected return for socket syscall");
                        info!("run_socket_syscall(): ret={:?}", ret);
                        anyhow::bail!(cause);
                    },
                }
            },
        }
    }

    /// Runs a bind system call.
    fn run_bind_syscall(&mut self, args: &BindArgs, ret: i32) -> Result<()> {
        // Extract bind address.
        let local_addr: SocketAddrV4 = match args.addr {
            None => {
                self.local_sockaddr.set_port(self.local_port);
                self.local_sockaddr
            },

            // Custom bind address is not supported.
            Some(addr) => {
                let cause: String = format!("unsupported bind address (addr={:?})", addr);
                info!("run_bind_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract local queue descriptor.
        let local_qd: QDesc = match args.qd {
            Some(local_fd) => match self.local_qd {
                Some((fd, qd)) if fd == local_fd => qd,
                _ => {
                    let cause: String = format!("local queue descriptor mismatch");
                    info!("run_bind_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                },
            },
            None => {
                let cause: String = format!("local queue descriptor must have been previously assigned");
                info!("run_bind_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Issue demi_bind().
        match self.engine.tcp_bind(local_qd, local_addr) {
            Ok(()) if ret == 0 => Ok(()),
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for bind syscall");
                info!("run_bind_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a listen system call.
    fn run_listen_syscall(&mut self, args: &ListenArgs, ret: i32) -> Result<()> {
        // Check if backlog length was informed.
        let backlog: usize = match args.backlog {
            Some(backlog) => backlog,
            None => {
                let cause: String = format!("backlog length must be informed");
                info!("run_listen_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract local queue descriptor.
        let local_qd: QDesc = match args.qd {
            Some(local_fd) => match self.local_qd {
                Some((fd, qd)) if fd == local_fd => qd,
                _ => {
                    let cause: String = format!("local queue descriptor mismatch");
                    info!("run_listen_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                },
            },
            None => {
                let cause: String = format!("local queue descriptor must have been previously assigned");
                info!("run_listen_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Issue demi_listen().
        match self.engine.tcp_listen(local_qd, backlog) {
            Ok(()) if ret == 0 => Ok(()),
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for listen syscall");
                info!("run_listen_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs an accept system call.
    fn run_accept_syscall(&mut self, args: &AcceptArgs, ret: i32) -> Result<()> {
        // Extract local queue descriptor.
        let local_qd: QDesc = match args.qd {
            Some(local_fd) => match self.local_qd {
                Some((fd, qd)) if fd == local_fd => qd,
                _ => {
                    let cause: String = format!("local queue descriptor mismatch");
                    info!("run_accept_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                },
            },
            None => {
                let cause: String = format!("local queue descriptor must have been previously assigned");
                info!("run_accept_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Issue demi_accept().
        match self.engine.tcp_accept(local_qd) {
            Ok(accept_qt) => {
                self.remote_qd = Some((ret as u32, None));
                self.inflight = Some(accept_qt);
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for accept syscall");
                info!("run_accept_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a connect system call.
    fn run_connect_syscall(&mut self, args: &ConnectArgs, ret: i32) -> Result<()> {
        // Extract local queue descriptor.
        let local_qd: QDesc = match self.local_qd {
            Some((_, qd)) => qd,
            None => {
                let cause: String = format!("local queue descriptor must have been previously assigned");
                info!("run_connect_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract remote address.
        let remote_addr: SocketAddrV4 = match args.addr {
            None => {
                self.remote_sockaddr = SocketAddrV4::new(self.remote_sockaddr.ip().clone(), self.remote_port);
                self.remote_sockaddr
            },
            Some(addr) => {
                // Unsupported remote address.
                let cause: String = format!("unsupported remote address (addr={:?})", addr);
                info!("run_connect_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        match self.engine.tcp_connect(local_qd, remote_addr) {
            Ok(connect_qt) => {
                self.inflight = Some(connect_qt);
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for connect syscall");
                info!("run_accept_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a push system call.
    fn run_push_syscall(&mut self, args: &PushArgs, ret: i32) -> Result<()> {
        // Extract buffer length.
        let buf_len: u16 = match args.len {
            Some(len) => len.try_into()?,
            None => {
                let cause: String = format!("buffer length must be informed");
                info!("run_push_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract remote queue descriptor.
        let remote_qd: QDesc = match self.remote_qd {
            Some((_, qd)) => qd.unwrap(),
            None => {
                anyhow::bail!("remote queue descriptor must have been previously assigned");
            },
        };

        let buf: DemiBuffer = Self::cook_buffer(buf_len as usize, None);
        match self.engine.tcp_push(remote_qd, buf) {
            Ok(push_qt) => {
                self.inflight = Some(push_qt);
                // We need an extra poll because we now perform all work for the push inside the asynchronous coroutine.
                // TODO: Remove this once we separate the poll and advance clock functions.
                self.engine.poll();

                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for push syscall");
                info!("run_push_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a pop system call.
    fn run_pop_syscall(&mut self, args: &nettest::glue::PopArgs, ret: i32) -> Result<()> {
        // Extract remote queue descriptor.
        let remote_qd: QDesc = args.qd.into();

        match self.protocol {
            Some(IpProtocol::TCP) => match self.engine.tcp_pop(remote_qd) {
                Ok(pop_qt) => {
                    self.inflight = Some(pop_qt);
                    Ok(())
                },
                Err(err) if ret as i32 == err.errno => Ok(()),
                _ => {
                    let cause: String = format!("unexpected return for pop syscall");
                    info!("run_pop_syscall(): ret={:?}", ret);
                    anyhow::bail!(cause);
                },
            },
            Some(IpProtocol::UDP) => match self.engine.udp_pop(remote_qd) {
                Ok(pop_qt) => {
                    self.inflight = Some(pop_qt);
                    Ok(())
                },
                Err(err) if ret as i32 == err.errno => Ok(()),
                _ => {
                    let cause: String = format!("unexpected return for pop syscall");
                    info!("run_pop_syscall(): ret={:?}", ret);
                    anyhow::bail!(cause);
                },
            },
            _ => {
                let cause: String = format!("protocol must have been previously assigned");
                info!("run_pop_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        }
    }

    /// Emulates wait system call.
    fn run_wait_syscall(&mut self, args: &nettest::glue::WaitArgs, ret: i32) -> Result<()> {
        // Extract queue descriptor.
        let args_qd: QDesc = match args.qd {
            Some(qd) => QDesc::from(qd),
            None => {
                let cause: String = format!("queue descriptor must be informed");
                info!("run_wait_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        match self.operation_has_completed() {
            Ok((qd, qr)) if args_qd == qd => match qr {
                crate::OperationResult::Accept((remote_qd, remote_addr)) if ret == 0 => {
                    info!("connection accepted (qd={:?}, addr={:?})", qd, remote_addr);
                    self.remote_qd = Some((self.remote_qd.unwrap().0, Some(remote_qd)));
                    Ok(())
                },
                crate::OperationResult::Connect => {
                    info!("connection established as expected (qd={:?})", qd);
                    Ok(())
                },
                crate::OperationResult::Pop(_sockaddr, _data) => {
                    info!("pop completed as expected (qd={:?})", qd);
                    Ok(())
                },
                crate::OperationResult::Push => {
                    info!("push completed as expected (qd={:?})", qd);
                    Ok(())
                },
                crate::OperationResult::Close => {
                    info!("close completed as expected (qd={:?})", qd);
                    Ok(())
                },
                crate::OperationResult::Failed(e) if e.errno == ret as i32 => {
                    info!("operation failed as expected (qd={:?}, errno={:?})", qd, e.errno);
                    Ok(())
                },
                crate::OperationResult::Failed(e) => {
                    unreachable!("operation failed unexpectedly (qd={:?}, errno={:?})", qd, e.errno);
                },
                _ => unreachable!("unexpected operation has completed coroutine has completed"),
            },
            _ => unreachable!("no operation has completed coroutine has completed, but it should"),
        }
    }

    /// Runs a pushto system call.
    fn run_pushto_syscall(&mut self, args: &PushToArgs, ret: i32) -> Result<()> {
        // Extract buffer length.
        let buf_len: u16 = match args.len {
            Some(len) => len.try_into()?,
            None => {
                let cause: String = format!("buffer length must be informed");
                info!("run_pushto_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract remote address.
        let remote_addr: SocketAddrV4 = match args.addr {
            None => {
                self.remote_sockaddr = SocketAddrV4::new(self.remote_sockaddr.ip().clone(), self.remote_port);
                self.remote_sockaddr
            },
            Some(addr) => {
                // Unsupported remote address.
                let cause: String = format!("unsupported remote address (addr={:?})", addr);
                info!("run_pushto_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract remote queue descriptor.
        let remote_qd: QDesc = match args.qd {
            Some(qd) => qd.into(),
            None => {
                anyhow::bail!("remote queue descriptor must have been previously assigned");
            },
        };

        let buf: DemiBuffer = Self::cook_buffer(buf_len as usize, None);
        match self.engine.udp_pushto(remote_qd, buf, remote_addr) {
            Ok(push_qt) => {
                self.inflight = Some(push_qt);
                // We need an extra poll because we now perform all work for the push inside the asynchronous coroutine.
                // TODO: Remove this once we separate the poll and advance clock functions.
                self.engine.poll();

                Ok(())
            },
            _ => {
                let cause: String = format!("unexpected return for pushto syscall");
                info!("run_pushto_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    fn run_close_syscall(&mut self, args: &CloseArgs, ret: i32) -> Result<()> {
        // Extract queue descriptor.
        let args_qd: QDesc = args.qd.into();

        match self.engine.tcp_async_close(args_qd) {
            Ok(close_qt) => {
                self.inflight = Some(close_qt);
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for close syscall");
                error!("run_close_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    // Build options list.
    fn build_tcp_options(&self, options: &Vec<nettest::glue::TcpOption>) -> ([TcpOptions2; MAX_TCP_OPTIONS], usize) {
        let mut option_list: Vec<TcpOptions2> = Vec::new();

        // Convert options.
        for option in options {
            match option {
                nettest::glue::TcpOption::Noop => option_list.push(TcpOptions2::NoOperation),
                nettest::glue::TcpOption::Mss(mss) => option_list.push(TcpOptions2::MaximumSegmentSize(mss.clone())),
                nettest::glue::TcpOption::WindowScale(wscale) => {
                    option_list.push(TcpOptions2::WindowScale(wscale.clone()))
                },
                nettest::glue::TcpOption::SackOk => option_list.push(TcpOptions2::SelectiveAcknowlegementPermitted),
                nettest::glue::TcpOption::Timestamp(sender, echo) => option_list.push(TcpOptions2::Timestamp {
                    sender_timestamp: sender.clone(),
                    echo_timestamp: echo.clone(),
                }),
                nettest::glue::TcpOption::EndOfOptions => option_list.push(TcpOptions2::EndOfOptionsList),
            }
        }

        let num_options: usize = option_list.len();

        // Pad options list.
        while option_list.len() < MAX_TCP_OPTIONS {
            option_list.push(TcpOptions2::NoOperation);
        }

        (option_list.try_into().unwrap(), num_options)
    }

    /// Builds an Ethernet 2 header.
    fn build_ethernet_header(&self) -> Ethernet2Header {
        let (src_addr, dst_addr) = { (self.remote_mac, self.local_mac) };
        Ethernet2Header::new(dst_addr, src_addr, EtherType2::Ipv4)
    }

    /// Builds an IPv4 header.
    fn build_ipv4_header(&self, protocol: IpProtocol) -> Ipv4Header {
        let (src_addr, dst_addr) = {
            (
                self.remote_sockaddr.ip().to_owned(),
                self.local_sockaddr.ip().to_owned(),
            )
        };

        Ipv4Header::new(src_addr, dst_addr, protocol)
    }

    /// Builds a TCP header.
    fn build_tcp_header(&self, tcp_packet: &TcpPacket) -> TcpHeader {
        let (option_list, num_options): ([TcpOptions2; MAX_TCP_OPTIONS], usize) =
            self.build_tcp_options(&tcp_packet.options);

        let (src_port, dst_port) = { (self.remote_port, self.local_sockaddr.port()) };

        let ack_num = match tcp_packet.ack {
            Some(ack_num) => ack_num,
            None => 0,
        };

        TcpHeader {
            src_port,
            dst_port,
            seq_num: tcp_packet.seqnum.seq.into(),
            ack_num: ack_num.into(),
            ns: false,
            cwr: tcp_packet.flags.cwr,
            ece: tcp_packet.flags.ece,
            urg: tcp_packet.flags.urg,
            ack: tcp_packet.flags.ack,
            psh: tcp_packet.flags.psh,
            rst: tcp_packet.flags.rst,
            syn: tcp_packet.flags.syn,
            fin: tcp_packet.flags.fin,
            window_size: tcp_packet.win.unwrap() as u16,
            urgent_pointer: 0,
            num_options,
            option_list,
        }
    }

    /// Builds a UDP header.
    fn build_udp_header(&self, _udp_packet: &UdpPacket) -> UdpHeader {
        let (src_port, dest_port): (u16, u16) = { (self.remote_port, self.local_sockaddr.port()) };

        UdpHeader::new(src_port, dest_port)
    }

    /// Builds a TCP segment.
    fn build_tcp_segment(&self, tcp_packet: &TcpPacket) -> TcpSegment {
        // Create headers.
        let ethernet2_hdr: Ethernet2Header = self.build_ethernet_header();
        let ipv4_hdr: Ipv4Header = self.build_ipv4_header(IpProtocol::TCP);
        let tcp_hdr: TcpHeader = self.build_tcp_header(&tcp_packet);
        let data: Option<DemiBuffer> = if tcp_packet.seqnum.win > 0 {
            Some(Self::cook_buffer(tcp_packet.seqnum.win as usize, None))
        } else {
            None
        };

        TcpSegment {
            ethernet2_hdr,
            ipv4_hdr,
            tcp_hdr,
            data,
            tx_checksum_offload: false,
        }
    }

    /// Builds a UDP datagram.
    fn build_udp_datagram(&self, udp_packet: &UdpPacket) -> UdpDatagram {
        // Create headers.
        let ethernet2_hdr: Ethernet2Header = self.build_ethernet_header();
        let ipv4_hdr: Ipv4Header = self.build_ipv4_header(IpProtocol::UDP);
        let udp_hdr: UdpHeader = self.build_udp_header(&udp_packet);
        let data: DemiBuffer = Self::cook_buffer(udp_packet.len as usize, None);

        UdpDatagram::new(ethernet2_hdr, ipv4_hdr, udp_hdr, data, false)
    }

    /// Runs an incoming packet.
    fn run_incoming_packet(&mut self, tcp_packet: &TcpPacket) -> Result<()> {
        let segment: TcpSegment = self.build_tcp_segment(&tcp_packet);

        let buf: DemiBuffer = Self::serialize_segment(segment);
        self.engine.receive(buf)?;

        self.engine.poll();
        Ok(())
    }

    /// Runs an incoming packet.
    fn run_incoming_udp_packet(&mut self, udp_packet: &UdpPacket) -> Result<()> {
        let datagram: UdpDatagram = self.build_udp_datagram(&udp_packet);

        let buf: DemiBuffer = Self::serialize_datagram(datagram);
        self.engine.receive(buf)?;

        self.engine.poll();
        Ok(())
    }

    /// Checks an Ethernet 2 header.
    fn check_ethernet2_header(&self, eth2_header: &Ethernet2Header) -> Result<()> {
        crate::ensure_eq!(eth2_header.src_addr(), self.local_mac);
        crate::ensure_eq!(eth2_header.dst_addr(), self.remote_mac);
        crate::ensure_eq!(eth2_header.ether_type(), EtherType2::Ipv4);

        Ok(())
    }

    /// Checks an IPv4 header.
    fn check_ipv4_header(&self, ipv4_header: &Ipv4Header, protocol: IpProtocol) -> Result<()> {
        crate::ensure_eq!(ipv4_header.get_src_addr(), self.local_sockaddr.ip().to_owned());
        crate::ensure_eq!(ipv4_header.get_dest_addr(), self.remote_sockaddr.ip().to_owned());
        crate::ensure_eq!(ipv4_header.get_protocol(), protocol);

        Ok(())
    }

    /// Checks a TCP header.
    fn check_tcp_header(&self, tcp_header: &TcpHeader, tcp_packet: &TcpPacket) -> Result<()> {
        // Check if source port number matches what we expect.
        crate::ensure_eq!(tcp_header.src_port, self.local_sockaddr.port());
        // Check if destination port number matches what we expect.
        crate::ensure_eq!(tcp_header.dst_port, self.remote_port);
        // Check if sequence number matches what we expect.
        crate::ensure_eq!(tcp_header.seq_num, tcp_packet.seqnum.seq.into());
        // Check if acknowledgement number matches what we expect.
        if let Some(ack_num) = tcp_packet.ack {
            crate::ensure_eq!(tcp_header.ack, true);
            crate::ensure_eq!(tcp_header.ack_num, ack_num.into());
        }
        // Check if window size matches what we expect.
        if let Some(winsize) = tcp_packet.win {
            crate::ensure_eq!(tcp_header.window_size, winsize as u16);
        }
        // Check if flags matches what we expect.
        crate::ensure_eq!(tcp_header.cwr, tcp_packet.flags.cwr);
        crate::ensure_eq!(tcp_header.ece, tcp_packet.flags.ece);
        crate::ensure_eq!(tcp_header.urg, tcp_packet.flags.urg);
        crate::ensure_eq!(tcp_header.ack, tcp_packet.flags.ack);
        crate::ensure_eq!(tcp_header.psh, tcp_packet.flags.psh);
        crate::ensure_eq!(tcp_header.rst, tcp_packet.flags.rst);
        crate::ensure_eq!(tcp_header.syn, tcp_packet.flags.syn);
        crate::ensure_eq!(tcp_header.fin, tcp_packet.flags.fin);
        // Check if urgent pointer matches what we expect.
        crate::ensure_eq!(tcp_header.urgent_pointer, 0);
        // Check if options match what we expect.
        for i in 0..tcp_packet.options.len() {
            match tcp_packet.options[i] {
                nettest::glue::TcpOption::Noop => {
                    crate::ensure_eq!(tcp_header.option_list[i], TcpOptions2::NoOperation);
                },
                nettest::glue::TcpOption::Mss(mss) => {
                    crate::ensure_eq!(tcp_header.option_list[i], TcpOptions2::MaximumSegmentSize(mss));
                },
                nettest::glue::TcpOption::WindowScale(wscale) => {
                    crate::ensure_eq!(tcp_header.option_list[i], TcpOptions2::WindowScale(wscale));
                },
                nettest::glue::TcpOption::SackOk => {
                    crate::ensure_eq!(tcp_header.option_list[i], TcpOptions2::SelectiveAcknowlegementPermitted);
                },
                nettest::glue::TcpOption::Timestamp(sender, echo) => {
                    crate::ensure_eq!(
                        tcp_header.option_list[i],
                        TcpOptions2::Timestamp {
                            sender_timestamp: sender,
                            echo_timestamp: echo,
                        }
                    );
                },
                nettest::glue::TcpOption::EndOfOptions => {
                    crate::ensure_eq!(tcp_header.option_list[i], TcpOptions2::EndOfOptionsList);
                },
            }
        }

        Ok(())
    }

    /// Checks a UDP header.
    fn check_udp_header(&self, udp_header: &UdpHeader, _udp_packet: &UdpPacket) -> Result<()> {
        // Check if source port number matches what we expect.
        crate::ensure_eq!(udp_header.src_port(), self.local_sockaddr.port());
        // Check if destination port number matches what we expect.
        crate::ensure_eq!(udp_header.dest_port(), self.remote_port);

        Ok(())
    }

    /// Checks if an operation has completed.
    fn operation_has_completed(&mut self) -> Result<(QDesc, OperationResult)> {
        match self.inflight.take() {
            Some(qt) => Ok(self.engine.wait(qt, DEFAULT_TIMEOUT)?),
            None => anyhow::bail!("should have an inflight queue token"),
        }
    }

    /// Runs an outgoing TCP packet.
    fn run_outgoing_packet(&mut self, tcp_packet: &TcpPacket) -> Result<()> {
        let mut n: usize = 0;
        let frames: VecDeque<DemiBuffer> = loop {
            let frames: VecDeque<DemiBuffer> = self.engine.pop_all_frames();
            if frames.is_empty() {
                if n > MAX_POP_RETRIES {
                    anyhow::bail!("did not emit a frame after {:?} loops", MAX_POP_RETRIES);
                } else {
                    self.engine.poll();
                    n += 1;
                }
            } else {
                // FIXME: We currently do not support multi-frame segments.
                crate::ensure_eq!(frames.len(), 1);
                break frames;
            }
        };
        let bytes: &DemiBuffer = &frames[0];
        let (eth2_header, eth2_payload): (Ethernet2Header, DemiBuffer) = Ethernet2Header::parse(bytes.clone())?;
        self.check_ethernet2_header(&eth2_header)?;

        let (ipv4_header, ipv4_payload): (Ipv4Header, DemiBuffer) = Ipv4Header::parse(eth2_payload)?;
        self.check_ipv4_header(&ipv4_header, IpProtocol::TCP)?;

        let (tcp_header, tcp_payload): (TcpHeader, DemiBuffer) = TcpHeader::parse(&ipv4_header, ipv4_payload, true)?;
        crate::ensure_eq!(tcp_packet.seqnum.win as usize, tcp_payload.len());
        self.check_tcp_header(&tcp_header, &tcp_packet)?;

        Ok(())
    }

    /// Runs an outgoing UDP packet.
    fn run_outgoing_udp_packet(&mut self, udp_packet: &UdpPacket) -> Result<()> {
        let mut n: usize = 0;
        let frames: VecDeque<DemiBuffer> = loop {
            let frames: VecDeque<DemiBuffer> = self.engine.pop_all_frames();
            if frames.is_empty() {
                if n > MAX_POP_RETRIES {
                    anyhow::bail!("did not emit a frame after {:?} loops", MAX_POP_RETRIES);
                } else {
                    self.engine.poll();
                    n += 1;
                }
            } else {
                // FIXME: We currently do not support multi-frame segments.
                crate::ensure_eq!(frames.len(), 1);
                break frames;
            }
        };
        let bytes: &DemiBuffer = &frames[0];
        let (eth2_header, eth2_payload): (Ethernet2Header, DemiBuffer) = Ethernet2Header::parse(bytes.clone())?;
        self.check_ethernet2_header(&eth2_header)?;

        let (ipv4_header, ipv4_payload): (Ipv4Header, DemiBuffer) = Ipv4Header::parse(eth2_payload)?;
        self.check_ipv4_header(&ipv4_header, IpProtocol::UDP)?;

        let (udp_header, udp_payload): (UdpHeader, DemiBuffer) = UdpHeader::parse(&ipv4_header, ipv4_payload, true)?;
        crate::ensure_eq!(udp_packet.len as usize, udp_payload.len());
        self.check_udp_header(&udp_header, &udp_packet)?;

        Ok(())
    }

    /// Serializes a TCP segment.
    fn serialize_segment(mut pkt: TcpSegment) -> DemiBuffer {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();
        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);
        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }
        buf
    }

    /// Serializes a UDP datagram.
    fn serialize_datagram(mut pkt: UdpDatagram) -> DemiBuffer {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();
        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);
        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }
        buf
    }

    /// Cooks a buffer.
    fn cook_buffer(size: usize, stamp: Option<u8>) -> DemiBuffer {
        assert!(size < u16::MAX as usize);
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        for i in 0..size {
            buf[i] = stamp.unwrap_or(i as u8);
        }
        buf
    }
}
