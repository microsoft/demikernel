mod etherparse_error;

use custom_error::custom_error;
use std::io;

pub use etherparse_error::EtherParseError;

custom_error! {pub Fail
    IoError{source: io::Error} = "I/O failure",
    EtherParseError{source: EtherParseError} = "error reported by etherparse crate",
    Misdelivered{} = "misdelivered packet",
    Unsupported{} = "unsupported",
    Ignored{} = "operation has no effect",
}
