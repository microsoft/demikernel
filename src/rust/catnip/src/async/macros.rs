#[macro_export]
macro_rules! try_poll {
    ($async:expr, $now:expr) => {{
        match $async.poll($now) {
            None => (),
            Some(Ok(_)) => (),
            Some(Err(e)) => return Some(Err(e)),
        };
    }};
}

#[macro_export]
macro_rules! yield_until {
    ($cond:expr, $now:expr) => {{
        while !$cond {
            yield None;
        }

        true
    }};
    ($cond:expr, $now:expr, $timeout:expr) => {{
        let mut success = false;
        let t0 = $now;
        loop {
            if $cond {
                success = true;
                break;
            }

            yield None;

            let dt = $now - t0;
            if dt >= $timeout {
                break;
            }
        }

        success
    }};
}

#[macro_export]
macro_rules! r#await {
    ($async:expr, $now:expr) => {{
        use $crate::r#async::Async;
        let r#async = $async;
        assert!(yield_until!(r#async.poll($now).is_some(), $now));
        r#async.poll($now).unwrap()
    }};
    ($async:expr, $now:expr, $timeout:expr) => {{
        use $crate::r#async::Async;
        let r#async = $async;
        if yield_until!(r#async.poll($now), $now, $timeout) {
            r#async.poll($now).unwrap()
        } else {
            use $crate::fail::Fail;
            Err(Fail::Timeout {})
        }
    }};
}
