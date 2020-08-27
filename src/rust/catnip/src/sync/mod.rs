mod threadsafe;
mod threadunsafe;

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
