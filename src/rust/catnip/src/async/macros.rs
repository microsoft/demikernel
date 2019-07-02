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
macro_rules! await_yield {
    ($async:expr, $now:expr) => {{
        assert!(yield_until!($async.poll($now).is_some(), $now));
        $async.poll($now).unwrap()
    }};
    ($async:expr, $now:expr, $timeout:expr) => {{
        if yield_until!($async.poll($now), $now, $timeout) {
            $async.poll($now).unwrap()
        } else {
            None
        }
    }};
}

