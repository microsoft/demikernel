use crate::protocols::icmpv4;
use custom_error::custom_error;
use float_duration;
use std::{
    cell::BorrowMutError, io::Error as IoError, num::TryFromIntError, rc::Rc,
};

custom_error! {#[derive(Clone)] pub Fail
    IoError{source: Rc<IoError>} = "I/O failure",
    BorrowMutError{source: Rc<BorrowMutError>} = "mutable reference already borrowed",
    Icmpv4Error{source: icmpv4::Error} = "received an ICMPv4 message",
    Misdelivered{} = "misdelivered datagram",
    Unsupported{} = "unsupported",
    Ignored{} = "operation has no effect",
    TryAgain{} = "try again later",
    Timeout{} = "an asynchronous operation timed out",
    OutOfRange{} = "a value is out of range",
    Malformed{} = "received a malformed datagram",
    TypeMismatch{} = "type mismatch",
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
