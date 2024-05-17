// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    XSK_RING_INFO,
    XSK_RING_FLAGS,
};

use std::{
    ops::{
        Index,
        IndexMut,
    },
    iter::IntoIterator,
    sync::atomic::{
        AtomicPtr,
        AtomicIsize,
    },
};


macro_rules! define_ring {
    ( $name:ident ) => {
        pub struct $name<T> {
            cached_prod: u32,
            cached_cons: u32,
            mask: u32,
            size: u32,
            producer: *mut u32,
            consumer: *mut u32,
            ring_base: *mut T,
            flags: *mut u32,
        }
    }
}

define_ring!(XskRingProd);
define_ring!(XskRingCons);

pub struct ProdTransaction<'a, T> {
    ring: &'a XskRingProd<T>,
    size: u32,
}

impl<T> XskRingProd<T>
where
    T: Copy,
{
    
}
