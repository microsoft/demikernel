use std::{
    marker::PhantomPinned,
    mem::ManuallyDrop,
    ops::DerefMut,
    pin::Pin,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::Duration,
};

use futures::Future;
use windows::Win32::{
    Foundation::{
        CloseHandle,
        ERROR_INVALID_HANDLE,
        ERROR_SUCCESS,
        FALSE,
        HANDLE,
        INVALID_HANDLE_VALUE,
        STATUS_PENDING,
        WAIT_ABANDONED,
        WAIT_TIMEOUT,
        WIN32_ERROR,
    },
    System::{
        Threading::INFINITE,
        IO::{
            CreateIoCompletionPort,
            GetQueuedCompletionStatusEx,
            OVERLAPPED,
            OVERLAPPED_ENTRY,
        },
    },
};

use crate::runtime::fail::Fail;

/// Pack the waker and completion key into a union, since their presence is mutually exclusive.
#[repr(C)]
union WakeState {
    waker: ManuallyDrop<Option<Waker>>,
    completion: (usize, bool),
}

/// Possible states for an OverlappedFuture.
pub enum FutureState {
    NotStarted,
    InProgress(Optional<&Waker>),
    Completed(usize),
}

/// OverlappedFuture is a `Future` which coordinates with an I/O completion port to signal completion. This is a single
/// use type: after this has been used for a completion operation once, it is unsound to use it again. Sending an
/// instance of this type to multiple overlapped I/O calls will result in at most one notification.
#[repr(C)]
pub struct OverlappedFuture<'a> {
    /// The OVERLAPPED object required by windows overlapped I/O
    overlapped: OVERLAPPED,
    /// When ready to be used as an OVERLAPPED, this will hold an `Option<Waker>` in waker field. Once completed, this
    /// will hold the completion key. The state of this union is determined by the `iocp` field; Some(_) indicates
    /// waker, while None indicates completion_key.
    wake_state: WakeState,
    /// When ready to be used as a completion, this refers back to the completion port for which this was created.
    iocp: Option<&'a IoCompletionPort>,
    /// This type must be pinned for Windows to find the OVERLAPPED structure.
    _marker: PhantomPinned,
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

    /// Poll for events, blocking until at least one event is posted to the completion port. If more than one event is
    /// available, the completion port will be drained until it would otherwise block. Returns the number of events
    /// processed.
    pub fn poll(&self) -> Result<usize, Fail> {
        self.pub_poll(INFINITE)
    }

    /// Same as `poll`, but will timeout after `timeout` has elapsed, returning even if no events have been processed.
    pub fn poll_timeout(&self, timeout: Duration) -> Result<usize, Fail> {
        let timeout_ms: u32 =
            u32::try_from(timeout.as_millis()).map_err(|_| Fail::new(libc::ERANGE, "timeout too large"))?;
        self.pub_poll(timeout_ms)
    }

    /// Implementation of `poll` and `poll_timeout`.
    fn pub_poll(&self, timeout: u32) -> Result<usize, Fail> {
        match self.do_poll(None, timeout) {
            Ok((Poll::Ready(_), _)) => {
                panic!("invariant violation: poll should not return a completion key without an invoker")
            },
            Ok((Poll::Pending, events)) => Ok(events),
            Err(err) => Err(err),
        }
    }

    pub fn associate(&self, file: HANDLE, completion_key: usize) -> Result<(), Fail> {
        match unsafe { CreateIoCompletionPort(file, self.iocp, completion_key, 0) } {
            Ok(_) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn make_future<'a>(&'a self) -> OverlappedFuture<'a> {
        OverlappedFuture::new()
    }

    /// Call a function `f` which will start an overlapped I/O operation with the passed-in OVERLAPPED pointer. After
    /// a successful call to this method and before the OverlappedFuture is dropped, one of the following conditions
    /// must be met:
    /// 1. the OVERLAPPED returned from this call is dequeued from this I/O completion port; or
    /// 2. the I/O completion port is closed.
    /// If neither of these conditions is met, the program is unsound and the drop call for the OverlappedFuture will
    /// panic. This is enforced because deallocating an OVERLAPPED structure in use for an overlapped I/O operation
    /// causes undefined behavior. If the callback fails with an Err(_), it should not start any overlapped I/O
    /// operations. Any overlapped I/O started with this pointer
    pub unsafe fn do_with_overlapped<'a, F, S>(
        &'a self,
        future: Pin<&mut OverlappedFuture<'a>>,
        f: F,
    ) -> Result<S, Fail>
    where
        F: FnOnce(*mut OVERLAPPED) -> Result<S, Fail>,
    {
        let overlapped: *mut OVERLAPPED = unsafe { future.try_ready_overlapped(self) }?;
        f(overlapped).or_else(|err| {
            unsafe { future.unready_overlapped() };
            Err(err)
        })
    }

    /// Signal the specified future with the given completion key.
    pub fn signal_future<'a>(
        &'a self,
        future: Pin<&mut OverlappedFuture<'a>>,
        completion_key: usize,
    ) -> Result<(), Fail> {
        if future.is_in_progress() {
            future.complete_and_wake(completion_key);
        }
        Ok(())
    }

    fn process_overlapped<'a>(
        &'a self,
        invoker: Option<&mut Pin<&mut OverlappedFuture<'a>>>,
        entry: &OVERLAPPED_ENTRY,
    ) -> Poll<()> {
        if let Some(fut) = unsafe { entry.lpOverlapped.cast::<OverlappedFuture>().as_mut() } {
            // Pin the overlapped.
            let pinned_fut: Pin<&mut OverlappedFuture> = unsafe { Pin::new_unchecked(fut) };

            // Optimization: for the invoker, don't bother waking the waker. Set the completion key and return a value
            // indicating that the calling future may proceed.
            if let Some(invoker) = invoker {
                if pinned_fut.as_ref().is_same(invoker.as_ref()) {
                    invoker.as_mut().signal_completion(entry.lpCompletionKey);
                    return Poll::Ready(());
                }
            }

            pinned_fut.complete_and_wake(entry.lpCompletionKey);
        }

        Poll::Pending
    }

    /// Poll, either from an external caller or from a polled `OverlappedFuture`. When called from an OverlappedFuture,
    /// `invoker` will hold a reference to that future. If `invoker` is completed on this poll, the Waker will not be
    /// notified; instead, the first element of the return tuple will be `Poll::Ready`. In all other cases, this value
    /// will be `Poll::Pending`. The second element of the return tuple is the number of events dequed from the
    /// completion port.
    fn do_poll<'a>(
        &'a self,
        mut invoker: Option<Pin<&mut OverlappedFuture<'a>>>,
        mut timeout: u32,
    ) -> Result<(Poll<()>, usize), Fail> {
        const BATCH_SIZE: usize = 4;
        let mut entries: [OVERLAPPED_ENTRY; BATCH_SIZE] = [OVERLAPPED_ENTRY::default(); BATCH_SIZE];
        let mut dequeued: u32 = 0;
        let mut total_dequeued: usize = 0;
        let mut status: Poll<()> = Poll::Pending;

        loop {
            match unsafe {
                GetQueuedCompletionStatusEx(self.iocp, entries.as_mut_slice(), &mut dequeued, timeout, FALSE)
            } {
                Ok(()) => {
                    for i in 0..dequeued {
                        if let Poll::Ready(ck) = self.process_overlapped(invoker.as_mut(), &entries[i as usize]) {
                            status = Poll::Ready(ck);
                        }
                    }
                    total_dequeued += dequeued as usize;

                    if dequeued < BATCH_SIZE as u32 {
                        return Ok((status, total_dequeued));
                    } else {
                        // NB next loop will only peek the completion port.
                        timeout = 0;
                    }
                },

                Err(err) if err.code() == WIN32_ERROR(WAIT_TIMEOUT.0).into() => return Ok((status, total_dequeued)),

                // TODO: Completion port closed. Cancel everything?
                Err(err)
                    if err.code() == WIN32_ERROR(WAIT_ABANDONED.0).into()
                        || err.code() == ERROR_INVALID_HANDLE.into() =>
                {
                    return Err(Fail::new(libc::EFAULT, "completion port closed"))
                },

                Err(err) => return Err(err.into()),
            }
        }
    }
}

impl<'a> OverlappedFuture<'a> {
    fn new() -> OverlappedFuture<'a> {
        OverlappedFuture {
            overlapped: OVERLAPPED::default(),
            wake_state: WakeState { completion: (0, false) },
            iocp: None,
            _marker: PhantomPinned,
        }
    }

    /// Read the Internal field of the `OVERLAPPED` structure.
    pub fn field_internal(self: Pin<&Self>) -> usize {
        self.overlapped.Internal
    }

    /// Read the InternalHigh field of the `OVERLAPPED` structure.
    pub fn field_internal_high(self: Pin<&Self>) -> usize {
        self.overlapped.InternalHigh
    }

    /// Read the Offset field of the `OVERLAPPED` structure.
    pub fn field_offset(self: Pin<&Self>) -> u32 {
        unsafe { self.overlapped.Anonymous.Anonymous.Offset }
    }

    /// Read the OffsetHigh field of the `OVERLAPPED` structure.
    pub fn field_offset_high(self: Pin<&Self>) -> u32 {
        unsafe { self.overlapped.Anonymous.Anonymous.OffsetHigh }
    }

    /// Determine whether the instances refer to the same memory location. This is valid as long as the type is pinned.
    fn is_same(self: Pin<&Self>, other: Pin<&Self>) -> bool {
        std::ptr::eq(self.get_ref(), other.get_ref())
    }

    /// Expose the underlying OVERLAPPED structure. This method is unsafe because it relies on correct usage of
    /// overlapped I/O: all OVERLAPPED structures using the associated completion port must be `OverlappedFuture`
    /// created by that completion port, or the behavior is not defined.
    unsafe fn try_ready_overlapped(self: Pin<&mut Self>, iocp: &'a IoCompletionPort) -> Result<*mut OVERLAPPED, Fail> {
        // If the iocp is set, another operation is in progress.
        if self.iocp.is_some() || self.wake_state.completion.1 {
            return Err(Fail::new(libc::EINVAL, "invalid usage of OverlappedFuture"));
        }

        // Safety: as long as this instance is not used in an overlapped I/O operation, it does not need to be pinned.
        let mut_self: &mut Self = unsafe { self.get_unchecked_mut() };
        mut_self.iocp = Some(iocp);
        mut_self.wake_state.waker = ManuallyDrop::new(None);
        Ok(&mut mut_self.overlapped)
    }

    /// Unrolls changes made by try_ready_overlapped. This method must only be called if the overlapped is not used in
    /// an overlapped I/O operation.
    unsafe fn unready_overlapped(self: Pin<&mut Self>) {
        // Safety: as long as this instance is not used in an overlapped I/O operation, it does not need to be pinned.
        let mut_self: &mut Self = unsafe { self.get_unchecked_mut() };
        if self.iocp.is_some() {
            mut_self.iocp = None;
            unsafe {
                ManuallyDrop::drop(&mut mut_self.wake_state.waker);
            }
        }

        // Prevent the object from being reused.
        mut_self.wake_state.completion.0 = 0;
        mut_self.wake_state.completion.1 = true;
    }

    pub fn get_state(self: Pin<&Self>) -> FutureState {
        if self.iocp.is_some() {
            FutureState::InProgress(unsafe { self.wake_state.waker.as_ref() })
        } else {
            if self.wake_state.completion.1 {
                FutureState::Completed(self.wake_state.completion.0)
            } else {
                FutureState::NotStarted
            }
        }
    }

    /// Signal that the OVERLAPPED event completed, swapping the `wake_state::waker` for the
    /// `wake_state::completion_key`.
    fn signal_completion(self: Pin<&mut Self>, completion_key: usize) -> Option<Waker> {
        if let FutureState::InProgress(_) = self.as_ref().get_state() {
            // Safety: once completed, self can be unpinned, as it will not be used by a completion port.
            let mut_self: &mut Self = unsafe { self.get_unchecked_mut() };

            // Swap the waker out for the completion key.
            let waker_option: Option<Waker> = unsafe { ManuallyDrop::take(&mut mut_self.wake_state.waker) };
            mut_self.wake_state.completion.0 = completion_key;
            mut_self.wake_state.completion.1 = true;
            mut_self.iocp = None;
            waker_option
        } else {
            debug_assert!(false);
            None
        }
    }

    /// Complete the future and wake the depending Future.
    fn complete_and_wake(mut self: Pin<&mut Self>, completion_key: usize) {
        // Swap the waker out for the completion key.
        if let Some(waker) = self.as_mut().signal_completion(completion_key) {
            waker.wake();
        }
    }

    fn update_waker(self: Pin<&mut Self>, waker: &Waker) {
        // iocp must be set to use the waker field.
        debug_assert!(self.iocp.is_some());
        let waker_dest: &mut Option<Waker> = unsafe { self.get_unchecked_mut().wake_state.waker.deref_mut() };
        *waker_dest = Some(waker.clone());
    }

    fn get_completion_key(self: Pin<&Self>) -> usize {
        // iocp must be cleared to have a completion key.
        debug_assert!(self.iocp.is_none() && unsafe { self.wake_state.completion.1 });
        unsafe { self.wake_state.completion.0 }
    }
}

impl Drop for IoCompletionPort {
    /// Close the underlying handle when the completion port is dropped. The underlying primitive will not be freed
    /// until until all `associate`d handles are closed.
    fn drop(&mut self) {
        unsafe { CloseHandle(self.iocp) };
    }
}

impl<'a> Drop for OverlappedFuture<'a> {
    fn drop(&mut self) {
        if self.iocp.is_some() {
            panic!("cannot drop an OverlappedFuture while an OVERLAPPED operation might still be pending");
        }
    }
}

impl<'a> Future for OverlappedFuture<'a> {
    type Output = usize;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<usize, Fail>> {
        if self.iocp.is_some() {
            match self.iocp.unwrap().do_poll(Some(self.as_mut()), 0) {
                // This future not yet ready: set the waker so we will wake on some future poll.
                Ok((Poll::Pending, _)) => {
                    self.as_mut().update_waker(cx.waker());
                    Poll::Pending
                },

                // Ready signals that this future has completed by the call to do_poll in this method. This transitions
                /// self.iocp to None and replaces the wake_state with the completion key.
                Ok((Poll::Ready(()), _)) => Poll::Ready(Ok(self.as_ref().get_completion_key())),

                // Polling failed. Update the state and propagate the error.
                Err(err) => {
                    // Double-check the iocp state and clear the state so drop doesn't panic.
                    if self.iocp.is_some() {
                        self.as_mut().signal_completion(0);
                    }
                    Poll::Ready(Err(err))
                },
            }
        } else {
            Poll::Ready(Ok(self.as_ref().get_completion_key()))
        }
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

    #[test]
    fn completion_port_open_close() -> Result<()> {
        let iocp: IoCompletionPort = make_iocp()?;
        std::mem::drop(iocp);
        Ok(())
    }

    #[test]
    fn completion_port_poll() -> Result<()> {
        const COMPLETION_KEY: usize = 123;
        let iocp: IoCompletionPort = make_iocp()?;
        let mut future: Pin<&mut OverlappedFuture> = pin!(iocp.make_future());
        let overlapped: *mut OVERLAPPED = iocp.get_overlapped(future.as_mut());
        let test_waker: Arc<TestWaker> = Arc::new(TestWaker(AtomicBool::new(false)));
        let waker: Waker = test_waker.clone().into();
        let mut ctx: Context = Context::from_waker(&waker);

        let events: usize = iocp.poll_timeout(Duration::ZERO)?;
        ensure_eq!(events, 0);

        post_completion(&iocp, overlapped, COMPLETION_KEY)?;
        let events: usize = iocp.poll()?;
        ensure_eq!(events, 1);

        ensure!(future.iocp.is_none());
        ensure_eq!(future.wake_state.completion_key, COMPLETION_KEY);

        match future.poll(&mut ctx) {
            Poll::Pending => bail!("future should be ready"),
            Poll::Ready(completion_key) => ensure_eq!(completion_key, COMPLETION_KEY),
        }

        Ok(())
    }

    #[test]
    fn test_completion_wake() -> Result<()> {
        const COMPLETION_KEY: usize = 123;
        let iocp: IoCompletionPort = make_iocp()?;
        let mut future: Pin<&mut OverlappedFuture> = pin!(iocp.make_future());
        let overlapped: *mut OVERLAPPED = iocp.get_overlapped(future.as_mut());
        let mut task: Pin<Box<dyn Future<Output = usize>>> = Box::pin(async { future.as_mut().await });
        let test_waker: Arc<TestWaker> = Arc::new(TestWaker(AtomicBool::new(false)));
        let waker: Waker = test_waker.clone().into();
        let mut ctx: Context = Context::from_waker(&waker);
        ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

        if let Poll::Ready(_) = task.as_mut().poll(&mut ctx) {
            bail!("future should not be ready");
        };
        ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

        let events: usize = iocp.poll_timeout(Duration::ZERO)?;
        ensure_eq!(events, 0);
        ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

        post_completion(&iocp, overlapped, COMPLETION_KEY)?;
        ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

        match task.as_mut().poll(&mut ctx) {
            Poll::Pending => bail!("task should be ready"),
            Poll::Ready(completion_key) => ensure_eq!(completion_key, COMPLETION_KEY),
        }

        // NB waker never gets called as an optimization, since the task polling the completion port would be the woken
        // task.
        ensure_eq!(test_waker.0.load(Ordering::Relaxed), false);

        // Ensure completion port is empty.
        let events: usize = iocp.poll_timeout(Duration::ZERO)?;
        ensure_eq!(events, 0);

        Ok(())
    }
}
