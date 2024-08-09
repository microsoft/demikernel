// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};

use std::{cell::RefCell, collections::{HashMap, HashSet, hash_map::Entry}, rc::Rc, time::Instant};
use ::std::{
    env,
    net::SocketAddr,
    panic,
    str::FromStr,
    slice,
    time::Duration,
};
use ctrlc;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use colored::Colorize;

#[macro_use]
extern crate lazy_static;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
//=====================================================================================

macro_rules! server_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "capy-log")]
        if let Ok(val) = std::env::var("CAPY_LOG") {
            if val == "all" {
                eprintln!("{}", format!($($arg)*).green());
            }
        }
    };
}

//=====================================================================================

const ROOT: &str = "/var/www/demo";
const BUFSZ: usize = 4096;

static mut START_TIME: Option<Instant> = None;
static mut SERVER_PORT: u16 = 0;

//=====================================================================================

// Borrowed from Loadgen
struct Buffer {
    buf: Vec<u8>,
    head: usize,
    tail: usize,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            buf: vec![0; BUFSZ],
            head: 0,
            tail: 0,
        }
    }

    pub fn data_size(&self) -> usize {
        self.head - self.tail
    }

    pub fn get_data(&self) -> &[u8] {
        &self.buf[self.tail..self.head]
    }

    pub fn push_data(&mut self, size: usize) {
        self.head += size;
        assert!(self.head <= self.buf.len());
    }

    pub fn pull_data(&mut self, size: usize) {
        assert!(size <= self.data_size());
        self.tail += size;
    }

    pub fn get_empty_buf(&mut self) -> &mut [u8] {
        &mut self.buf[self.head..]
    }

    pub fn try_shrink(&mut self) -> Result<()> {
        if self.data_size() == 0 {
            self.head = 0;
            self.tail = 0;
            return Ok(());
        }

        if self.head < self.buf.len() {
            return Ok(());
        }

        if self.data_size() == self.buf.len() {
            panic!("Need larger buffer for HTTP messages");
        }

        self.buf.copy_within(self.tail..self.head, 0);
        self.head = self.data_size();
        self.tail = 0;
        Ok(())
    }
}

//======================================================================================================================
// push_and_wait()
//======================================================================================================================

/// Pushes a scatter-gather array to a remote socket and waits for the operation to complete.
fn push_and_wait(libos: &mut LibOS, sockqd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
    // Push data.
    let qt: QToken = match libos.push(sockqd, sga) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("push failed: {:?}", e),
    };
    // match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
    //     Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
    //     Ok(_) => anyhow::bail!("unexpected result"),
    //     Err(e) => anyhow::bail!("operation failed: {:?}", e),
    // };

    Ok(())
}

//======================================================================================================================
// server()
//======================================================================================================================

struct SessionData {
    data: Vec<u8>,
}

impl SessionData {
    fn new(size: usize) -> Self {
        Self { data: vec![5; size] }
    }
}

struct ConnectionState {
    buffer: Buffer,
    session_data: SessionData,
}


/// Creates a scatter-gather-array.
fn mksga(libos: &mut LibOS, data: &[u8]) -> Result<demi_sgarray_t> {
    let size = data.len();
    debug_assert!(size > std::mem::size_of::<u64>());

    let sga: demi_sgarray_t = libos.sgaalloc(size)?;
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;

    if size > len {
        anyhow::bail!("Allocated SGA segment is smaller than the input data");
    }

    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };

    // Copy the data into the slice
    slice[0..size].copy_from_slice(data);

    Ok(sga)
}

fn respond_to_request(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> Result<()> {
    lazy_static! {
        static ref RESPONSE: String = {
            match std::fs::read_to_string("/var/www/demo/index.html") {
                Ok(contents) => {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents)
                },
                Err(_) => {
                    format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n")
                },
            }
        };
    }
    
    server_log!("PUSH: {}", RESPONSE.lines().next().unwrap_or(""));

    let sga: demi_sgarray_t = mksga(libos, RESPONSE.as_bytes())?;
    if let Err(e) = push_and_wait(
        libos,
        qd,
        &sga,
    ) {
        anyhow::bail!("push and wait failed: {:?}", e)
    }
    if let Err(e) = libos.sgafree(sga) {
        anyhow::bail!("failed to release scatter-gather array: {:?}", e)
    }
    Ok(())
}

#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn push_data_and_run(libos: &mut LibOS, qd: QDesc, buffer: &mut Buffer, data: &[u8], qts: &mut Vec<QToken>) -> Result<()>{
    
    server_log!("buffer.data_size() {}", buffer.data_size());
    // fast path: no previous data in the stream and this request contains exactly one HTTP request
    if buffer.data_size() == 0 {
        if find_subsequence(data, b"\r\n\r\n").unwrap_or(data.len()) == data.len() - 4 {
            server_log!("responding 1");
            
            if let Err(e) = respond_to_request(libos, qd, data) {
                anyhow::bail!("respond_to_request failed: {:?}", e);
            }
            return Ok(());
        }
    }
    // println!("* CHECK *\n");
    // Copy new data into buffer
    buffer.get_empty_buf()[..data.len()].copy_from_slice(data);
    buffer.push_data(data.len());
    server_log!("buffer.data_size() {}", buffer.data_size());
    let mut sent = 0;

    loop {
        let dbuf = buffer.get_data();
        match find_subsequence(dbuf, b"\r\n\r\n") {
            Some(idx) => {
                server_log!("responding 2");
                if let Err(e) = respond_to_request(libos, qd, data) {
                    anyhow::bail!("respond_to_request failed: {:?}", e);
                }
                buffer.pull_data(idx + 4);
                buffer.try_shrink().unwrap();
                return Ok(());
            }
            None => {
                return Ok(());
            }
        }
    }
}

fn server(local: SocketAddr) -> Result<()> {
    unsafe{ SERVER_PORT = local.port() };
    let mut request_count = 0;
    let mut queue_length_vec: Vec<(usize, usize)> = Vec::new();
    let migration_per_n: i32 = env::var("MIG_PER_N")
            .unwrap_or(String::from("10")) // Default value is 10 if MIG_PER_N is not set
            .parse()
            .expect("MIG_PER_N must be a i32");
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        // LibOS::dpdk_print_eth_stats();
        // LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
    // unsafe { START_TIME = Some(Instant::now()); }

    let session_data_size: usize = std::env::var("SESSION_DATA_SIZE").map_or(1024, |v| v.parse().unwrap());

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name, None).expect("intialized libos");
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, Rc<RefCell<ConnectionState>>> = HashMap::new();

    qts.push(libos.accept(sockqd).expect("accept"));

    #[cfg(feature = "manual-tcp-migration")]
    let mut requests_remaining: HashMap<QDesc, i32> = HashMap::new();

    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, demi_opcode_t)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), demi_opcode_t::DEMI_OPC_CONNECT));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);
    
    loop {
        let (offset, qr) = libos.wait_any(&qts, None).expect("result");

        /* let mut pop_count = completed_results.iter().filter(|(_, _, result)| {
            matches!(result, OperationResult::Pop(_, _))
        }).count(); */
        /* #[cfg(feature = "tcp-migration")]{
            request_count += 1;
            if request_count % 1 == 0 {
                eprintln!("request_counnt: {} {}", request_count, libos.global_recv_queue_length());
            queue_length_vec.push((request_count, libos.global_recv_queue_length()));
            }
        } */
            
        server_log!("\n\n======= OS: I/O operations have been completed, take the results! =======");

        qts.swap_remove(offset);
        let qd = qr.qr_qd.into();
        
        match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_ACCEPT  => {
                let new_qd = unsafe { qr.qr_value.ares.qd.into() };
                server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);
                match libos.pop(new_qd, None) {
                    Ok(pop_qt) => {
                        qts.push(pop_qt)
                    },
                    Err(e) => panic!("pop qt: {}", e),
                }
                let state = Rc::new(RefCell::new(ConnectionState {
                    buffer: Buffer::new(),
                    session_data: SessionData::new(session_data_size),
                }));
                connstate.insert(new_qd, state);
                qts.push(libos.accept(qd).expect("accept qtoken"));
            },
            demi_opcode_t::DEMI_OPC_POP => {
                let sga = unsafe { qr.qr_value.sga };
                let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
                let len: usize = sga.sga_segs[0].sgaseg_len as usize;
                let recvbuf: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
                // match std::str::from_utf8(recvbuf) {
                //     Ok(human_readable) => eprintln!("POP: {:?}", human_readable),
                //     Err(e) => eprintln!("POP: invalid UTF-8 sequence: {:?}", e),
                // }
                let mut state = connstate.get_mut(&qd).unwrap().borrow_mut();
                let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                
                match libos.pop(qd, None) {
                    Ok(pop_qt) => {
                        qts.push(pop_qt)
                    },
                    Err(e) => panic!("pop qt: {}", e),
                }

                // Do not silently ignore if unable to free scatter-gather array.
                if let Err(e) = libos.sgafree(sga) {
                    anyhow::bail!("failed to release scatter-gather array: {:?}", e);
                }
            },
            demi_opcode_t::DEMI_OPC_PUSH => {
                server_log!("PUSH complete");
            },
            demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("operation failed"),
            _ => anyhow::bail!("unexpected result"),
        }
        
        // #[cfg(feature = "profiler")]
        // profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
        
        server_log!("******* APP: Okay, handled the results! *******");
    }

    eprintln!("server stopping");

    /* #[cfg(feature = "tcp-migration")]
    // Get the length of the vector
    let vec_len = queue_length_vec.len();

    // Calculate the starting index for the last 10,000 elements
    let start_index = if vec_len >= 5_000 {
        vec_len - 5_000
    } else {
        0 // If the vector has fewer than 10,000 elements, start from the beginning
    };

    // Create a slice of the last 10,000 elements
    let last_10_000 = &queue_length_vec[start_index..];

    // Iterate over the slice and print the elements
    let mut cnt = 0;
    for (idx, qlen) in last_10_000.iter() {
        println!("{},{}", cnt, qlen);
        cnt+=1;
    } */
    #[cfg(feature = "tcp-migration")]
    demi_print_queue_length_log();

    // LibOS::capylog_dump(&mut std::io::stderr().lock());

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &String) {
    println!("Usage: {} address\n", program_name);
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    server_log!("*** HTTP SERVER LOGGING IS ON ***");
    // logging::initialize();

    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let sockaddr: SocketAddr = SocketAddr::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
