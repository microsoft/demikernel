use crate::{prelude::*, r#async, rand::Rng};
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
    events: Rc<RefCell<VecDeque<Event>>>,
    options: Rc<Options>,
    r#async: r#async::Runtime<'a>,
    rng: Rc<RefCell<Rng>>,
}

impl<'a> Runtime<'a> {
    pub fn from_options(now: Instant, options: Options) -> Runtime<'a> {
        let rng = Rng::from_seed(options.decode_rng_seed());
        Runtime {
            options: Rc::new(options),
            events: Rc::new(RefCell::new(VecDeque::new())),
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

    pub fn emit_event(&self, event: Event) {
        let mut events = self.events.borrow_mut();
        debug!(
            "event emitted for {} (len is now {}) => {:?}",
            self.options.my_ipv4_addr,
            events.len() + 1,
            event
        );
        events.push_back(event);
    }

    pub fn rng_mut(&self) -> RefMut<Rng> {
        self.rng.borrow_mut()
    }
}

impl<'a> Async<Event> for Runtime<'a> {
    fn poll(&self, now: Instant) -> Option<Result<Event>> {
        while self.r#async.poll(now).is_some() {}
        let mut events = self.events.borrow_mut();
        let event = events.pop_front();
        debug!(
            "event popped from {}; {} remain(s) => {:?}",
            self.options.my_ipv4_addr,
            events.len(),
            event
        );
        event.map(Ok)
    }
}
