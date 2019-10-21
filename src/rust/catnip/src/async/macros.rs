// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
        let timeout = $timeout;
        trace!("yield_until!(..., {:?})", timeout);
        let mut success = false;
        let t0 = $now;
        debug!("yield_until!(): will timeout at {:?}", t0 + timeout);
        loop {
            if $cond {
                success = true;
                break;
            }

            yield None;

            let dt = $now - t0;
            let timeout = $timeout;
            if dt > timeout {
                debug!("yield_until!(): *timeout*");
                break;
            } else {
                debug!("yield_until!(): dt = {:?}", dt);
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
    ($async:expr, $now:expr, $retry:expr) => {{
        trace!("r#await!()");
        use $crate::{fail::Fail, r#async::Async};
        let retry = $retry;
        let mut result = None;
        for timeout in retry {
            let r#async = $async;
            if yield_until!(r#async.poll($now).is_some(), $now, timeout) {
                result = r#async.poll($now);
                assert!(result.is_some());
                break;
            }
        }

        result.unwrap_or(Err(Fail::Timeout {}))
    }};
}
