#[macro_use]
mod macros;

mod coroutine;
mod future;
mod retry;
mod runtime;
mod schedule;
mod traits;

use crate::prelude::*;
use std::{any::Any, rc::Rc};

pub use future::{Future, WhenAny};
pub use retry::Retry;
pub use runtime::AsyncRuntime as Runtime;
pub use traits::Async;

#[allow(non_snake_case)]
pub fn CoroutineOk<T>(value: T) -> Result<Rc<dyn Any>>
where
    T: 'static,
{
    Ok(Rc::new(value))
}
