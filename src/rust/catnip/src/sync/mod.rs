mod threadsafe;
mod threadunsafe;

#[cfg(feature = "threadunsafe")]
pub use self::threadunsafe::{
    SharedWaker,
    WakerU64,
};

#[cfg(not(feature = "threadunsafe"))]
pub use self::threadsafe::{
    SharedWaker,
    WakerU64,
};
