use std::{
    mem::ManuallyDrop,
    ops::DerefMut,
    pin::Pin,
    task::{
        Context,
        Poll,
        Waker,
    },
};

use futures::Future;
use windows::Win32::{
    Foundation::{
        ERROR_SUCCESS,
        FALSE,
        HANDLE,
        INVALID_HANDLE_VALUE,
        STATUS_PENDING,
        WAIT_ABANDONED,
        WAIT_TIMEOUT,
    },
    System::IO::{
        CreateIoCompletionPort,
        GetQueuedCompletionStatusEx,
        OVERLAPPED,
        OVERLAPPED_ENTRY,
    },
};

use crate::runtime::fail::Fail;

union WakeState {
    waker: ManuallyDrop<Option<Waker>>,
    completion_key: u32,
}

pub struct OverlappedFuture<'a> {
    overlapped: OVERLAPPED,
    wake_state: WakeState,
    iocp: &'a IoCompletionPort,
}

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

    pub fn associate(&self, file: HANDLE, completion_key: u32) -> Result<(), Fail> {
        unsafe { CreateIoCompletionPort(file, self.iocp, completion_key, 0) }.map_err(Fail::from)
    }

    pub fn make_future<'a>(&'a self) -> OverlappedFuture<'a> {
        OverlappedFuture::new(self)
    }

    fn process_overlapped<'a>(&'a self, invoker: Pin<&OverlappedFuture<'a>>, entry: &OVERLAPPED_ENTRY) -> Poll<()> {
        if let Some(fut) = unsafe { entry.lpOverlapped.cast::<OverlappedFuture>().as_mut() } {
            let pinned_fut: Pin<&mut OverlappedFuture> = unsafe { Pin::new_unchecked(fut) };

            if pinned_fut.as_ptr() == invoker.as_ptr() {
                return Poll::Ready(());
            }

            pinned_fut.complete(entry.lpCompletionKey);
        }

        Poll::Pending
    }

    fn poll<'a, 'b>(&'a self, future: Pin<&'b OverlappedFuture<'a>>) -> Poll<()> {
        const BATCH_SIZE: usize = 4;
        let mut entries: [OVERLAPPED_ENTRY; BATCH_SIZE] = [OVERLAPPED_ENTRY::default(); BATCH_SIZE];
        let mut dequeued: u32 = 0;

        loop {
            unsafe {
                match GetQueuedCompletionStatusEx(self.iocp, entries.as_mut_slice(), &mut dequeued, 0, FALSE) {
                    Ok(()) => {
                        let mut status: Poll<()> = Poll::Pending;
                        for i in 0..dequed {
                            if let Poll::Ready(_) = self.process_overlapped(future, &entires[i as usize]) {
                                status = Poll::Ready(());
                            }
                        }

                        if status == Poll::Ready(()) {
                            return status;
                        }
                    },

                    Err(err) if err.code() == WAIT_TIMEOUT.into() => {
                        return Poll::Pending;
                    },

                    Err(err) if err.code() == WAIT_ABANDONED.into || err.code() == INVALID_HANDLE_VALUE => {
                        // TODO: Completion port closed. Cancel everything?
                        panic!("completion port closed");
                    },

                    Err(err) => {
                        // TODO: error reporting, retryable errors.
                        panic!("{}", err);
                    },
                }
            }
        }
    }
}

impl<'a> OverlappedFuture<'a> {
    fn new(iocp: &'a IoCompletionPort) -> OverlappedFuture {
        OverlappedFuture {
            overlapped: OVERLAPPED::default(),
            wake_state: WakeState { waker: None },
            iocp,
        }
    }

    pub fn as_ptr(self: Pin<&Self>) -> *const OVERLAPPED {
        &self.overlapped
    }

    pub unsafe fn as_mut_ptr(self: Pin<&mut Self>) -> *mut OVERLAPPED {
        &mut unsafe { self.get_unchecked_mut() }.overlapped
    }

    fn complete(self: Pin<&mut Self>, completion_key: usize) {
        if self.overlapped.Internal == STATUS_PENDING.0 as usize {
            // TODO: this may not be correct; determine more sensible behavior here.
            unsafe { self.get_unchecked_mut() }.overlapped.Internal = ERROR_SUCCESS.0 as usize;
        }

        let wake_state: &mut WakeState = &mut unsafe { self.get_unchecked_mut() }.wake_state;

        // Swap the waker out for the completion key.
        if let Some(waker) = wake_state.waker.take().take() {
            wake_state.completion_key = completion_key;
            waker.wake();
        }
    }
}

impl !Unpin for OverlappedFuture {}

impl Future for OverlappedFuture {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<usize> {
        if self.overlapped.Internal == STATUS_PENDING.0 as usize {
            unsafe { self.get_unchecked_mut() }.wake_state.waker = ManuallyDrop::new(None);
            if self.iocp.poll(self.as_ref()) == Poll::Pending {
                (*unsafe { self.get_unchecked_mut() }.wake_state.waker) = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }

        Poll::Ready(self.wake_state.completion_key)
    }
}
