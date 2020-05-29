// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::coroutine::{Coroutine, CoroutineId, CoroutineStatus};
use std::{cmp::Ordering, collections::BinaryHeap, time::Instant};

#[derive(PartialEq, Eq)]
struct Record {
    when: Instant,
    cid: CoroutineId,
}

impl Ord for Record {
    fn cmp(&self, other: &Record) -> Ordering {
        // `BinaryHeap` is a max-heap, so we need to reverse the order of
        // comparisons in order to get `peek()` and `pop()` to return the
        // smallest time.
        match self.when.cmp(&other.when) {
            Ordering::Equal => Ordering::Equal,
            Ordering::Less => Ordering::Greater,
            Ordering::Greater => Ordering::Less,
        }
    }
}

impl PartialOrd for Record {
    fn partial_cmp(&self, other: &Record) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct Schedule {
    heap: BinaryHeap<Record>,
    clock: Instant,
}

impl Schedule {
    pub fn new(now: Instant) -> Schedule {
        Schedule {
            heap: BinaryHeap::new(),
            clock: now,
        }
    }

    pub fn schedule<'a>(&mut self, t: &Coroutine<'a>) {
        match t.status() {
            CoroutineStatus::AsleepUntil(when) => {
                self.heap.push(Record {
                    when: *when,
                    cid: t.id(),
                });
            }
            CoroutineStatus::Active => {
                panic!("attempt to schedule an active coroutine")
            }
            CoroutineStatus::Completed(_) => {
                panic!("attempt to schedule a completed coroutine")
            }
        }
    }

    pub fn clock(&self) -> Instant {
        self.clock
    }

    pub fn poll(&mut self, now: Instant) -> Option<CoroutineId> {
        trace!("Schedule::poll({:?})", now);
        assert!(self.clock <= now);
        self.clock = now;

        match self.heap.peek() {
            Some(rec) if rec.when <= now => {
                let rec = self.heap.pop().unwrap();
                Some(rec.cid)
            }
            _ => None,
        }
    }
}
