// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// todo: remove once all aliases are referenced.
#![allow(dead_code)]

use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;

pub type Rng = ChaChaRng;
pub type Seed = <ChaChaRng as SeedableRng>::Seed;
