use std::pin::{
    pin,
    Pin,
};

use windows::Win32::{
    Foundation::{
        CloseHandle,
        ERROR_INVALID_HANDLE,
        FALSE,
        HANDLE,
        INVALID_HANDLE_VALUE,
        WAIT_ABANDONED,
        WAIT_TIMEOUT,
        WIN32_ERROR,
    },
    System::IO::{
        CreateIoCompletionPort,
        GetQueuedCompletionStatusEx,
        OVERLAPPED,
        OVERLAPPED_ENTRY,
    },
};

use crate::runtime::{
    fail::Fail,
    scheduler::{
        Yielder,
        YielderHandle,
    },
};

#[repr(C)]
struct OverlappedCompletion {
    overlapped: OVERLAPPED,
    completion_key: Option<usize>,
    yielder_handle: Option<YielderHandle>,
}

/// A single-threaded I/O completion port implementation, designed to integrate with rust futures. This class allows
/// creation of futures which can themselves be used as `OVERLAPPED` pointers to Windows overlapped I/O functions.
pub struct IoCompletionPort {
    iocp: HANDLE,
}

impl IoCompletionPort {
    pub fn new() -> Result<IoCompletionPort, Fail> {
        let iocp: HANDLE = match unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1) } {
            Ok(handle) => handle,
            Err(err) => return Err(err.into()),
        };

        // Verified by windows crate.
        assert!(!iocp.is_invalid());

        Ok(IoCompletionPort { iocp })
    }

    /// Associate `file` with this I/O completion port. All overlapped I/O operations which complete on this completion
    /// port will return `completion_key` to the caller.
    pub fn associate(&self, file: HANDLE, completion_key: usize) -> Result<(), Fail> {
        match unsafe { CreateIoCompletionPort(file, self.iocp, completion_key, 0) } {
            Ok(_) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Call a function `f` which will start an overlapped I/O operation with the passed-in OVERLAPPED pointer. If the
    /// callback starts an overlapped I/O operation with the argument, it must return Ok(_). Conversely, if an
    /// overlapped I/O operation is not started, the callback must return Err(_). Failing to meet these criteria
    /// produces unsound behavior. This function will await until the OVERLAPPED is dequeued from this I/O completion
    /// port.
    /// Method 1 polls the completion port inline with the coroutine.
    pub async unsafe fn method1_do_overlapped<F>(&self, yielder: Yielder, f: F) -> Result<usize, Fail>
    where
        F: FnOnce(*mut OVERLAPPED) -> Result<(), Fail>,
    {
        let mut state: Pin<&mut OverlappedCompletion> = pin!(OverlappedCompletion {
            overlapped: OVERLAPPED::default(),
            completion_key: None,
            yielder_handle: None, // Not used for this method.
        });
        let overlapped: *mut OVERLAPPED = state.as_mut().marshal();
        match f(overlapped) {
            Ok(()) => loop {
                if let Some(ck) = state.completion_key {
                    return Ok(ck);
                }
                if let Err(err) = self.process_events() {
                    // If the completion port is shut down, we won't have an issue with the deallocated OVERLAPPED
                    // coming back around.
                    return Err(err);
                }

                if let Some(ck) = state.completion_key {
                    return Ok(ck);
                }

                if let Err(_) = yielder.yield_once().await {
                    panic!("returning here would deallocate the OVERLAPPED.");
                }
            },

            Err(err) => return Err(err),
        }
    }

    /// Call a function `f` which will start an overlapped I/O operation with the passed-in OVERLAPPED pointer. If the
    /// callback starts an overlapped I/O operation with the argument, it must return Ok(_). Conversely, if an
    /// overlapped I/O operation is not started, the callback must return Err(_). Failing to meet these criteria
    /// produces unsound behavior. This function will await until the OVERLAPPED is dequeued from this I/O completion
    /// port.
    ///
    /// Method 2 requires a separate task polling the coroutine.
    pub async unsafe fn method2_do_overlapped<F>(&self, yielder: Yielder, f: F) -> Result<usize, Fail>
    where
        F: FnOnce(*mut OVERLAPPED) -> Result<(), Fail>,
    {
        let mut state: Pin<&mut OverlappedCompletion> = pin!(OverlappedCompletion {
            overlapped: OVERLAPPED::default(),
            completion_key: None,
            yielder_handle: Some(yielder.get_handle()),
        });
        let overlapped: *mut OVERLAPPED = state.as_mut().marshal();
        match f(overlapped) {
            Ok(()) => loop {
                if let Some(ck) = state.completion_key {
                    return Ok(ck);
                }

                if let Err(_) = yielder.yield_once().await {
                    panic!("returning here would deallocate the OVERLAPPED.");
                }
            },

            Err(err) => return Err(err),
        }
    }

    /// Run the even processor coroutine for method 2.
    pub async fn method2_run(&self, yielder: Yielder) -> Result<(), Fail> {
        loop {
            if let Err(err) = self.process_events() {
                return Err(err);
            }

            if let Err(err) = yielder.yield_once().await {
                if err.errno == libc::ECANCELED {
                    return Ok(());
                } else {
                    return Err(err);
                }
            }
        }
    }

    /// Process a single overlapped entry.
    fn process_overlapped<'a>(&'a self, entry: &OVERLAPPED_ENTRY) {
        if let Some(overlapped) = std::ptr::NonNull::new(entry.lpOverlapped) {
            // Safety: this is valid as long as the caller follows the contract: all queued OVERLAPPED instances are
            // generated by `IoCompletionPort` API.
            let mut pinned_fut: Pin<&mut OverlappedCompletion> = unsafe { OverlappedCompletion::unmarshal(overlapped) };
            unsafe { pinned_fut.as_mut().get_unchecked_mut() }.completion_key = Some(entry.lpCompletionKey);
            if let Some(mut yielder_handle) = unsafe { pinned_fut.as_mut().get_unchecked_mut() }.yielder_handle.take() {
                yielder_handle.wake_with(Ok(()));
            }
        }
    }

    /// Process entries by peeking the completion port.
    fn process_events<'a>(&'a self) -> Result<(), Fail> {
        const BATCH_SIZE: usize = 4;
        let mut entries: [OVERLAPPED_ENTRY; BATCH_SIZE] = [OVERLAPPED_ENTRY::default(); BATCH_SIZE];

        loop {
            let mut dequeued: u32 = 0;
            match unsafe { GetQueuedCompletionStatusEx(self.iocp, entries.as_mut_slice(), &mut dequeued, 0, FALSE) } {
                Ok(()) => {
                    for i in 0..dequeued {
                        self.process_overlapped(&entries[i as usize]);
                    }

                    if dequeued < BATCH_SIZE as u32 {
                        return Ok(());
                    }
                },

                Err(err) if err.code() == WIN32_ERROR(WAIT_TIMEOUT.0).into() => return Ok(()),

                Err(err)
                    if err.code() == WIN32_ERROR(WAIT_ABANDONED.0).into()
                        || err.code() == ERROR_INVALID_HANDLE.into() =>
                {
                    return Err(Fail::new(libc::EBADF, "completion port closed"))
                },

                Err(err) => return Err(err.into()),
            }
        }
    }
}

impl OverlappedCompletion {
    /// Marshal an OverlappedCompletion into an OVERLAPPED pointer.
    fn marshal(self: Pin<&mut Self>) -> *mut OVERLAPPED {
        unsafe { self.get_unchecked_mut() as *mut Self }.cast()
    }

    /// Marshal an OVERLAPPED pointer back into an OverlappedCompletion.
    fn unmarshal<'a>(overlapped: std::ptr::NonNull<OVERLAPPED>) -> Pin<&'a mut Self> {
        unsafe { Pin::new_unchecked(&mut *(overlapped.as_ptr() as *mut Self)) }
    }
}

impl Drop for IoCompletionPort {
    /// Close the underlying handle when the completion port is dropped. The underlying primitive will not be freed
    /// until until all `associate`d handles are closed.
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.iocp) };
    }
}

#[cfg(test)]
mod tests {
    use std::{
        pin::pin,
        sync::{
            atomic::{
                AtomicBool,
                Ordering,
            },
            Arc,
        },
        task::Wake,
    };

    use crate::ensure_eq;

    use super::*;
    use anyhow::{
        anyhow,
        bail,
        ensure,
        Result,
    };
    use windows::Win32::System::IO::PostQueuedCompletionStatus;

    struct TestWaker(AtomicBool);

    impl Wake for TestWaker {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    // Create an I/O completion port, mapping the error return to
    fn make_iocp() -> Result<IoCompletionPort> {
        IoCompletionPort::new().map_err(|err| anyhow!("Failed to create I/O completion port: {}", err))
    }

    fn post_completion(iocp: &IoCompletionPort, overlapped: *const OVERLAPPED, completion_key: usize) -> Result<()> {
        unsafe { PostQueuedCompletionStatus(iocp.iocp, 0, completion_key, Some(overlapped)) }
            .map_err(|err| anyhow!("PostQueuedCompletionStatus failed: {}", err))
    }

    // #[test]
    // fn completion_port_open_close() -> Result<()> {
    //     let iocp: IoCompletionPort = make_iocp()?;
    //     std::mem::drop(iocp);
    //     Ok(())
    // }

    // #[test]
    // fn completion_port_poll() -> Result<()> {
    //     const COMPLETION_KEY: usize = 123;
    //     let iocp: IoCompletionPort = make_iocp()?;
    //     let mut future: Pin<&mut OverlappedFuture> = pin!(iocp.make_future());
    //     ensure_eq!(future.as_ref().get_state(), FutureState::NotStarted);

    //     let overlapped: *mut OVERLAPPED = unsafe {
    //         iocp.do_with_overlapped(future.as_ref(), |overlapped| Ok(overlapped))
    //             .unwrap()
    //     };
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(None));

    //     let test_waker: Arc<TestWaker> = Arc::new(TestWaker(AtomicBool::new(false)));
    //     let waker: Waker = test_waker.clone().into();
    //     let mut ctx: Context = Context::from_waker(&waker);

    //     let events: usize = iocp.poll_timeout(Duration::ZERO)?;
    //     ensure_eq!(events, 0);
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(None));

    //     post_completion(&iocp, overlapped, COMPLETION_KEY)?;
    //     let events: usize = iocp.poll()?;
    //     ensure_eq!(events, 1);
    //     ensure_eq!(future.as_ref().get_state(), FutureState::Completed(COMPLETION_KEY));

    //     match future.as_ref().as_future().poll(&mut ctx) {
    //         Poll::Ready(Ok(completion_key)) => ensure_eq!(completion_key, COMPLETION_KEY),
    //         _ => bail!("future should be ready"),
    //     }

    //     Ok(())
    // }

    // #[test]
    // fn test_completion_wake() -> Result<()> {
    //     const COMPLETION_KEY: usize = 123;
    //     let iocp: IoCompletionPort = make_iocp()?;
    //     let future: Pin<&mut OverlappedFuture> = pin!(iocp.make_future());
    //     ensure_eq!(future.as_ref().get_state(), FutureState::NotStarted);

    //     let overlapped: *mut OVERLAPPED = unsafe {
    //         iocp.do_with_overlapped(future.as_ref(), |overlapped| Ok(overlapped))
    //             .unwrap()
    //     };
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(None));

    //     let dyn_future: Pin<&mut dyn Future<Output = Result<usize, Fail>>> = future.as_ref().as_future();
    //     let mut task: Pin<Box<dyn Future<Output = Result<usize, Fail>>>> = Box::pin(async { dyn_future.await });
    //     let test_waker: Arc<TestWaker> = Arc::new(TestWaker(AtomicBool::new(false)));
    //     let waker: Waker = test_waker.clone().into();
    //     let mut ctx: Context = Context::from_waker(&waker);
    //     ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(None));

    //     if let Poll::Ready(_) = task.as_mut().poll(&mut ctx) {
    //         bail!("future should not be ready");
    //     };
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(Some(&waker)));
    //     ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

    //     let events: usize = iocp.poll_timeout(Duration::ZERO)?;
    //     ensure_eq!(events, 0);
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(Some(&waker)));
    //     ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

    //     post_completion(&iocp, overlapped, COMPLETION_KEY)?;
    //     ensure_eq!(future.as_ref().get_state(), FutureState::InProgress(Some(&waker)));
    //     ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

    //     match task.as_mut().poll(&mut ctx) {
    //         Poll::Ready(Ok(completion_key)) => ensure_eq!(completion_key, COMPLETION_KEY),
    //         _ => bail!("task should be ready"),
    //     }

    //     // NB waker never gets called as an optimization, since the task polling the completion port would be the woken
    //     // task.
    //     ensure_eq!(future.as_ref().get_state(), FutureState::Completed(COMPLETION_KEY));
    //     ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

    //     // Ensure completion port is empty.
    //     let events: usize = iocp.poll_timeout(Duration::ZERO)?;
    //     ensure_eq!(events, 0);

    //     Ok(())
    // }
}
