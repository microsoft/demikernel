// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod intrusive;

cfg_if! {
    if #[cfg(feature = "catmem-libos")] {
        pub mod raw_array;
        pub mod ring;
        pub mod shared_ring;
    }
}
