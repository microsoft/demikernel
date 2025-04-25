// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use windows::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
};

use crate::{catpowder::win::api::XdpApi, catpowder::win::ring::TxRing, runtime::fail::Fail};

//======================================================================================================================
// Functions
//======================================================================================================================

/// Returns the number of processor cores on the system. This is used to determine the number of RSS queues that can be
/// created.
fn count_processor_cores() -> Result<usize, Fail> {
    let mut proc_info: SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX = SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX::default();
    let mut buffer_len: u32 = 0;

    if let Err(e) =
        unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, Some(&mut proc_info), &mut buffer_len) }
    {
        if e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() {
            let cause: String = format!("GetLogicalProcessorInformationEx failed: {:?}", e);
            return Err(Fail::new(libc::EFAULT, &cause));
        }
    } else {
        return Err(Fail::new(
            libc::EFAULT,
            "GetLogicalProcessorInformationEx did not return any information",
        ));
    }

    let mut buf: Vec<u8> = vec![0; buffer_len as usize];
    if let Err(e) = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut buffer_len,
        )
    } {
        let cause: String = format!("GetLogicalProcessorInformationEx failed: {:?}", e);
        return Err(Fail::new(libc::EFAULT, &cause));
    }

    let mut core_count: usize = 0;
    let std::ops::Range {
        start: mut proc_core_info,
        end: proc_core_end,
    } = buf.as_ptr_range();
    while proc_core_info < proc_core_end && proc_core_info >= buf.as_ptr() {
        // Safety: the buffer is initialized to valid values by GetLogicalProcessorInformationEx, and the pointer is
        // not aliased. Bounds are checked above.
        let proc_info: &SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX =
            unsafe { &*(proc_core_info as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX) };
        if proc_info.Relationship == RelationProcessorCore {
            core_count += 1;
        }
        proc_core_info = proc_core_info.wrapping_add(proc_info.Size as usize);
    }

    return Ok(core_count);
}

/// Deduces the RSS settings for the given interface. Returns the number of valid RSS queues for the interface.
pub fn deduce_rss_settings(api: &mut XdpApi, ifindex: u32) -> Result<u32, Fail> {
    const DUMMY_QUEUE_LENGTH: u32 = 1;
    const DUMMY_BUFFER_COUNT: u32 = 1;
    const DUMMY_MTU: u16 = 500;
    let sys_proc_count: u32 = count_processor_cores()? as u32;

    // NB there will always be at least one queue available, hence starting the loop at 1. There should not be more
    // queues than the number of processors on the system.
    for queueid in 1..sys_proc_count {
        match TxRing::new(
            api,
            DUMMY_QUEUE_LENGTH,
            DUMMY_BUFFER_COUNT,
            DUMMY_MTU,
            ifindex,
            queueid,
            false,
        ) {
            Ok(_) => (),
            Err(e) => {
                warn!(
                    "Failed to create TX ring on queue {}: {:?}. This is only an error if {} is a valid RSS queue \
                     ID",
                    queueid, e, queueid
                );
                return Ok(queueid);
            },
        }
    }

    Ok(sys_proc_count)
}
