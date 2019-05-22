use super::task::{Id, Status, Task};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::Instant;

#[derive(PartialEq, Eq)]
struct Record {
    when: Instant,
    tid: Id,
}

pub struct Schedule {
    heap: BinaryHeap<Record>,
}

impl Ord for Record {
    fn cmp(&self, other: &Record) -> Ordering {
        // `BinaryHeap` is a max-heap, so we need to reverse the order of comparisons in order to get `peek()` and `pop()` to return the smallest time.
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

impl Schedule {
    pub fn new() -> Schedule {
        Schedule {
            heap: BinaryHeap::new(),
        }
    }

    pub fn schedule(&mut self, t: &Task) {
        match t.status() {
            Status::Completed(_) => {
                panic!("attempt to schedule a completed task")
            }
            Status::AsleepUntil(when) => {
                self.heap.push(Record {
                    when: *when,
                    tid: *t.id(),
                });
            }
        }
    }

    pub fn pop_if_due(&mut self, now: Instant) -> Option<Id> {
        if let Some(rec) = self.heap.peek() {
            if rec.when < now {
                // next task due isn't due yet.
                None
            } else {
                // next task is due.
                let rec = self.heap.pop().unwrap();
                Some(rec.tid)
            }
        } else {
            // nothing in the heap.
            None
        }
    }
}
