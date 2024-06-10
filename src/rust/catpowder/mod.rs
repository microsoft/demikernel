// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(target_os = "windows")]
pub mod win;

pub use win::runtime::CatpowderRuntime;
