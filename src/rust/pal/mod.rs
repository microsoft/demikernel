// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// PAL: Platform Abstraction Layer
//======================================================================================================================

// This is the platform abstraction layer designed to hide the platform specific implementation details of contants and
// datastructures. This layer exists because of the differences between the Windows/Linux libc/WinSock implementations
// of the corresponding Rust libraries.

pub mod arch;
pub mod constants;
pub mod data_structures;
pub mod functions;

#[cfg(target_os = "linux")]
pub mod linux;
