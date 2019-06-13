mod future;
mod schedule;
mod task;

use crate::prelude::*;
use schedule::Schedule;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::Debug,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};
use task::{Task, TaskId, TaskStatus};

pub use future::Future;

#[derive(Clone)]
pub struct Async<'a> {
    next_unused_id: Rc<Cell<u64>>,
    tasks: Rc<RefCell<HashMap<TaskId, Task<'a>>>>,
    schedule: Rc<RefCell<Schedule>>,
}

impl<'a> Async<'a> {
    pub fn new(now: Instant) -> Self {
        Async {
            next_unused_id: Rc::new(Cell::new(0)),
            tasks: Rc::new(RefCell::new(HashMap::new())),
            schedule: Rc::new(RefCell::new(Schedule::new(now))),
        }
    }

    pub fn clock(&self) -> Instant {
        self.schedule.borrow().clock()
    }

    pub fn start_task<G, T>(&self, gen: G) -> Future<'a, T>
    where
        T: Any + Clone + Debug + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        let tid = self.new_tid();
        let t = Task::new(tid, gen, self.clock());
        self.schedule.borrow_mut().schedule(&t);
        self.tasks.borrow_mut().insert(tid, t);
        let fut = Future::task_result(self.clone(), tid);
        let _ = fut.poll(self.clock());
        fut
    }

    fn new_tid(&self) -> TaskId {
        let n = self.next_unused_id.get();
        let tid = TaskId::from(n);
        // todo: we should deal with overflow.
        self.next_unused_id.set(n + 1);
        tid
    }

    pub fn poll_schedule(&self, now: Instant) -> Option<TaskId> {
        // we had to extract this into its own function to limit the scope
        // of the mutable borrow (it was causing borrowing deadlocks).
        self.schedule.borrow_mut().poll(now)
    }

    pub fn poll(&self, now: Instant) -> Result<TaskId> {
        trace!("entering `Async::poll({:?})`", now);
        if let Some(tid) = self.poll_schedule(now) {
            debug!("Async::poll_schedule() returned a task (tid = {})", tid);
            let mut task = {
                let mut tasks = self.tasks.borrow_mut();
                // task has to be removed from tasks in order to work around a
                // mutablility deadlock when futures are used from within a
                // coroutine. we also don't anticipate a reasonable situation
                // where the schedule would give us an ID that isn't in
                // `self.tasks`.
                tasks.remove(&tid).unwrap()
            };

            if !task.resume(now) {
                self.schedule.borrow_mut().schedule(&task);
            }

            let tid = task.id();
            debug!(
                "coroutine {} successfully resumed; status is now `{:?}`",
                tid,
                task.status()
            );
            let mut tasks = self.tasks.borrow_mut();
            assert!(tasks.insert(tid, task).is_none());
            Ok(tid)
        } else {
            Err(Fail::TryAgain {})
        }
    }

    pub fn drop_task(&self, tid: TaskId) {
        self.schedule.borrow_mut().cancel(tid);
        assert!(self.tasks.borrow_mut().remove(&tid).is_some());
    }

    pub fn task_status(&self, tid: TaskId) -> TaskStatus {
        self.tasks.borrow().get(&tid).unwrap().status().clone()
    }
}
