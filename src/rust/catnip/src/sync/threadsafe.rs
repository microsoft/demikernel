#![allow(unused)]

use futures::task::AtomicWaker;
use std::{
    fmt,
    ops::{
        Deref,
        DerefMut,
    },
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
    },
    task::Waker,
};

pub struct SharedWaker(Arc<AtomicWaker>);

impl Clone for SharedWaker {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl SharedWaker {
    pub fn new() -> Self {
        Self(Arc::new(AtomicWaker::new()))
    }

    pub fn register(&self, waker: &Waker) {
        self.0.register(waker);
    }

    pub fn wake(&self) {
        self.0.wake();
    }
}

pub struct WakerU64(AtomicU64);

impl WakerU64 {
    pub fn new(val: u64) -> Self {
        WakerU64(AtomicU64::new(val))
    }

    pub fn fetch_or(&self, val: u64) {
        self.0.fetch_or(val, Ordering::SeqCst);
    }

    pub fn fetch_and(&self, val: u64) {
        self.0.fetch_and(val, Ordering::SeqCst);
    }

    pub fn fetch_add(&self, val: u64) -> u64 {
        self.0.fetch_add(val, Ordering::SeqCst)
    }

    pub fn fetch_sub(&self, val: u64) -> u64 {
        self.0.fetch_sub(val, Ordering::SeqCst)
    }

    pub fn load(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    pub fn swap(&self, val: u64) -> u64 {
        self.0.swap(val, Ordering::SeqCst)
    }
}
