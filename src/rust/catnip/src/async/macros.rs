#[macro_export]
macro_rules! try_poll {
    ($async:expr, $now:expr) => {
        {
            match $async.poll($now()) {
                None => (),
                Some(Ok(_)) => (),
                Some(Err(e)) => return Some(Err(e)),
            };
        }
    };
}
