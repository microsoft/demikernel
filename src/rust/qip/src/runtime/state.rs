use crate::prelude::*;
use std::collections::VecDeque;

pub struct RuntimeState {
    options: Options,
    effects: VecDeque<Effect>,
}

impl RuntimeState {
    pub fn from_options(options: Options) -> RuntimeState {
        RuntimeState {
            options,
            effects: VecDeque::new(),
        }
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    pub fn poll(&mut self) -> Option<Effect> {
        self.effects.pop_front()
    }

    pub fn emit(&mut self, effect: Effect) {
        self.effects.push_back(effect)
    }
}
