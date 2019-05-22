use super::schedule::Schedule;
use super::task::{Id, Task};
use crate::prelude::*;
use std::{
    collections::HashMap,
    ops::Generator,
    time::{Duration, Instant},
};

pub struct State<'a> {
    next_unused_id: u64,
    tasks: HashMap<Id, Task<'a>>,
    schedule: Schedule,
}

impl<'a> State<'a> {
    pub fn new() -> State<'a> {
        State {
            next_unused_id: 0,
            tasks: HashMap::new(),
            schedule: Schedule::new(),
        }
    }

    pub fn new_task<G>(&mut self, gen: G, now: Instant) -> Id
    where
        G: Generator<Yield = Option<Duration>, Return = Result<()>>
            + 'a
            + Unpin,
    {
        let id = self.new_id();
        let t = Task::new(id.clone(), gen, now);
        self.schedule.schedule(&t);
        self.tasks.insert(id.clone(), t);
        id
    }

    fn new_id(&mut self) -> Id {
        let id = Id::from(self.next_unused_id);
        // todo: we should deal with overflow.
        self.next_unused_id = self.next_unused_id + 1;
        id
    }

    pub fn resume(&mut self, now: Instant) {
        if let Some(id) = self.schedule.pop_if_due(now) {}
    }
}
