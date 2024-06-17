// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::LinuxRuntime as CatpowderRuntime;
