use super::coroutine::{Coroutine, CoroutineId, CoroutineStatus};
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashSet},
    time::Instant,
};

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
    ids: HashSet<CoroutineId>,
    heap: BinaryHeap<Record>,
    clock: Instant,
}

impl Schedule {
    pub fn new(now: Instant) -> Schedule {
        Schedule {
            ids: HashSet::new(),
            heap: BinaryHeap::new(),
            clock: now,
        }
    }

    pub fn schedule<'a>(&mut self, t: &Coroutine<'a>) {
        match t.status() {
            CoroutineStatus::AsleepUntil(when) => {
                self.ids.insert(t.id());
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

    pub fn cancel(&mut self, id: CoroutineId) {
        self.ids.remove(&id);
    }

    pub fn poll(&mut self, now: Instant) -> Option<CoroutineId> {
        trace!("Schedule::poll({:?})", now);
        assert!(self.clock <= now);
        self.clock = now;

        if let Some(rec) = self.heap.peek() {
            if rec.when > now {
                // next coroutine isn't due yet.
                debug!(
                    "no coroutines due (next is #{}, due in {:?})",
                    rec.cid,
                    rec.when - now,
                );
                None
            } else {
                // next coroutine is due.
                let rec = self.heap.pop().unwrap();
                if self.ids.contains(&rec.cid) {
                    debug!("coroutine is #{} due", rec.cid);
                    self.cancel(rec.cid);
                    Some(rec.cid)
                } else {
                    debug!(
                        "coroutine #{} would have been due but was cancelled",
                        rec.cid
                    );
                    // coroutine is due but was cancelled; discard and try
                    // again.
                    self.poll(now)
                }
            }
        } else {
            debug!("no coroutines scheduled");
            // nothing in the heap.
            None
        }
    }
}
