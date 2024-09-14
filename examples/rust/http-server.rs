// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use demikernel::runtime::types::demi_qresult_t;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::{
    env,
    net::SocketAddr,
    slice,
    str::FromStr,
    time::Duration,
    collections::HashMap,
    cell::RefCell,
    rc::Rc,
};
use log::{
    error,
    warn,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use colored::Colorize;

#[macro_use]
extern crate lazy_static;

//=====================================================================================
macro_rules! server_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "ih-log")]
        if let Ok(val) = std::env::var("IH_LOG") {
            if val == "1" {
                eprintln!("{}", format!($($arg)*).green());
            }
        }
    };
}
//=====================================================================================

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;
const FILL_CHAR: u8 = 0x65;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

//======================================================================================================================
// mksga()
//======================================================================================================================

/// Creates a scatter-gather-array.
fn mksga(libos: &mut LibOS, data: &[u8]) -> Result<demi_sgarray_t> {
    let size = data.len();
    debug_assert!(size > std::mem::size_of::<u64>());

    let sga: demi_sgarray_t = libos.sgaalloc(size)?;
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;

    if size > len {
        freesga(libos, sga);
        anyhow::bail!("Allocated SGA segment is smaller than the input data");
    }

    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };

    // Copy the data into the slice
    slice[0..size].copy_from_slice(data);

    Ok(sga)
}

//======================================================================================================================
// freesga()
//======================================================================================================================

/// Free scatter-gather array and warn on error.
fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        error!("sgafree() failed (error={:?})", e);
        warn!("leaking sga");
    }
}

//======================================================================================================================
// close()
//======================================================================================================================

/// Closes a socket and warns if not successful.
fn close(libos: &mut LibOS, sockqd: QDesc) {
    if let Err(e) = libos.close(sockqd) {
        error!("close() failed (error={:?})", e);
        warn!("leaking sockqd={:?}", sockqd);
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
    //     Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => {
    //         server_log!("PUSH DONE");
    //         ()
    //     },
    //     Ok(_) => anyhow::bail!("unexpected result"),
    //     Err(e) => anyhow::bail!("operation failed: {:?}", e),
    // };

    Ok(())
}



const ROOT: &str = "/var/www/demo";
const BUFSZ: usize = 8192 * 2;

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
    pub fn buf_size(&self) {
        eprintln!("buf_size: {}", self.buf.len())
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

        // if self.head < self.buf.len() {
        //     return Ok(());
        // }

        if self.data_size() == self.buf.len() {
            panic!("Need larger buffer for HTTP messages");
        }

        self.buf.copy_within(self.tail..self.head, 0);
        self.head = self.data_size();
        self.tail = 0;
        Ok(())
    }
}

struct SessionData {
    data: Vec<u8>,
}

impl SessionData {
    fn new(size: usize) -> Self {
        Self { data: vec![5; size] }
    }
}
// The TCP server.
struct ConnectionState {
    buffer: Buffer,
    session_data: SessionData,
}


#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub struct TcpServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket queue descriptor.
    sockqd: QDesc,
    /// Accepted socket queue descriptor.
    accepted_qd: Option<QDesc>,
    /// The scatter-gather array.
    sga: Option<demi_sgarray_t>,
}

// Implementation of the TCP server.
impl TcpServer {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        // Create the local socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self {
            libos,
            sockqd,
            accepted_qd: None,
            sga: None,
        });
    }

    pub fn respond_to_request(&mut self, qd: QDesc, data: &[u8]) -> QToken {
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
    
        let sga: demi_sgarray_t = mksga(&mut self.libos, RESPONSE.as_bytes()).unwrap();
        let qt = self.libos.push(qd, &sga).expect("push success");
        
        // if let Err(e) = push_and_wait(
        //     &mut self.libos,
        //     qd,
        //     &sga,
        // ) {
        //     anyhow::bail!("push and wait failed: {:?}", e)
        // }
        if let Err(e) = self.libos.sgafree(sga) {
            panic!("failed to release scatter-gather array: {:?}", e);
        }
        // Ok(())
        return qt;
    }
    

    pub fn push_data_and_run(&mut self, qd: QDesc, buffer: &mut Buffer, data: &[u8], qts: &mut Vec<QToken>) -> usize {
    
        // fast path: no previous data in the stream and this request contains exactly one HTTP request
        if buffer.data_size() == 0 {
            if find_subsequence(data, b"\r\n\r\n").unwrap_or(data.len()) == data.len() - 4 {
                // if let Err(e) = self.respond_to_request(qd, data) {
                //     anyhow::bail!("respond_to_request failed: {:?}", e);
                // }
                // return Ok(());
                let resp_qt = self.respond_to_request(qd, data);
                qts.push(resp_qt);
                return 1;
            }
        }
        // println!("* CHECK * data size: {}, buffer size: {}\n", data.len(), buffer.data_size());
        // buffer.buf_size();
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
                    // if let Err(e) = self.respond_to_request(qd, data) {
                    //     anyhow::bail!("respond_to_request failed: {:?}", e);
                    // }
                    let resp_qt = self.respond_to_request(qd, &dbuf[..idx + 4]);
                    qts.push(resp_qt);
                    buffer.pull_data(idx + 4);
                    // buffer.try_shrink().unwrap();
                    // return Ok(());
                    sent += 1;
                }
                None => {
                    buffer.try_shrink().unwrap();
                    // return Ok(());
                    return sent;
                }
            }
        }
    }

    pub fn run(&mut self, local: SocketAddr, fill_char: u8, buffer_size: usize) -> Result<()> {
        let session_data_size: usize = std::env::var("SESSION_DATA_SIZE").map_or(1024, |v| v.parse().unwrap());

        if let Err(e) = self.libos.bind(self.sockqd, local) {
            anyhow::bail!("bind failed: {:?}", e.cause)
        };
        

        // Mark as a passive one.
        if let Err(e) = self.libos.listen(self.sockqd, 300) {
            anyhow::bail!("listen failed: {:?}", e.cause)
        };
        server_log!("LibOS BIND and LISTEN to {:?}", local);

        let mut qts: Vec<QToken> = Vec::new();
        let mut connstate: HashMap<QDesc, Rc<RefCell<ConnectionState>>> = HashMap::new();
        // Accept incoming connections.
        let qt: QToken = match self.libos.accept(self.sockqd) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("accept failed: {:?}", e.cause),
        };
        
        qts.push(qt);
        server_log!("issue ACCEPT");

        // demi_qresult_t qr = {qr_opcode: 0, 0, qt, 0, 0};
        // Create qrs filled with garbage.
        let mut qrs: Vec<demi_qresult_t> = Vec::with_capacity(2000);
        // qrs.resize_with(2000, || demi_qresult_t::default());
        unsafe {
            qrs.set_len(2000);
        }
        let mut indices: Vec<usize> = Vec::with_capacity(2000);
        indices.resize(2000, 0);


        // ctrlc::set_handler(move || {
        //     eprintln!("Received Ctrl-C signal.");
        //     std::process::exit(0);
        // }).expect("Error setting Ctrl-C handler");


        loop {
            // let (offset, qr) = self.libos.wait_any(&qts, None).expect("result");
            let result_count = self.libos.wait_any_n(&qts, &mut qrs, &mut indices, Some(DEFAULT_TIMEOUT)).expect("result");
            server_log!("\n\n****** wait_any --> OS: take complete results! ******");
            if result_count == 0{
                break;
            }
            let results = &qrs[..result_count];
            let indices = &indices[..result_count];

            for (index, qr) in indices.iter().zip(results.iter()).rev() {
                qts.swap_remove(*index);
                let qd = qr.qr_qd.into();

                match qr.qr_opcode {
                    demi_opcode_t::DEMI_OPC_ACCEPT  => {
                        let new_qd = unsafe { qr.qr_value.ares.qd.into() };
                        match self.libos.pop(new_qd, None) {
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
                        qts.push(self.libos.accept(qd).expect("accept qtoken"));
                        server_log!("ACCEPT {:?} complete ==> issue POP and ACCEPT", new_qd);
                        server_log!("remaining Qtokens: {}", qts.len());
                    },
                    demi_opcode_t::DEMI_OPC_POP => {
                        let sga = unsafe { qr.qr_value.sga };
                        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
                        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
                        let recvbuf: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
                        server_log!("POP {:?} complete, data: {:?}", qd, std::str::from_utf8(recvbuf).unwrap());
                        
                        // match std::str::from_utf8(recvbuf) {
                        //     Ok(human_readable) => eprintln!("POP: {:?}", human_readable),
                        //     Err(e) => eprintln!("POP: invalid UTF-8 sequence: {:?}", e),
                        // }
                        let mut state = connstate.get_mut(&qd).unwrap().borrow_mut();
                        let sent = self.push_data_and_run(qd, &mut state.buffer, &recvbuf, &mut qts);
                        
                        match self.libos.pop(qd, None) {
                            Ok(pop_qt) => {
                                qts.push(pop_qt)
                            },
                            Err(e) => panic!("pop qt: {}", e),
                        }
                        server_log!("issue POP");
                        // Do not silently ignore if unable to free scatter-gather array.
                        if let Err(e) = self.libos.sgafree(sga) {
                            anyhow::bail!("failed to release scatter-gather array: {:?}", e);
                        }
                        server_log!("remaining Qtokens: {}", qts.len());
                    },
                    demi_opcode_t::DEMI_OPC_PUSH => {
                        server_log!("PUSH complete");
                    },
                    demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("operation failed"),
                    _ => anyhow::bail!("unexpected result"),
                }
            }

            // qts.swap_remove(offset);
            // let qd = qr.qr_qd.into();
            
            // match qr.qr_opcode {
            //     demi_opcode_t::DEMI_OPC_ACCEPT  => {
            //         let new_qd = unsafe { qr.qr_value.ares.qd.into() };
            //         match self.libos.pop(new_qd, None) {
            //             Ok(pop_qt) => {
            //                 qts.push(pop_qt)
            //             },
            //             Err(e) => panic!("pop qt: {}", e),
            //         }
            //         let state = Rc::new(RefCell::new(ConnectionState {
            //             buffer: Buffer::new(),
            //             session_data: SessionData::new(session_data_size),
            //         }));
            //         connstate.insert(new_qd, state);
            //         qts.push(self.libos.accept(qd).expect("accept qtoken"));
            //         server_log!("ACCEPT {:?} complete ==> issue POP and ACCEPT", new_qd);
            //         server_log!("remaining Qtokens: {}", qts.len());
            //     },
            //     demi_opcode_t::DEMI_OPC_POP => {
            //         let sga = unsafe { qr.qr_value.sga };
            //         let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            //         let len: usize = sga.sga_segs[0].sgaseg_len as usize;
            //         let recvbuf: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            //         server_log!("POP {:?} complete, data: {:?}", qd, std::str::from_utf8(recvbuf).unwrap());
                    
            //         // match std::str::from_utf8(recvbuf) {
            //         //     Ok(human_readable) => eprintln!("POP: {:?}", human_readable),
            //         //     Err(e) => eprintln!("POP: invalid UTF-8 sequence: {:?}", e),
            //         // }
            //         let mut state = connstate.get_mut(&qd).unwrap().borrow_mut();
            //         let sent = self.push_data_and_run(qd, &mut state.buffer, &recvbuf, &mut qts);
                    
            //         match self.libos.pop(qd, None) {
            //             Ok(pop_qt) => {
            //                 qts.push(pop_qt)
            //             },
            //             Err(e) => panic!("pop qt: {}", e),
            //         }
            //         server_log!("issue POP");
            //         // Do not silently ignore if unable to free scatter-gather array.
            //         if let Err(e) = self.libos.sgafree(sga) {
            //             anyhow::bail!("failed to release scatter-gather array: {:?}", e);
            //         }
            //         server_log!("remaining Qtokens: {}", qts.len());
            //     },
            //     demi_opcode_t::DEMI_OPC_PUSH => {
            //         server_log!("PUSH complete");
            //     },
            //     demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("operation failed"),
            //     _ => anyhow::bail!("unexpected result"),
            // }
        }
        eprintln!("server stopping");

        // LibOS::capylog_dump(&mut std::io::stderr().lock());

        // #[cfg(feature = "profiler")]
        // profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

        // TODO: close socket when we get close working properly in catnip.
        Ok(())
    }
}

// The Drop implementation for the TCP server.
impl Drop for TcpServer {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);

        if let Some(accepted_qd) = self.accepted_qd {
            close(&mut self.libos, accepted_qd);
        }

        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
    }
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
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        // Create the LibOS.
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        let libos: LibOS = match LibOS::new(libos_name, None) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
        };
        let sockaddr: SocketAddr = SocketAddr::from_str(&args[1])?;
        let mut server: TcpServer = TcpServer::new(libos)?;
        return server.run(sockaddr, FILL_CHAR, BUFFER_SIZE);
    }

    usage(&args[0]);

    Ok(())
}
