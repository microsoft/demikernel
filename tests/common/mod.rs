// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

cfg_if::cfg_if! {
    if #[cfg(feature = "catnip-libos")] {
        mod catnip;
        pub use self::catnip::Test;
    } else {
        mod catnap;
        pub use self::catnap::Test;
    }
}
