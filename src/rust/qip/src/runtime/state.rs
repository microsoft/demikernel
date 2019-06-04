use crate::{
    prelude::*,
    r#async::{Async, Future},
};
use std::{
    any::Any,
    collections::VecDeque,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct RuntimeState<'a> {
    options: Rc<Options>,
    effects: VecDeque<Effect>,
    r#async: Async<'a>,
}

impl<'a> RuntimeState<'a> {
    pub fn from_options(now: Instant, options: Options) -> RuntimeState<'a> {
        RuntimeState {
            options: Rc::new(options),
            effects: VecDeque::new(),
            r#async: Async::new(now),
        }
    }

    pub fn options(&self) -> Rc<Options> {
        self.options.clone()
    }

    pub fn start_task<G, T>(&self, gen: G) -> Future<'a, T>
    where
        T: Any + Clone + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        self.r#async.start_task(gen)
    }

    pub fn poll(&mut self, now: Instant) -> Option<Effect> {
        self.r#async.service(now);
        self.effects.pop_front()
    }

    pub fn emit_effect(&mut self, effect: Effect) {
        self.effects.push_back(effect)
    }
}
