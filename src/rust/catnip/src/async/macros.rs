#[macro_export]
macro_rules! try_poll {
    ($async:expr, $now:expr) => {{
        match $async.poll($now()) {
            None => (),
            Some(Ok(_)) => (),
            Some(Err(e)) => return Some(Err(e)),
        };
    }};
}

#[macro_export]
macro_rules! await_yield {
    ($async:expr, $now:expr) => {{
        let x;
        loop {
            if let Some(result) = $async.poll($now()) {
                match result {
                    Ok(y) => {
                        x = y;
                        break;
                    }
                    Err(e) => return Err(e),
                }
            } else {
                yield None;
                continue;
            }
        }

        x
    }};
}
