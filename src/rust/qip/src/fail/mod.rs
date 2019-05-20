mod etherparse_error;

use custom_error::custom_error;

pub use etherparse_error::EtherParseError;

custom_error! {pub Fail
    // todo: using `MacAddress` instead of `String` here doesn't work because `MacAddress::to_string()` does not conform to the `ToString` trait.
    EtherParseError{source: EtherParseError} = "failure to encode or decode a packet",
    Misdelivered{dest: String} = "misdelivered packet (intended for {dest})",
    UnrecognizedFieldValue{ name: String, value: usize } = "unrecognized value ({value}) in field `{name}`.",
}
