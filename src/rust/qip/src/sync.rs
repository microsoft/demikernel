use spin;
use std::sync;

pub use spin::Mutex;

pub type Arc<T> = sync::Arc<T>;
