use custom_error::custom_error;

custom_error! {#[derive(Clone)] pub Fail
    Unknown{code:u8} = "unknown error with code ({code}).",
    DuplicateKey{key:String} = "collection already contains key `{key}`"
}
