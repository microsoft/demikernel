use super::task::{Task, TaskId, TaskStatus};
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashSet},
    time::Instant,
};

#[derive(PartialEq, Eq)]
struct Record {
    when: Instant,
    tid: TaskId,
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
    ids: HashSet<TaskId>,
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

    pub fn schedule<'a>(&mut self, t: &Task<'a>) {
        match t.status() {
            TaskStatus::Completed(_) => {
                panic!("attempt to schedule a completed task")
            }
            TaskStatus::AsleepUntil(when) => {
                self.ids.insert(t.id());
                self.heap.push(Record {
                    when: *when,
                    tid: t.id(),
                });
            }
        }
    }

    pub fn clock(&self) -> Instant {
        self.clock
    }

    pub fn cancel(&mut self, id: TaskId) {
        self.ids.remove(&id);
    }

    pub fn poll(&mut self, now: Instant) -> Option<TaskId> {
        assert!(self.clock <= now);
        self.clock = now;

        if let Some(rec) = self.heap.peek() {
            if rec.when > now {
                // next task isn't due yet.
                None
            } else {
                // next task is due.
                let rec = self.heap.pop().unwrap();
                if self.ids.contains(&rec.tid) {
                    self.cancel(rec.tid);
                    Some(rec.tid)
                } else {
                    // task is due but was cancelled; discard and try again.
                    self.poll(now)
                }
            }
        } else {
            // nothing in the heap.
            None
        }
    }
}
