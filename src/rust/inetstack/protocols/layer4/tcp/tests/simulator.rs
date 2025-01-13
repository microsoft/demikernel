// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// TODO: Move this component to the `network-simulator` crate.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    ensure_eq,
    inetstack::{
        consts::MAX_HEADER_SIZE,
        protocols::{
            layer2::{EtherType2, Ethernet2Header},
            layer3::{ip::IpProtocol, ipv4::Ipv4Header},
            layer4::{
                tcp::header::{TcpHeader, TcpOptions2, MAX_TCP_OPTIONS},
                udp::header::UdpHeader,
            },
        },
        test_helpers::{self, engine::SharedEngine, physical_layer::SharedTestPhysicalLayer},
    },
    runtime::{memory::DemiBuffer, OperationResult},
    MacAddress, QDesc, QToken,
};
use ::anyhow::Result;
use ::std::{
    collections::VecDeque,
    env,
    fs::{DirEntry, File},
    io::{BufRead, BufReader},
    net::{Ipv4Addr, SocketAddrV4},
    path::{self, Path, PathBuf},
    time::Instant,
};
use network_simulator::glue::{
    AcceptArgs, Action, BindArgs, CloseArgs, ConnectArgs, DemikernelSyscall, Event, ListenArgs, PacketDirection,
    PacketEvent, PopArgs, PushArgs, PushToArgs, SocketArgs, SocketDomain, SocketProtocol, SocketType, SyscallEvent,
    TcpOption, TcpPacket, UdpPacket, WaitArgs,
};

//======================================================================================================================
// Tests
//======================================================================================================================

#[test]
fn test_run_simulation() -> Result<()> {
    let verbose: bool = false;
    let local_mac: MacAddress = test_helpers::ALICE_MAC;
    let remote_mac: MacAddress = test_helpers::BOB_MAC;
    let local_port: u16 = 12345;
    let local_ephemeral_port: u16 = 65535;
    let local_ipv4: Ipv4Addr = test_helpers::ALICE_IPV4;
    let remote_port: u16 = 23456;
    let remote_ephemeral_port: u16 = 65535;
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

fn collect_tests(test_path: &str) -> Result<Vec<String>> {
    let mut files: Vec<String> = Vec::new();
    let path: &Path = path::Path::new(test_path);

    if path.is_dir() {
        let mut directories: Vec<String> = Vec::new();
        directories.push(test_path.to_string());

        // Recurse through all directories.
        while directories.len() > 0 {
            let directory: String = directories.pop().unwrap();
            for entry in std::fs::read_dir(&directory)? {
                let entry: DirEntry = entry?;
                let path: PathBuf = entry.path();

                if path.is_dir() {
                    // It is a directory, so add it to the list of directories to be processed.
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
    inflight: VecDeque<QToken>,
    lines: Vec<String>,
}

impl Simulation {
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

        let test_rig: SharedTestPhysicalLayer = SharedTestPhysicalLayer::new_test(now);
        let local: SharedEngine = SharedEngine::new(test_helpers::ALICE_CONFIG_PATH, test_rig, now)?;

        info!("Local: sockaddr={:?}, macaddr={:?}", local_ipv4, local_mac);
        info!("Remote: sockaddr={:?}, macaddr={:?}", remote_ipv4, remote_mac);

        let lines: Vec<String> = Self::read_input_file(&filename)?;
        Ok(Simulation {
            protocol: None,
            local_mac: local_mac.clone(),
            remote_mac: remote_mac.clone(),
            engine: local,
            now,
            local_qd: None,
            remote_qd: None,
            inflight: VecDeque::with_capacity(4),
            local_sockaddr: SocketAddrV4::new(local_ipv4.clone(), local_ephemeral_port),
            local_port,
            remote_sockaddr: SocketAddrV4::new(remote_ipv4.clone(), remote_ephemeral_port),
            remote_port,
            lines,
        })
    }

    fn read_input_file(filename: &str) -> Result<Vec<String>> {
        let mut lines: Vec<String> = Vec::new();
        let file: File = File::open(filename)?;
        let reader: BufReader<File> = BufReader::new(file);

        for line in reader.lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
        }

        Ok(lines)
    }

    pub fn run(&mut self, verbose: bool) -> Result<()> {
        for line in &self.lines.clone() {
            if verbose {
                info!("Line: {:?}", line);
            }

            if let Some(event) = network_simulator::run_parser(&line, verbose)? {
                self.run_event(&event)?;
            }
        }

        // Ensure that there are no more events to be processed.
        let frames: VecDeque<DemiBuffer> = self.engine.pop_expected_frames(0);
        if !frames.is_empty() {
            for frame in &frames {
                info!("run(): {:?}", frame);
            }
            anyhow::bail!("run(): unexpected outgoing frames");
        }

        Ok(())
    }

    fn run_event(&mut self, event: &Event) -> Result<()> {
        self.now += event.time;
        self.engine.advance_clock(self.now);

        match &event.action {
            Action::SyscallEvent(syscall) => self.run_syscall(syscall)?,
            Action::PacketEvent(packet) => self.run_packet(packet)?,
        }

        Ok(())
    }

    #[allow(unused_variables)]
    fn run_syscall(&mut self, syscall: &SyscallEvent) -> Result<()> {
        info!("{:?}: {:?}", self.now, syscall);
        match &syscall.syscall {
            DemikernelSyscall::Socket(args, ret) => self.run_socket_syscall(args, ret.clone())?,
            DemikernelSyscall::Bind(args, ret) => self.run_bind_syscall(args, ret.clone())?,
            DemikernelSyscall::Listen(args, ret) => self.run_listen_syscall(args, ret.clone())?,
            DemikernelSyscall::Accept(args, fd) => self.run_accept_syscall(args, fd.clone())?,
            DemikernelSyscall::Connect(args, ret) => self.run_connect_syscall(args, ret.clone())?,
            DemikernelSyscall::Push(args, ret) => self.run_push_syscall(args, ret.clone())?,
            DemikernelSyscall::PushTo(args, ret) => self.run_pushto_syscall(args, ret.clone())?,
            DemikernelSyscall::Pop(args, ret) => self.run_pop_syscall(args, ret.clone())?,
            DemikernelSyscall::Wait(args, ret) => self.run_wait_syscall(args, ret.clone())?,
            DemikernelSyscall::Close(args, ret) => self.run_close_syscall(args, ret.clone())?,
            DemikernelSyscall::Unsupported => {
                error!("Unsupported syscall");
            },
        }

        Ok(())
    }

    fn run_packet(&mut self, packet: &PacketEvent) -> Result<()> {
        info!("{:?}: {:?}", self.now, packet);
        match packet {
            PacketEvent::Tcp(direction, tcp_packet) => match direction {
                PacketDirection::Incoming => self.run_incoming_packet(tcp_packet)?,
                PacketDirection::Outgoing => self.run_outgoing_packet(tcp_packet)?,
            },
            PacketEvent::Udp(direction, udp_packet) => match direction {
                PacketDirection::Incoming => self.run_incoming_udp_packet(udp_packet)?,
                PacketDirection::Outgoing => self.run_outgoing_udp_packet(udp_packet)?,
            },
        }

        Ok(())
    }

    fn run_socket_syscall(&mut self, args: &SocketArgs, ret: i32) -> Result<()> {
        // Check for unsupported socket domain.
        if args.domain != SocketDomain::AF_INET {
            let cause: String = format!("unsupported domain socket domain (domain={:?})", args.domain);
            info!("run_socket_syscall(): {:?}", cause);
            anyhow::bail!(cause);
        }

        match args.typ {
            // TCP Socket
            SocketType::SOCK_STREAM => {
                // Check for unsupported socket protocol
                if args.protocol != SocketProtocol::IPPROTO_TCP {
                    let cause: String = format!("unsupported socket protocol (protocol={:?})", args.protocol);
                    info!("run_socket_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                }

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
            // UDP Socket
            SocketType::SOCK_DGRAM => {
                // Check for unsupported socket protocol
                if args.protocol != SocketProtocol::IPPROTO_UDP {
                    let cause: String = format!("unsupported socket protocol (protocol={:?})", args.protocol);
                    info!("run_socket_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                }

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

    fn run_bind_syscall(&mut self, args: &BindArgs, ret: i32) -> Result<()> {
        let local_bind_addr: SocketAddrV4 = match args.addr {
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

        match self.engine.tcp_bind(local_qd, local_bind_addr) {
            Ok(()) if ret == 0 => Ok(()),
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for bind syscall");
                info!("run_bind_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    fn run_listen_syscall(&mut self, args: &ListenArgs, ret: i32) -> Result<()> {
        let backlog: usize = match args.backlog {
            Some(backlog) => backlog,
            None => {
                let cause: String = format!("backlog length must be informed");
                info!("run_listen_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

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

    fn run_accept_syscall(&mut self, args: &AcceptArgs, ret: i32) -> Result<()> {
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

        match self.engine.tcp_accept(local_qd) {
            Ok(accept_qt) => {
                self.remote_qd = Some((ret as u32, None));
                self.inflight.push_back(accept_qt);
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

    fn run_connect_syscall(&mut self, args: &ConnectArgs, ret: i32) -> Result<()> {
        let local_qd: QDesc = match self.local_qd {
            Some((_, qd)) => qd,
            None => {
                let cause: String = format!("local queue descriptor must have been previously assigned");
                info!("run_connect_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

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
                self.inflight.push_back(connect_qt);
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

    fn run_push_syscall(&mut self, args: &PushArgs, ret: i32) -> Result<()> {
        let buf_len: usize = match args.len {
            Some(len) => len.try_into()?,
            None => {
                let cause: String = format!("buffer length must be informed");
                info!("run_push_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        let syscall_qd: QDesc = match args.qd {
            Some(qd) => qd.into(),
            None => {
                anyhow::bail!("remote queue descriptor must have been previously assigned");
            },
        };

        let buf: DemiBuffer = Self::prepare_dummy_buffer(buf_len, None);
        match self.engine.tcp_push(syscall_qd, buf) {
            Ok(push_qt) => {
                self.inflight.push_back(push_qt);
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

    fn run_pop_syscall(&mut self, args: &PopArgs, ret: i32) -> Result<()> {
        let remote_qd: QDesc = args.qd.into();

        match self.protocol {
            Some(IpProtocol::TCP) => match self.engine.tcp_pop(remote_qd) {
                Ok(pop_qt) => {
                    self.inflight.push_back(pop_qt);
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
                    self.inflight.push_back(pop_qt);
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

    fn run_wait_syscall(&mut self, args: &WaitArgs, ret: i32) -> Result<()> {
        let args_qd: QDesc = match args.qd {
            Some(qd) => QDesc::from(qd),
            None => {
                let cause: String = format!("queue descriptor must be informed");
                info!("run_wait_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        match self.has_operation_completed() {
            Ok((qd, qr)) if args_qd == qd => match qr {
                OperationResult::Accept((remote_qd, remote_addr)) if ret == 0 => {
                    info!("connection accepted (qd={:?}, addr={:?})", qd, remote_addr);
                    self.remote_qd = Some((self.remote_qd.unwrap().0, Some(remote_qd)));
                    Ok(())
                },
                OperationResult::Connect => {
                    info!("connection established as expected (qd={:?})", qd);
                    Ok(())
                },
                OperationResult::Pop(_sockaddr, _data) => {
                    info!("pop completed as expected (qd={:?})", qd);
                    Ok(())
                },
                OperationResult::Push => {
                    info!("push completed as expected (qd={:?})", qd);
                    Ok(())
                },
                OperationResult::Close => {
                    info!("close completed as expected (qd={:?})", qd);
                    Ok(())
                },
                OperationResult::Failed(e) if e.errno == ret as i32 => {
                    info!("operation failed as expected (qd={:?}, errno={:?})", qd, e.errno);
                    Ok(())
                },
                OperationResult::Failed(e) => {
                    unreachable!("operation failed unexpectedly (qd={:?}, errno={:?})", qd, e.errno);
                },
                _ => unreachable!("unexpected operation has completed"),
            },
            _ => unreachable!("no operation has completed, but it should"),
        }
    }

    fn run_pushto_syscall(&mut self, args: &PushToArgs, ret: i32) -> Result<()> {
        let buf_len: usize = match args.len {
            Some(len) => len.try_into()?,
            None => {
                let cause: String = format!("buffer length must be informed");
                info!("run_pushto_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

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

        let remote_qd: QDesc = match args.qd {
            Some(qd) => qd.into(),
            None => {
                anyhow::bail!("remote queue descriptor must have been previously assigned");
            },
        };

        let buf: DemiBuffer = Self::prepare_dummy_buffer(buf_len, None);
        match self.engine.udp_pushto(remote_qd, buf, remote_addr) {
            Ok(push_qt) => {
                self.inflight.push_back(push_qt);
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
        let args_qd: QDesc = args.qd.into();

        match self.engine.tcp_async_close(args_qd) {
            Ok(close_qt) => {
                self.inflight.push_back(close_qt);
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

    fn build_tcp_options(&self, options: &Vec<TcpOption>) -> ([TcpOptions2; MAX_TCP_OPTIONS], usize) {
        let mut option_list: Vec<TcpOptions2> = Vec::new();

        // Convert options.
        for option in options {
            match option {
                TcpOption::Noop => option_list.push(TcpOptions2::NoOperation),
                TcpOption::Mss(mss) => option_list.push(TcpOptions2::MaximumSegmentSize(mss.clone())),
                TcpOption::WindowScale(wscale) => option_list.push(TcpOptions2::WindowScale(wscale.clone())),
                TcpOption::SackOk => option_list.push(TcpOptions2::SelectiveAcknowlegementPermitted),
                TcpOption::Timestamp(sender, echo) => option_list.push(TcpOptions2::Timestamp {
                    sender_timestamp: sender.clone(),
                    echo_timestamp: echo.clone(),
                }),
                TcpOption::EndOfOptions => option_list.push(TcpOptions2::EndOfOptionsList),
            }
        }

        let num_options: usize = option_list.len();

        // Pad options list.
        while option_list.len() < MAX_TCP_OPTIONS {
            option_list.push(TcpOptions2::NoOperation);
        }

        (option_list.try_into().unwrap(), num_options)
    }

    fn build_ethernet2_header(&self) -> Ethernet2Header {
        let (src_addr, dst_addr) = { (self.remote_mac, self.local_mac) };
        Ethernet2Header::new(dst_addr, src_addr, EtherType2::Ipv4)
    }

    fn build_ipv4_header(&self, protocol: IpProtocol) -> Ipv4Header {
        let (src_addr, dst_addr) = {
            (
                self.remote_sockaddr.ip().to_owned(),
                self.local_sockaddr.ip().to_owned(),
            )
        };

        Ipv4Header::new(src_addr, dst_addr, protocol)
    }

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

    fn build_tcp_segment(&self, tcp_packet: &TcpPacket) -> DemiBuffer {
        let tcp_hdr: TcpHeader = self.build_tcp_header(&tcp_packet);
        let mut pkt: DemiBuffer = if tcp_packet.seqnum.win > 0 {
            Self::prepare_dummy_buffer(tcp_packet.seqnum.win as usize, None)
        } else {
            DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16)
        };
        tcp_hdr.serialize_and_attach(&mut pkt, self.remote_sockaddr.ip(), self.local_sockaddr.ip(), false);
        self.prepend_ipv4_header(IpProtocol::TCP, &mut pkt);
        self.prepend_ethernet_header(&mut pkt);
        pkt
    }

    fn build_udp_datagram(&self, udp_packet: &UdpPacket) -> DemiBuffer {
        let udp_hdr: UdpHeader = self.build_udp_header(&udp_packet);
        let mut pkt: DemiBuffer = Self::prepare_dummy_buffer(udp_packet.len as usize, None);
        // This is an incoming packet, so the source is the remote address and the destination is the local address.
        udp_hdr.serialize_and_attach(&mut pkt, &self.remote_sockaddr.ip(), &self.local_sockaddr.ip(), false);
        self.prepend_ipv4_header(IpProtocol::UDP, &mut pkt);
        self.prepend_ethernet_header(&mut pkt);
        pkt
    }

    fn prepend_ethernet_header(&self, pkt: &mut DemiBuffer) {
        let ethernet2_hdr: Ethernet2Header = self.build_ethernet2_header();
        ethernet2_hdr.serialize_and_attach(pkt);
    }

    fn prepend_ipv4_header(&self, ip_protocol: IpProtocol, pkt: &mut DemiBuffer) {
        let ipv4_hdr: Ipv4Header = self.build_ipv4_header(ip_protocol);
        ipv4_hdr.serialize_and_attach(pkt);
    }

    fn run_incoming_packet(&mut self, tcp_packet: &TcpPacket) -> Result<()> {
        let buf: DemiBuffer = self.build_tcp_segment(&tcp_packet);
        self.engine.push_frame(buf);
        Ok(())
    }

    fn run_incoming_udp_packet(&mut self, udp_packet: &UdpPacket) -> Result<()> {
        let buf: DemiBuffer = self.build_udp_datagram(&udp_packet);
        self.engine.push_frame(buf);
        Ok(())
    }

    fn check_ethernet2_header(&self, eth2_header: &Ethernet2Header) -> Result<()> {
        ensure_eq!(eth2_header.src_addr(), self.local_mac);
        ensure_eq!(eth2_header.dst_addr(), self.remote_mac);
        ensure_eq!(eth2_header.ether_type(), EtherType2::Ipv4);
        Ok(())
    }

    fn check_ipv4_header(&self, ipv4_header: &Ipv4Header, protocol: IpProtocol) -> Result<()> {
        ensure_eq!(ipv4_header.get_src_addr(), self.local_sockaddr.ip().to_owned());
        ensure_eq!(ipv4_header.get_dest_addr(), self.remote_sockaddr.ip().to_owned());
        ensure_eq!(ipv4_header.get_protocol(), protocol);
        Ok(())
    }

    fn check_tcp_header(&self, tcp_header: &TcpHeader, tcp_packet: &TcpPacket) -> Result<()> {
        ensure_eq!(tcp_header.src_port, self.local_sockaddr.port());
        ensure_eq!(tcp_header.dst_port, self.remote_port);
        ensure_eq!(tcp_header.seq_num, tcp_packet.seqnum.seq.into());

        if let Some(ack_num) = tcp_packet.ack {
            ensure_eq!(tcp_header.ack, true);
            ensure_eq!(tcp_header.ack_num, ack_num.into());
        }

        if let Some(winsize) = tcp_packet.win {
            ensure_eq!(tcp_header.window_size, winsize as u16);
        }

        ensure_eq!(tcp_header.cwr, tcp_packet.flags.cwr);
        ensure_eq!(tcp_header.ece, tcp_packet.flags.ece);
        ensure_eq!(tcp_header.urg, tcp_packet.flags.urg);
        ensure_eq!(tcp_header.ack, tcp_packet.flags.ack);
        ensure_eq!(tcp_header.psh, tcp_packet.flags.psh);
        ensure_eq!(tcp_header.rst, tcp_packet.flags.rst);
        ensure_eq!(tcp_header.syn, tcp_packet.flags.syn);
        ensure_eq!(tcp_header.fin, tcp_packet.flags.fin);
        ensure_eq!(tcp_header.urgent_pointer, 0);

        // Check if options match what we expect.
        for i in 0..tcp_packet.options.len() {
            match tcp_packet.options[i] {
                TcpOption::Noop => {
                    ensure_eq!(tcp_header.option_list[i], TcpOptions2::NoOperation);
                },
                TcpOption::Mss(mss) => {
                    ensure_eq!(tcp_header.option_list[i], TcpOptions2::MaximumSegmentSize(mss));
                },
                TcpOption::WindowScale(wscale) => {
                    ensure_eq!(tcp_header.option_list[i], TcpOptions2::WindowScale(wscale));
                },
                TcpOption::SackOk => {
                    ensure_eq!(tcp_header.option_list[i], TcpOptions2::SelectiveAcknowlegementPermitted);
                },
                TcpOption::Timestamp(sender, echo) => {
                    ensure_eq!(
                        tcp_header.option_list[i],
                        TcpOptions2::Timestamp {
                            sender_timestamp: sender,
                            echo_timestamp: echo,
                        }
                    );
                },
                TcpOption::EndOfOptions => {
                    ensure_eq!(tcp_header.option_list[i], TcpOptions2::EndOfOptionsList);
                },
            }
        }

        Ok(())
    }

    fn check_udp_header(&self, udp_header: &UdpHeader, _udp_packet: &UdpPacket) -> Result<()> {
        ensure_eq!(udp_header.src_port(), self.local_sockaddr.port());
        ensure_eq!(udp_header.dest_port(), self.remote_port);
        Ok(())
    }

    fn has_operation_completed(&mut self) -> Result<(QDesc, OperationResult)> {
        match self.inflight.pop_front() {
            Some(qt) => Ok(self.engine.wait(qt)?),
            None => anyhow::bail!("should have an inflight queue token"),
        }
    }

    fn run_outgoing_packet(&mut self, tcp_packet: &TcpPacket) -> Result<()> {
        let mut frames: VecDeque<DemiBuffer> = self.engine.pop_expected_frames(1);
        ensure_eq!(frames.len(), 1);

        let pkt: &mut DemiBuffer = &mut frames[0];
        let eth2_header: Ethernet2Header = Ethernet2Header::parse_and_strip(pkt)?;
        self.check_ethernet2_header(&eth2_header)?;

        let ipv4_header: Ipv4Header = Ipv4Header::parse_and_strip(pkt)?;
        self.check_ipv4_header(&ipv4_header, IpProtocol::TCP)?;

        let src_ipv4_addr: Ipv4Addr = ipv4_header.get_src_addr();
        let dest_ipv4_addr: Ipv4Addr = ipv4_header.get_dest_addr();
        let tcp_header: TcpHeader = TcpHeader::parse_and_strip(&src_ipv4_addr, &dest_ipv4_addr, pkt, true)?;
        ensure_eq!(tcp_packet.seqnum.win as usize, pkt.len());
        self.check_tcp_header(&tcp_header, &tcp_packet)?;

        Ok(())
    }

    fn run_outgoing_udp_packet(&mut self, udp_packet: &UdpPacket) -> Result<()> {
        let mut frames: VecDeque<DemiBuffer> = self.engine.pop_expected_frames(1);
        // FIXME: We currently do not support multi-frame segments.
        ensure_eq!(frames.len(), 1);
        let pkt: &mut DemiBuffer = &mut frames[0];
        let eth2_header: Ethernet2Header = Ethernet2Header::parse_and_strip(pkt)?;
        self.check_ethernet2_header(&eth2_header)?;

        let ipv4_header: Ipv4Header = Ipv4Header::parse_and_strip(pkt)?;
        self.check_ipv4_header(&ipv4_header, IpProtocol::UDP)?;

        let udp_header: UdpHeader =
            UdpHeader::parse_and_strip(&self.local_sockaddr.ip(), &self.remote_sockaddr.ip(), pkt, true)?;
        ensure_eq!(udp_packet.len as usize, pkt.len());
        self.check_udp_header(&udp_header, &udp_packet)?;

        Ok(())
    }

    fn prepare_dummy_buffer(size: usize, stamp: Option<u8>) -> DemiBuffer {
        assert!(size < u16::MAX as usize);
        let mut buf: DemiBuffer = DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16);
        for i in 0..size {
            buf[i] = stamp.unwrap_or(i as u8);
        }
        buf
    }
}
