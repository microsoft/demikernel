// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use custom_error::custom_error;
use std::{
    cell::BorrowMutError, error::Error, io::Error as IoError,
    num::TryFromIntError, rc::Rc,
};

// the following type alias is needed because the `custom_error!` macro doesn't
// allow `&` in type specifications.
type Str = &'static str;

custom_error! {#[derive(Clone)] pub Fail
    ConnectionAborted{} = "connection aborted",
    ConnectionRefused{} = "connection refused",
    ForeignError{source: Rc<dyn Error>} = "{source}",
    Ignored{details: Str} = "operation had no effect ({details})",
    Malformed{details: Str} = "encountered a malformed datagram ({details})",
    Misdelivered{} = "misdelivered datagram",
    OutOfRange{details: Str} = "a value is out of range ({details})",
    ResourceBusy{details: Str} = "resource is busy ({details})",
    ResourceExhausted{details: Str} = "resource exhausted ({details})",
    ResourceNotFound{details: Str} = "resource not found ({details})",
    Timeout{} = "an asynchronous operation timed out",
    TypeMismatch{details: Str} = "type mismatch ({details})",
    Unsupported{details: Str} = "unsupported ({details})",
}

impl From<IoError> for Fail {
    fn from(e: IoError) -> Self {
        Fail::ForeignError { source: Rc::new(e) }
    }
}

impl From<BorrowMutError> for Fail {
    fn from(e: BorrowMutError) -> Self {
        Fail::ForeignError { source: Rc::new(e) }
    }
}

impl From<float_duration::error::OutOfRangeError> for Fail {
    fn from(_: float_duration::error::OutOfRangeError) -> Self {
        Fail::OutOfRange {
            details: "float_duration::error::OutOfRangeError",
        }
    }
}

impl From<TryFromIntError> for Fail {
    fn from(_: TryFromIntError) -> Self {
        Fail::OutOfRange {
            details: "std::num::TryFromIntError",
        }
    }
}
