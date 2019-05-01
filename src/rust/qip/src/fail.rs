use custom_error::custom_error;

custom_error!{pub Fail
    Unknown{code:u8} = "unknown error with code ({code})."
}
