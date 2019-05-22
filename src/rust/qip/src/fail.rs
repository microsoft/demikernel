use custom_error::custom_error;
use std::io;

custom_error! {pub Fail
    IoError{source: io::Error} = "I/O failure",
    Misdelivered{} = "misdelivered packet",
    Unsupported{} = "unsupported",
    Ignored{} = "operation has no effect",
}
