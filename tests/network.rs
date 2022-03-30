// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::demikernel::LibOS;
use std::ptr::null_mut;

//==============================================================================
// Unit Various Network Helper Functions
//==============================================================================

/// Tests getaddrinfo().
#[test]
fn test_unit_getaddrinfo() {
    let libos: LibOS = LibOS::new();

    let hints: libc::addrinfo = libc::addrinfo {
        ai_flags: libc::AI_PASSIVE,
        ai_family: libc::AF_UNSPEC,
        ai_socktype: libc::SOCK_DGRAM,
        ai_protocol: 0,
        ai_addrlen: 0,
        ai_addr: null_mut(),
        ai_canonname: null_mut(),
        ai_next: null_mut(),
    };

    let node: Option<&str> = None;
    let service: Option<&str> = Some("80");
    let hints: Option<&libc::addrinfo> = Some(&hints);

    let ai: *mut libc::addrinfo = match libos.getaddrinfo(node, service, hints) {
        Ok(ai) => ai,
        Err(e) => panic!("{:?}", e.cause),
    };

    // Print address information list.
    let mut next: *mut libc::addrinfo = ai;
    while !next.is_null() {
        unsafe {
            println!("{:?}", (*next));
            next = (*next).ai_next;
        }
    }

    match libos.freeaddrinfo(ai) {
        Ok(_) => (),
        Err(e) => panic!("{:?}", e.cause),
    };
}
