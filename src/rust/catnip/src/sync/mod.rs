mod threadsafe;
mod threadunsafe;

use futures_intrusive::{
    buffer::GrowingHeapBuf,
    channel::shared::{
        GenericReceiver,
        GenericSender,
    },
    NoopLock,
};

#[cfg(feature = "threadunsafe")]
pub use self::threadunsafe::{
    Bytes,
    BytesMut,
    SharedWaker,
    WakerU64,
};

#[cfg(not(feature = "threadunsafe"))]
pub use self::threadsafe::{
    Bytes,
    BytesMut,
    SharedWaker,
    WakerU64,
};

pub type UnboundedSender<T> = GenericSender<NoopLock, T, GrowingHeapBuf<T>>;
pub type UnboundedReceiver<T> = GenericReceiver<NoopLock, T, GrowingHeapBuf<T>>;
