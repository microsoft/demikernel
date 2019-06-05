use custom_error::custom_error;
use std::io::Error as IoError;
use std::rc::Rc;
use float_duration;

custom_error! {#[derive(Clone)] pub Fail
    IoError{source: Rc<IoError>} = "I/O failure",
    Misdelivered{} = "misdelivered packet",
    Unsupported{} = "unsupported",
    Ignored{} = "operation has no effect",
    TryAgain{} = "try again later",
    Timeout{} = "an asynchronous operation timed out",
    OutOfRange{} = "a value is out of range",
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
