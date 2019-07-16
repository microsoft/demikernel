use crate::{
    prelude::*,
    r#async::{self, Async},
    rand::Rng,
};
use rand_core::SeedableRng;
use std::{
    any::Any,
    cell::{RefCell, RefMut},
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
    r#async: r#async::Runtime<'a>,
    rng: Rc<RefCell<Rng>>,
}

impl<'a> Runtime<'a> {
    pub fn from_options(now: Instant, options: Options) -> Runtime<'a> {
        let rng = Rng::from_seed(options.decode_rng_seed());
        Runtime {
            options: Rc::new(options),
            effects: Rc::new(RefCell::new(VecDeque::new())),
            r#async: r#async::Runtime::new(now),
            rng: Rc::new(RefCell::new(rng)),
        }
    }

    pub fn options(&self) -> Rc<Options> {
        self.options.clone()
    }

    pub fn now(&self) -> Instant {
        self.r#async.clock()
    }

    pub fn start_coroutine<G, T>(&self, gen: G) -> r#async::Future<'a, T>
    where
        T: Any + Clone + Debug + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<dyn Any>>>
            + 'a
            + Unpin,
    {
        self.r#async.start_coroutine(gen)
    }

    pub fn emit_effect(&self, effect: Effect) {
        let mut effects = self.effects.borrow_mut();
        debug!("effect emitted (len: {}) => {:?}", effects.len(), effect);
        effects.push_back(effect);
    }

    pub fn borrow_rng(&self) -> RefMut<Rng> {
        self.rng.borrow_mut()
    }
}

impl<'a> Async<Effect> for Runtime<'a> {
    fn poll(&self, now: Instant) -> Option<Result<Effect>> {
        try_poll!(self.r#async, now);
        let mut effects = self.effects.borrow_mut();
        let effect = effects.pop_front();
        debug!("effect popped; {} remain(s) => {:?}", effects.len(), effect);
        effect.map(Ok)
    }
}
