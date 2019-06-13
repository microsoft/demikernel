use custom_error::custom_error;
use float_duration;
use std::{io::Error as IoError, num::TryFromIntError, rc::Rc};

custom_error! {#[derive(Clone)] pub Fail
    IoError{source: Rc<IoError>} = "I/O failure",
    Misdelivered{} = "misdelivered packet",
    Unsupported{} = "unsupported",
    Ignored{} = "operation has no effect",
    TryAgain{} = "try again later",
    Timeout{} = "an asynchronous operation timed out",
    OutOfRange{} = "a value is out of range",
    Malformed{} = "received a malformed packet",
}

impl From<IoError> for Fail {
    fn from(e: IoError) -> Self {
        Fail::IoError { source: Rc::new(e) }
    }
}

impl From<float_duration::error::OutOfRangeError> for Fail {
    fn from(_: float_duration::error::OutOfRangeError) -> Self {
        Fail::OutOfRange {}
    }
}

impl From<TryFromIntError> for Fail {
    fn from(_: TryFromIntError) -> Self {
        Fail::OutOfRange {}
    }
}
