// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::prelude::*;
use std::time::Instant;

pub trait Async<T> {
    fn poll(&self, now: Instant) -> Option<Result<T>>;
}
