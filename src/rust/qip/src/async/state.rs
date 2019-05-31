use super::{
    schedule::Schedule,
    task::{Task, TaskId},
};
use crate::prelude::*;
use std::{
    collections::HashMap,
    ops::Generator,
    time::{Duration, Instant},
};

pub struct AsyncState<'a, T> {
    next_unused_id: u64,
    tasks: HashMap<TaskId, Task<'a, T>>,
    schedule: Schedule,
    clock: Instant,
}

impl<'a, T> AsyncState<'a, T>
where
    T: Clone,
{
    pub fn new(now: Instant) -> AsyncState<'a, T> {
        AsyncState {
            next_unused_id: 0,
            tasks: HashMap::new(),
            schedule: Schedule::default(),
            clock: now,
        }
    }

    pub fn start_task<G>(&mut self, gen: G) -> TaskId
    where
        G: Generator<Yield = Option<Duration>, Return = Result<T>>
            + 'a
            + Unpin,
    {
        let tid = self.new_tid();
        let t = Task::new(tid, gen, self.clock);
        self.schedule.schedule(&t);
        self.tasks.insert(tid, t);
        tid
    }

    fn new_tid(&mut self) -> TaskId {
        let tid = TaskId::from(self.next_unused_id);
        // todo: we should deal with overflow.
        self.next_unused_id += self.next_unused_id;
        tid
    }

    pub fn poll(&mut self, now: Instant) -> Result<TaskId> {
        eprintln!("# async::State::poll()");
        assert!(now >= self.clock);
        self.clock = now;
        if let Some(tid) = self.schedule.poll(now) {
            eprintln!(
                "# async::Schedule::poll() returned a task (tid = {})",
                tid
            );
            // we don't anticipate a reasonable situation where the schedule
            // would give us an ID that isn't in `self.tasks`.
            let task = self.tasks.get_mut(&tid).unwrap();
            if let Err(Fail::TryAgain {}) = task.resume(now) {
                self.schedule.schedule(task);
            }

            Ok(task.id())
        } else {
            Err(Fail::TryAgain {})
        }
    }

    pub fn drop_task(&mut self, tid: TaskId) {
        self.schedule.cancel(tid);
        assert!(self.tasks.remove(&tid).is_some());
    }

    pub fn get_task(&self, tid: TaskId) -> &Task<'a, T> {
        self.tasks.get(&tid).unwrap()
    }
}
