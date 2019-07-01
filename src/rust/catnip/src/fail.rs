use custom_error::custom_error;
use float_duration;
use std::{
    cell::BorrowMutError, io::Error as IoError, num::TryFromIntError, rc::Rc,
};

// the following type alias is needed because the `custom_error!` macro doesn't
// allow `&` in type specifications.
type Str = &'static str;

custom_error! {#[derive(Clone)] pub Fail
    IoError{source: Rc<IoError>} = "{source}",
    BorrowMutError{source: Rc<BorrowMutError>} = "{source}",
    Misdelivered{} = "misdelivered datagram",
    Unsupported{details: Str} = "unsupported ({details})",
    Ignored{} = "operation has no effect",
    Timeout{} = "an asynchronous operation timed out",
    OutOfRange{} = "a value is out of range",
    Malformed{details: Str} = "encountered a malformed datagram ({details})",
    TypeMismatch{} = "type mismatch",
    Unimplemented{} = "not yet implemented",
}

impl From<IoError> for Fail {
    fn from(e: IoError) -> Self {
        Fail::IoError { source: Rc::new(e) }
    }
}

impl From<BorrowMutError> for Fail {
    fn from(e: BorrowMutError) -> Self {
        Fail::BorrowMutError { source: Rc::new(e) }
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
