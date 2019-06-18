use crate::{
    prelude::*,
    r#async::{Async, Future},
};
use std::{
    any::Any,
    cell::RefCell,
    collections::VecDeque,
    fmt::Debug,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Runtime<'a> {
    options: Rc<Options>,
    effects: Rc<RefCell<VecDeque<Effect>>>,
    r#async: Async<'a>,
}

impl<'a> Runtime<'a> {
    pub fn from_options(now: Instant, options: Options) -> Runtime<'a> {
        Runtime {
            options: Rc::new(options),
            effects: Rc::new(RefCell::new(VecDeque::new())),
            r#async: Async::new(now),
        }
    }

    pub fn options(&self) -> Rc<Options> {
        self.options.clone()
    }

    pub fn clock(&self) -> Instant {
        self.r#async.clock()
    }

    pub fn start_coroutine<G, T>(&self, gen: G) -> Future<'a, T>
    where
        T: Any + Clone + Debug + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        self.r#async.start_coroutine(gen)
    }

    pub fn poll(&self, now: Instant) -> Option<Effect> {
        let _ = self.r#async.poll(now);
        self.effects.borrow_mut().pop_front()
    }

    pub fn emit_effect(&self, effect: Effect) {
        self.effects.borrow_mut().push_back(effect)
    }
}
