// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        protocols::{
            ethernet2::EtherType2,
            tcp::segment::{
                TcpOptions2,
                MAX_TCP_OPTIONS,
            },
        },
        test_helpers::SharedTestRuntime,
    },
    runtime::network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
    },
    MacAddress,
    QToken,
};
use anyhow::Result;
use nettest::glue::{
    AcceptArgs,
    BindArgs,
    ConnectArgs,
    Event,
    ListenArgs,
    PacketDirection,
    PacketEvent,
    PushArgs,
    SocketArgs,
    SyscallEvent,
    TcpPacket,
};
use std::{
    collections::{
        HashMap,
        VecDeque,
    },
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
    path::PathBuf,
    time::{
        Duration,
        Instant,
    },
};

use crate::{
    inetstack::{
        protocols::{
            ethernet2::Ethernet2Header,
            ip::IpProtocol,
            ipv4::Ipv4Header,
            tcp::segment::{
                TcpHeader,
                TcpSegment,
            },
        },
        test_helpers::{
            self,
            SharedEngine,
        },
    },
    runtime::{
        memory::DemiBuffer,
        network::PacketBuf,
    },
    QDesc,
};

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

    let input_path: String = match env::var("INPUT_DIR") {
        Ok(config_path) => config_path,
        Err(e) => {
            let cause: String = format!("missing INPUT_DIR environment variable (err={:?})", e);
            eprintln!("test_simulation(): {:?}", cause);
            anyhow::bail!(cause);
        },
    };

    let tests: Vec<String> = collect_tests(&input_path)?;

    if tests.is_empty() {
        let cause: String = format!("no tests found under {:?}", input_path);
        eprintln!("test_simulation(): {:?}", cause);
        return Ok(());
    }

    for test in &tests {
        println!("Running test: {:?}", test);

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
    for entry in std::fs::read_dir(test_path)? {
        let entry: DirEntry = entry?;
        // Skip directories.
        if entry.file_type()?.is_dir() {
            continue;
        }

        // Skip unsupported files.
        if !entry.file_name().to_str().unwrap().ends_with(".pkt") {
            continue;
        }

        let path: PathBuf = entry.path();
        let path: String = path.to_str().unwrap().to_string();
        files.push(path);
    }

    files.sort();

    Ok(files)
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// A simulation of the network stack.
struct Simulation {
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
        const ARP_CACHE_TTL: Duration = Duration::from_secs(600);
        let request_timeout: Duration = Duration::from_secs(1);
        let retry_count: usize = 2;
        let disable_arp: bool = false;

        let arp_config: ArpConfig = Self::new_arp_config(
            Some(ARP_CACHE_TTL),
            Some(request_timeout),
            Some(retry_count),
            Some(disable_arp),
            local_mac,
            local_ipv4,
            remote_mac,
            remote_ipv4,
        );
        let udp_config: UdpConfig = Self::new_udp_config();
        let tcp_config: TcpConfig = Self::new_tcp_config();

        let test_rig: SharedTestRuntime = SharedTestRuntime::new(
            now,
            arp_config,
            udp_config,
            tcp_config,
            local_mac.clone(),
            local_ipv4.clone(),
        );
        let local: SharedEngine = SharedEngine::new(test_rig)?;

        println!("Local: sockaddr={:?}, macaddr={:?}", local_ipv4, local_mac);
        println!("Remote: sockaddr={:?}, macaddr={:?}", remote_ipv4, remote_mac);

        let steps: Vec<String> = Self::read_input_file(&filename)?;
        Ok(Simulation {
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

    /// Creates a new ARP configuration.
    fn new_arp_config(
        cache_ttl: Option<Duration>,
        request_timeout: Option<Duration>,
        retry_count: Option<usize>,
        disable_arp: Option<bool>,
        local_mac: &MacAddress,
        local_ipv4: &Ipv4Addr,
        remote_mac: &MacAddress,
        remote_ipv4: &Ipv4Addr,
    ) -> ArpConfig {
        let mut initial_values: HashMap<std::net::Ipv4Addr, MacAddress> = HashMap::new();
        initial_values.insert(local_ipv4.clone(), local_mac.clone());
        initial_values.insert(remote_ipv4.clone(), remote_mac.clone());

        ArpConfig::new(
            cache_ttl,
            request_timeout,
            retry_count,
            Some(initial_values),
            disable_arp,
        )
    }

    /// Creates a new UDP configuration.
    fn new_udp_config() -> UdpConfig {
        UdpConfig::default()
    }

    /// Creates a new TCP configuration.
    fn new_tcp_config() -> TcpConfig {
        TcpConfig::default()
    }

    /// Runs the simulation.
    pub fn run(&mut self, verbose: bool) -> Result<()> {
        println!("+++++++++++++++++");
        // Process all lines of the source file.
        for step in &self.steps.clone() {
            if verbose {
                println!("Line: {:?}", step);
            }

            if let Some(event) = nettest::run_parser(&step, verbose)? {
                self.run_event(&event)?;
            }
        }

        Ok(())
    }

    /// Runs an event.
    fn run_event(&mut self, event: &Event) -> Result<()> {
        self.now += event.time;
        self.engine.get_test_rig().get_runtime().advance_clock(self.now);
        println!("=================");

        match &event.action {
            nettest::glue::Action::SyscallEvent(syscall) => self.run_syscall(syscall)?,
            nettest::glue::Action::PacketEvent(packet) => self.run_packet(packet)?,
        }

        Ok(())
    }

    /// Runs a system call.
    #[allow(unused_variables)]
    fn run_syscall(&mut self, syscall: &SyscallEvent) -> Result<()> {
        println!("{:?}: {:?}", self.now, syscall);
        match &syscall.syscall {
            // Issue demi_socket().
            nettest::glue::DemikernelSyscall::Socket(args, ret) => self.run_socket_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Bind(args, ret) => self.run_bind_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Listen(args, ret) => self.run_listen_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Accept(args, fd) => self.run_accept_syscall(args, fd.clone())?,
            nettest::glue::DemikernelSyscall::Connect(args, ret) => self.run_connect_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Push(args, ret) => self.run_push_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Pop(ret) => self.run_pop_syscall(ret.clone())?,
            nettest::glue::DemikernelSyscall::Wait(args, ret) => self.run_wait_syscall(args, ret.clone())?,
            nettest::glue::DemikernelSyscall::Unsupported => {
                eprintln!("Unsupported syscall");
            },
            _ => {
                eprintln!("Unimplemented syscall");
            },
        }

        Ok(())
    }

    /// Runs a packet.
    fn run_packet(&mut self, packet: &PacketEvent) -> Result<()> {
        println!("{:?}: {:?}", self.now, packet);
        match packet {
            nettest::glue::PacketEvent::Tcp(direction, tcp_packet) => match direction {
                PacketDirection::Incoming => self.run_incoming_packet(tcp_packet)?,
                PacketDirection::Outgoing => self.run_outgoing_packet(tcp_packet)?,
            },
        }

        Ok(())
    }

    /// Runs a socket system call.
    fn run_socket_syscall(&mut self, args: &SocketArgs, ret: u32) -> Result<()> {
        // Check for unsupported socket domain.
        if args.domain != nettest::glue::SocketDomain::AF_INET {
            let cause: String = format!("unsupported domain socket domain (domain={:?})", args.domain);
            eprintln!("run_socket_syscall(): {:?}", cause);
            anyhow::bail!(cause);
        }

        // Check for unsupported socket type.
        if args.typ != nettest::glue::SocketType::SOCK_STREAM {
            let cause: String = format!("unsupported socket type (type={:?})", args.typ);
            eprintln!("run_socket_syscall(): {:?}", cause);
            anyhow::bail!(cause);
        }

        // Check for unsupported socket protocol.
        if args.protocol != nettest::glue::SocketProtocol::IPPROTO_TCP {
            let cause: String = format!("unsupported socket protocol (protocol={:?})", args.protocol);
            eprintln!("run_socket_syscall(): {:?}", cause);
            anyhow::bail!(cause);
        }

        // Issue demi_socket().
        match self.engine.tcp_socket() {
            Ok(qd) => {
                self.local_qd = Some((ret, qd));
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for socket syscall");
                eprintln!("run_socket_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a bind system call.
    fn run_bind_syscall(&mut self, args: &BindArgs, ret: u32) -> Result<()> {
        // Extract bind address.
        let local_addr: SocketAddrV4 = match args.addr {
            None => {
                self.local_sockaddr.set_port(self.local_port);
                self.local_sockaddr
            },

            // Custom bind address is not supported.
            Some(addr) => {
                let cause: String = format!("unsupported bind address (addr={:?})", addr);
                eprintln!("run_bind_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract local queue descriptor.
        let local_qd: QDesc = match args.qd {
            Some(local_fd) => match self.local_qd {
                Some((fd, qd)) if fd == local_fd => qd,
                _ => {
                    let cause: String = format!("local queue descriptor mismatch");
                    eprintln!("run_bind_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                },
            },
            None => {
                let cause: String = format!("local queue descriptor musth have been previously assigned");
                eprintln!("run_bind_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Issue demi_bind().
        match self.engine.tcp_bind(local_qd, local_addr) {
            Ok(()) if ret == 0 => Ok(()),
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for bind syscall");
                eprintln!("run_bind_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a listen system call.
    fn run_listen_syscall(&mut self, args: &ListenArgs, ret: u32) -> Result<()> {
        // Check if backlog length was informed.
        let backlog: usize = match args.backlog {
            Some(backlog) => backlog,
            None => {
                let cause: String = format!("backlog length must be informed");
                eprintln!("run_listen_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract local queue descriptor.
        let local_qd: QDesc = match args.qd {
            Some(local_fd) => match self.local_qd {
                Some((fd, qd)) if fd == local_fd => qd,
                _ => {
                    let cause: String = format!("local queue descriptor mismatch");
                    eprintln!("run_listen_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                },
            },
            None => {
                let cause: String = format!("local queue descriptor musth have been previously assigned");
                eprintln!("run_listen_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Issue demi_listen().
        match self.engine.tcp_listen(local_qd, backlog) {
            Ok(()) if ret == 0 => Ok(()),
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for listen syscall");
                eprintln!("run_listen_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs an accept system call.
    fn run_accept_syscall(&mut self, args: &AcceptArgs, ret: u32) -> Result<()> {
        // Extract local queue descriptor.
        let local_qd: QDesc = match args.qd {
            Some(local_fd) => match self.local_qd {
                Some((fd, qd)) if fd == local_fd => qd,
                _ => {
                    let cause: String = format!("local queue descriptor mismatch");
                    eprintln!("run_accept_syscall(): {:?}", cause);
                    anyhow::bail!(cause);
                },
            },
            None => {
                let cause: String = format!("local queue descriptor musth have been previously assigned");
                eprintln!("run_accept_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Issue demi_accept().
        match self.engine.tcp_accept(local_qd) {
            Ok(accept_qt) => {
                self.remote_qd = Some((ret, None));
                self.inflight = Some(accept_qt);
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for accept syscall");
                eprintln!("run_accept_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a connect system call.
    fn run_connect_syscall(&mut self, args: &ConnectArgs, ret: u32) -> Result<()> {
        // Extract local queue descriptor.
        let local_qd: QDesc = match self.local_qd {
            Some((_, qd)) => qd,
            None => {
                let cause: String = format!("local queue descriptor musth have been previously assigned");
                eprintln!("run_connect_syscall(): {:?}", cause);
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
                eprintln!("run_connect_syscall(): {:?}", cause);
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
                eprintln!("run_accept_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a push system call.
    fn run_push_syscall(&mut self, args: &PushArgs, ret: u32) -> Result<()> {
        // Extract buffer length.
        let buf_len: u16 = match args.len {
            Some(len) => len.try_into()?,
            None => {
                let cause: String = format!("buffer length must be informed");
                eprintln!("run_push_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        // Extract remote queue descriptor.
        let remote_qd: QDesc = match self.remote_qd {
            Some((_, qd)) => qd.unwrap(),
            None => {
                anyhow::bail!("remote queue descriptor musth have been previously assigned");
            },
        };

        let buf: DemiBuffer = Self::cook_buffer(buf_len as usize, None);
        match self.engine.tcp_push(remote_qd, buf) {
            Ok(push_qt) => {
                self.inflight = Some(push_qt);
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for push syscall");
                eprintln!("run_push_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Runs a pop system call.
    fn run_pop_syscall(&mut self, ret: u32) -> Result<()> {
        // Extract remote queue descriptor.
        let remote_qd: QDesc = match self.remote_qd {
            Some((_, qd)) => qd.unwrap(),
            None => {
                anyhow::bail!("remote queue descriptor musth have been previously assigned");
            },
        };

        match self.engine.tcp_pop(remote_qd) {
            Ok(pop_qt) => {
                self.inflight = Some(pop_qt);
                Ok(())
            },
            Err(err) if ret as i32 == err.errno => Ok(()),
            _ => {
                let cause: String = format!("unexpected return for pop syscall");
                eprintln!("run_pop_syscall(): ret={:?}", ret);
                anyhow::bail!(cause);
            },
        }
    }

    /// Emulates wait system call.
    fn run_wait_syscall(&mut self, args: &nettest::glue::WaitArgs, ret: u32) -> Result<()> {
        // Extract queue descriptor.
        let args_qd: QDesc = match args.qd {
            Some(qd) => QDesc::from(qd),
            None => {
                let cause: String = format!("queue descriptor must be informed");
                eprintln!("run_wait_syscall(): {:?}", cause);
                anyhow::bail!(cause);
            },
        };

        self.engine.get_test_rig().poll_scheduler();

        match self.operation_has_completed() {
            Ok(Some(qt)) => match self
                .engine
                .get_test_rig()
                .get_runtime()
                .remove_coroutine_with_qtoken(qt)
                .get_result()
            {
                Some((qd, qr)) if args_qd == qd => match qr {
                    crate::OperationResult::Accept((remote_qd, remote_addr)) if ret == 0 => {
                        eprintln!("connection accepted (qd={:?}, addr={:?})", qd, remote_addr);
                        self.remote_qd = Some((self.remote_qd.unwrap().0, Some(remote_qd)));
                        Ok(())
                    },
                    crate::OperationResult::Connect => {
                        eprintln!("connection established as expected (qd={:?})", qd);
                        Ok(())
                    },
                    crate::OperationResult::Pop(_sockaddr, _data) => {
                        eprintln!("pop completed as expected (qd={:?})", qd);
                        Ok(())
                    },
                    crate::OperationResult::Push => {
                        eprintln!("push completed as expected (qd={:?})", qd);
                        Ok(())
                    },
                    crate::OperationResult::Failed(e) if e.errno == ret as i32 => {
                        eprintln!("operation failed as expected (qd={:?}, errno={:?})", qd, e.errno);
                        Ok(())
                    },
                    crate::OperationResult::Failed(e) => {
                        unreachable!("operation failed unexpectedly (qd={:?}, errno={:?})", qd, e.errno);
                    },
                    _ => unreachable!("unexpected operation has completed coroutine has completed"),
                },
                _ => unreachable!("no operation has completed coroutine has completed, but it should"),
            },
            _ => unreachable!("no operation has completed coroutine has completed, but it should"),
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
    fn build_ipv4_header(&self) -> Ipv4Header {
        let (src_addr, dst_addr) = {
            (
                self.remote_sockaddr.ip().to_owned(),
                self.local_sockaddr.ip().to_owned(),
            )
        };

        Ipv4Header::new(src_addr, dst_addr, IpProtocol::TCP)
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

    /// Builds a TCP segment.
    fn build_tcp_segment(&self, tcp_packet: &TcpPacket) -> TcpSegment {
        // Create headers.
        let ethernet2_hdr: Ethernet2Header = self.build_ethernet_header();
        let ipv4_hdr: Ipv4Header = self.build_ipv4_header();
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

    /// Runs an incoming packet.
    fn run_incoming_packet(&mut self, tcp_packet: &TcpPacket) -> Result<()> {
        let segment: TcpSegment = self.build_tcp_segment(&tcp_packet);

        let buf: DemiBuffer = Self::serialize_segment(segment);
        self.engine.receive(buf)?;

        self.engine.get_test_rig().poll_scheduler();
        // Poll the scheduler again.
        // TODO: Remove this once we have a way to poll the scheduler until there is no more work to be done.
        self.engine.get_test_rig().poll_scheduler();

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
    fn check_ipv4_header(&self, ipv4_header: &Ipv4Header) -> Result<()> {
        crate::ensure_eq!(ipv4_header.get_src_addr(), self.local_sockaddr.ip().to_owned());
        crate::ensure_eq!(ipv4_header.get_dest_addr(), self.remote_sockaddr.ip().to_owned());
        crate::ensure_eq!(ipv4_header.get_protocol(), IpProtocol::TCP);

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

    /// Checks if an operation has completed.
    fn operation_has_completed(&mut self) -> Result<Option<QToken>> {
        let has_completed: bool = match self.inflight {
            Some(qt) => match self.engine.get_test_rig().get_runtime().from_task_id(qt.clone()) {
                Ok(task_handle) => task_handle.has_completed(),
                Err(e) => anyhow::bail!("{:?}", e),
            },
            None => anyhow::bail!("should have an inflight queue token"),
        };
        if has_completed {
            Ok(Some(self.inflight.take().unwrap()))
        } else {
            Ok(None)
        }
    }

    /// Runs an outgoing packet.
    fn run_outgoing_packet(&mut self, tcp_packet: &TcpPacket) -> Result<()> {
        self.engine.get_test_rig().poll_scheduler();

        let frames: VecDeque<DemiBuffer> = self.engine.get_test_rig().pop_all_frames();

        // FIXME: We currently do not support multi-frame segments.
        crate::ensure_eq!(frames.len(), 1);

        for bytes in &frames {
            let (eth2_header, eth2_payload) = Ethernet2Header::parse(bytes.clone())?;
            self.check_ethernet2_header(&eth2_header)?;

            let (ipv4_header, ipv4_payload) = Ipv4Header::parse(eth2_payload)?;
            self.check_ipv4_header(&ipv4_header)?;

            let (tcp_header, _) = TcpHeader::parse(&ipv4_header, ipv4_payload, true)?;
            self.check_tcp_header(&tcp_header, &tcp_packet)?;
        }

        Ok(())
    }

    /// Serializes a TCP segment.
    fn serialize_segment(pkt: TcpSegment) -> DemiBuffer {
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
