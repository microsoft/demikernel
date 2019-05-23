use super::{
    schedule::Schedule,
    task::{Id, Task},
};
use crate::prelude::*;
use std::{
    collections::HashMap,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct State<'a, T> {
    next_unused_id: u64,
    tasks: HashMap<Id, Task<'a, T>>,
    schedule: Schedule,
}

impl<'a, T> State<'a, T> {
    pub fn new() -> State<'a, T> {
        State {
            next_unused_id: 0,
            tasks: HashMap::new(),
            schedule: Schedule::default(),
        }
    }

    pub fn new_task<G>(&mut self, gen: G, now: Instant) -> Id
    where
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<T>>>
            + 'a
            + Unpin,
    {
        let id = self.new_id();
        let t = Task::new(id, gen, now);
        self.schedule.schedule(&t);
        self.tasks.insert(id, t);
        id
    }

    fn new_id(&mut self) -> Id {
        let id = Id::from(self.next_unused_id);
        // todo: we should deal with overflow.
        self.next_unused_id += self.next_unused_id;
        id
    }

    pub fn poll(&mut self, now: Instant) -> Result<Rc<T>> {
        if let Some(id) = self.schedule.poll(now) {
            // we don't anticipate a reasonable situation where the schedule
            // would give us an ID that isn't in `self.tasks`.
            let task = self.tasks.get_mut(&id).unwrap();
            task.resume(now)
        } else {
            Err(Fail::TryAgain {})
        }
    }

    pub fn drop_task(&mut self, id: Id) {
        self.schedule.cancel(id);
        assert!(self.tasks.remove(&id).is_some());
    }
}
