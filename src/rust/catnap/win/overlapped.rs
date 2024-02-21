// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    cell::Cell,
    marker::{
        PhantomData,
        PhantomPinned,
    },
    pin::{
        pin,
        Pin,
    },
};

use windows::Win32::{
    Foundation::{
        CloseHandle,
        FALSE,
        HANDLE,
        INVALID_HANDLE_VALUE,
        NTSTATUS,
        WAIT_TIMEOUT,
        WIN32_ERROR,
    },
    Networking::WinSock::SOCKET,
    System::IO::{
        CreateIoCompletionPort,
        GetQueuedCompletionStatusEx,
        OVERLAPPED,
        OVERLAPPED_ENTRY,
    },
};

use crate::{
    catnap::transport::error::translate_ntstatus,
    runtime::{
        condition_variable,
        fail::Fail,
        SharedConditionVariable,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A structure to represent the results of an overlapped I/O operation.
#[derive(Clone, Copy)]
pub struct OverlappedResult {
    /// The completion key for the operation, as set with `IoCompletionPort::associate`.
    pub completion_key: usize,
    /// Status result of the operation.
    pub result: NTSTATUS,
    /// Number of bytes transferred, if applicable.
    pub bytes_transferred: u32,
}

/// Data required by the I/O completion port processor to process I/O completions.
#[repr(C)]
struct OverlappedCompletion {
    /// OVERLAPPED must be first; the implementation casts between the outer structure and this type.
    overlapped: OVERLAPPED,
    /// If set, indicates a coroutine in waiting. Cleared by the I/O processor to signal completion. If unset when an
    /// overlapped is dequeued from the completion port, the completion is abandoned.
    condition_variable: Option<SharedConditionVariable>,
    /// Set by the I/O processor to indicate overlapped result.
    completion_key: usize,
    /// A callback to free all resources associated with the completion for abandoned waits.
    free: unsafe fn(*mut OVERLAPPED) -> (),
    /// Ensure the data stays pinned since the OVERLAPPED must be pinned.
    _marker: PhantomPinned,
}

/// The set of data which must live as long as the I/O operation, including any optional state from the caller.
#[repr(C)]
struct OverlappedCompletionWithState<S> {
    /// Inner data required by the completion processor; must be first to "downcast" to OVERLAPPED.
    inner: OverlappedCompletion,
    /// Caller state, encased in a cell for interior mutability.
    state: S,
}

/// Handle to the OverlappedCompletionWithState
struct OverlappedHandle<S>(Pin<Box<OverlappedCompletionWithState<S>>>);

/// This struct encapsulates the behavior of Windows I/O completion ports. This implementation exposes a single
/// threaded interface for invoking and processing the results of overlapped I/O via idiomatic start-cancel-finish
/// operations. While this type implements `Send`, multiple threads should avoid calling `process_events`, as there may
/// be overhead or OS limitations on the number of such threads. This type cannot be shared among threads.
pub struct IoCompletionPort {
    /// The OS handle to the completion port.
    iocp: HANDLE,

    /// Marker to prevent this type from implementing `Sync`.
    _marker: PhantomData<Cell<()>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl OverlappedResult {
    /// Create an OverlappedResult from the completed OVERLAPPED structure and the completion key returned from the
    /// I/O completion port.
    pub fn new(overlapped: &OVERLAPPED, completion_key: usize) -> OverlappedResult {
        Self {
            completion_key,
            result: NTSTATUS(overlapped.Internal as i32),
            bytes_transferred: overlapped.InternalHigh as u32,
        }
    }

    /// Convert the OverlappedResult into a Result<...> indicating success or failure of the referent operation.
    pub fn ok(&self) -> Result<(), Fail> {
        let win32_error: WIN32_ERROR = translate_ntstatus(self.result);
        if win32_error.is_ok() {
            Ok(())
        } else {
            Err(win32_error.into())
        }
    }
}

impl IoCompletionPort {
    /// Create a new I/O completion port.
    pub fn new() -> Result<IoCompletionPort, Fail> {
        let iocp: HANDLE = match unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1) } {
            Ok(handle) => handle,
            Err(err) => return Err(err.into()),
        };

        if iocp.is_invalid() {
            const MSG: &str = "CreateIoCompletionPort succeeded by returned handle is invalid";
            error!("{}", MSG);
            return Err(Fail::new(libc::EFAULT, MSG));
        }

        Ok(IoCompletionPort {
            iocp,
            _marker: PhantomData,
        })
    }

    /// Associate socket `s` with this I/O completion port. All overlapped I/O operations for `s` which complete on this
    /// completion port will return `completion_key` in the `OverlappedResults` structure.
    #[allow(unused)]
    pub fn associate_socket(&self, s: SOCKET, completion_key: usize) -> Result<(), Fail> {
        self.associate_handle(HANDLE(s.0 as isize), completion_key)
    }

    /// Associate a file handle with this I/O completion port. All overlapped I/O operations for `handle` which complete
    /// on this completion port will return `completion_key` in the `OverlappedResults` structure.
    pub fn associate_handle(&self, handle: HANDLE, completion_key: usize) -> Result<(), Fail> {
        match unsafe { CreateIoCompletionPort(handle, self.iocp, completion_key, 0) } {
            Ok(_) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Perform an asynchronous overlapped I/O operation. This method requires three functions: one to start the I/O
    /// (`start`), one to cancel the I/O on cancellation (`cancel`), and one to finish/clean up and interpret the
    /// results (`finish`). `start` and `cancel` accept an OVERLAPPED pointer for controlling the I/O operation as
    /// well as the state value (initialized by `state`). If `start` fails, this method will abort without calling
    /// `cancel` or `finish`. In this instance, the OVERLAPPED passed to `start` must never be dequeued from the I/O
    /// completion port. If `start` must fail in a way that may result in the OVERLAPPED being dequeued, the method must
    /// return Ok(()) and indicate failure via the `finish` callback (presumably using `state`). `cancel` is invoked iff
    /// the Yielder wakes with an `ECANCELLED` error. When the OVERLAPPED passed to `start` is dequeued from the
    /// completion port, `finish` is invoked with the OverlappedResult type and the state (the same reference passed to
    /// `start`). The return of `finish` is returned from the method.
    ///
    /// Note that the state type S is bound to the `'static` lifetime. The state object is dynamically allocated and may
    /// outlive this function. An example here may be a failed cancellation: a call to `ReadFile` or `WSARecv` may be in
    /// the process of filling the receive buffer when a cancellation occurs (presumably kept alive by S). In this case,
    /// `CancelIo` will fail. Assuming the caller returns before the OVERLAPPED is dequeued, S may live longer than the
    /// calling method.
    ///
    /// Safety: `start` should return Ok(...) iff the I/O is started and the OVERLAPPED parameter will
    /// eventually be dequeued from the completion port; if this requirement is not met, resources will leak. Likewise,
    /// `cancel` should return Ok(...) iff the operation is cancelled and the OVERLAPPED parameter will never be
    /// dequeued from the completion port.  Waking the Yielder for errors which are no ECANCELLED will abandon the wait.
    /// While OVERLAPPED resources will be freed in a non-exceptional scenario, this may cause unsound behavior I/O on
    /// the same file/socket.
    pub async unsafe fn do_io_with<F1, F2, R, S>(&mut self, state: S, start: F1, finish: F2) -> Result<R, Fail>
    where
        S: 'static,
        for<'a> F1: FnOnce(Pin<&'a mut S>, *mut OVERLAPPED) -> Result<(), Fail>,
        for<'a> F2: FnOnce(Pin<&'a mut S>, OverlappedResult) -> Result<R, Fail>,
    {
        let cv: SharedConditionVariable = SharedConditionVariable::default();
        let mut completion: OverlappedHandle<S> =
            OverlappedHandle(OverlappedCompletionWithState::new(cv.clone(), state));

        let overlapped: *mut OVERLAPPED = completion.0.as_mut().marshal();
        match start(completion.0.as_mut().get_state_mut(), overlapped) {
            // Operation in progress, pending overlapped completion.
            Ok(()) => {
                loop {
                    cv.wait().await;

                    if let Some(_) = completion.0.as_ref().get_inner().condition_variable.as_ref() {
                        // Spurious wake-up.
                        continue;
                    } else {
                        let (overlapped, state) = completion.0.as_mut().split_mut();
                        return finish(
                            state,
                            OverlappedResult::new(&overlapped.overlapped, overlapped.completion_key),
                        );
                    }
                }
            },

            // Operation failed to start.
            Err(err) => Err(err),
        }
    }

    /// Same as `do_io_with`, but does not use an intermediate state value.
    pub async unsafe fn do_io<F1, F2, R>(&mut self, start: F1, finish: F2) -> Result<R, Fail>
    where
        F1: FnOnce(*mut OVERLAPPED) -> Result<(), Fail>,
        F2: FnOnce(OverlappedResult) -> Result<R, Fail>,
    {
        self.do_io_with(
            (),
            |_, overlapped: *mut OVERLAPPED| -> Result<(), Fail> { start(overlapped) },
            |_, result: OverlappedResult| -> Result<R, Fail> { finish(result) },
        )
        .await
    }

    /// Process a single overlapped entry.
    #[allow(unused)]
    fn process_overlapped(&mut self, entry: &OVERLAPPED_ENTRY) {
        if let Some(overlapped) = std::ptr::NonNull::new(entry.lpOverlapped) {
            // Safety: this is valid as long as the caller follows the contract: all queued OVERLAPPED instances are
            // generated by `IoCompletionPort` API.
            let overlapped: Pin<&mut OverlappedCompletion> = OverlappedCompletion::unmarshal(overlapped);

            // Safety: the OVERLAPPED does not need to be pinned after being dequeued from the completion port.
            let overlapped: &mut OverlappedCompletion = unsafe { overlapped.get_unchecked_mut() };
            if let Some(mut condition_variable) = overlapped.condition_variable.take() {
                debug_assert!(entry.dwNumberOfBytesTransferred as usize == overlapped.overlapped.InternalHigh);
                overlapped.completion_key = entry.lpCompletionKey;
                condition_variable.signal();
            } else {
                // This can happen due to a failed cancellation or any other error on the do_overlapped path.
                trace!("I/O dropped for completion key {}", entry.lpCompletionKey);
                let free_fn: unsafe fn(*mut OVERLAPPED) = overlapped.free;
                unsafe { (free_fn)(entry.lpOverlapped) };
            }
        }
    }

    /// Process entries by peeking the completion port.
    #[allow(unused)]
    pub fn process_events(&mut self) -> Result<(), Fail> {
        // Arbitrarily chosen batch size; should be updated after tuning.
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

                // WAIT_TIMEOUT is a not an error condition.
                Err(err) if err.code() == WIN32_ERROR(WAIT_TIMEOUT.0).into() => return Ok(()),

                // All other errors are error conditions. Note that GetQueuedCompletionStatusEx can set wait errors as
                // win32 errors.
                Err(err) => return Err(Fail::from_win32_error(&err, true)),
            }
        }
    }
}

impl OverlappedCompletion {
    /// Take the condition variable from this struct. The condition variable is not structurally pinned.
    pub fn take_cv(self: Pin<&mut Self>) -> Option<SharedConditionVariable> {
        // Safety: updating the condition variable does not violate pinning invariants.
        unsafe { self.get_unchecked_mut() }.condition_variable.take()
    }

    /// Check whether the condition variable is still set.
    pub fn has_cv(self: Pin<&Self>) -> bool {
        self.condition_variable.is_some()
    }

    /// Marshal an OVERLAPPED pointer back into an OverlappedCompletion.
    pub fn unmarshal<'a>(overlapped: std::ptr::NonNull<OVERLAPPED>) -> Pin<&'a mut Self> {
        unsafe { Pin::new_unchecked(&mut *(overlapped.as_ptr() as *mut Self)) }
    }
}

impl<S> OverlappedCompletionWithState<S> {
    /// Create a new pinned instance on the heap.
    pub fn new(condition_variable: SharedConditionVariable, state: S) -> Pin<Box<Self>> {
        Box::pin(Self {
            inner: OverlappedCompletion {
                overlapped: OVERLAPPED::default(),
                condition_variable: Some(condition_variable),
                completion_key: 0,
                free: Self::deallocate,
                _marker: PhantomPinned,
            },
            state,
        })
    }

    /// Marshal a OverlappedCompletionWithState into an OVERLAPPED pointer. This type must be pinned for marshaling.
    pub fn marshal(mut self: Pin<&mut Self>) -> *mut OVERLAPPED {
        unsafe { self.as_mut().get_unchecked_mut() as *mut Self }.cast()
    }

    /// Get a pinned reference to the state
    pub fn get_state_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
        // Safety: no values are moved.
        unsafe { self.map_unchecked_mut(|this| &mut this.state) }
    }

    /// Get a pinned mutable reference to the OverlappedCompletion.
    pub fn get_inner_mut(self: Pin<&mut Self>) -> Pin<&mut OverlappedCompletion> {
        // Safety: no values are moved.
        unsafe { self.map_unchecked_mut(|this| &mut this.inner) }
    }

    /// Get a pinned reference to the OverlappedCompletion.
    pub fn get_inner(self: Pin<&Self>) -> Pin<&OverlappedCompletion> {
        // Safety: no values are moved.
        unsafe { self.map_unchecked(|this| &this.inner) }
    }

    /// Split this struct into pinned references to its components.
    pub fn split_mut(self: Pin<&mut Self>) -> (Pin<&mut OverlappedCompletion>, Pin<&mut S>) {
        // Safety: we're not unpinning or moving anything, just splitting references.
        let me: &mut Self = unsafe { self.get_unchecked_mut() };

        // Safety: both pointers are pinned since self is pinned.
        unsafe { (Pin::new_unchecked(&mut me.inner), Pin::new_unchecked(&mut me.state)) }
    }

    /// Called by the I/O event processor to free this instance if the completion is abandoned by the waiter.
    fn deallocate(overlapped: *mut OVERLAPPED) {
        if overlapped != std::ptr::null_mut() {
            let overlapped: *mut Self = overlapped.cast();
            let overlapped: Box<Self> = unsafe { Box::from_raw(overlapped) };
            std::mem::drop(overlapped);
        }
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

impl<S> Drop for OverlappedHandle<S> {
    fn drop(&mut self) {
        if self.0.as_mut().get_inner_mut().take_cv().is_some() {
            // Coroutine exited before the completion was signaled. Leave the heap object alive such the OVERLAPPED
            // structure is still valid until the completion processor sees and deallocates it.
            std::mem::take(unsafe { self.0. });
            let ptr: Pin<Box<OverlappedCompletionWithState<S>>> = self.0;
            std::mem::forget(ptr);
        }
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
        cell::UnsafeCell,
        iter,
        pin::pin,
        ptr::NonNull,
        rc::Rc,
        sync::atomic::{
            AtomicU32,
            Ordering,
        },
        task::{
            Context,
            Poll,
        },
    };

    use crate::{
        ensure_eq,
        runtime::{
            scheduler::{
                scheduler::Scheduler,
                Task,
                TaskId,
                TaskWithResult,
            },
            Operation,
            SharedDemiRuntime,
        },
        OperationResult,
        QDesc,
        QToken,
    };

    use super::*;
    use ::futures::{
        future::FusedFuture,
        FutureExt,
    };
    use anyhow::{
        anyhow,
        bail,
        ensure,
        Result,
    };
    use futures::{
        task::noop_waker_ref,
        Future,
    };
    use windows::{
        core::{
            s,
            HRESULT,
            PCSTR,
        },
        Win32::{
            Foundation::{
                ERROR_IO_PENDING,
                GENERIC_WRITE,
            },
            Storage::FileSystem::{
                CreateFileA,
                ReadFile,
                WriteFile,
                FILE_FLAGS_AND_ATTRIBUTES,
                FILE_FLAG_FIRST_PIPE_INSTANCE,
                FILE_FLAG_OVERLAPPED,
                FILE_SHARE_NONE,
                OPEN_EXISTING,
                PIPE_ACCESS_DUPLEX,
            },
            System::{
                Pipes::{
                    ConnectNamedPipe,
                    CreateNamedPipeA,
                    PIPE_READMODE_MESSAGE,
                    PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_TYPE_MESSAGE,
                },
                IO::{
                    CancelIoEx,
                    PostQueuedCompletionStatus,
                },
            },
        },
    };

    struct SafeHandle(HANDLE);

    // A future wrapper which will poll the wrapped future exactly once.
    struct PollOnceFuture<F: Future<Output = (QDesc, OperationResult)>> {
        future: F,
        count: usize,
    }

    impl<F: Future<Output = (QDesc, OperationResult)>> PollOnceFuture<F> {
        pub fn new(future: F) -> Self {
            Self { future, count: 0 }
        }
    }

    impl<F: Future<Output = (QDesc, OperationResult)>> Future for PollOnceFuture<F> {
        type Output = (QDesc, OperationResult);

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<(QDesc, OperationResult)> {
            if self.count == 0 {
                // Safety: count is not structurally pinned.
                unsafe { self.as_mut().get_unchecked_mut() }.count += 1;

                // Safety: the following does not move any members of self.
                unsafe { self.map_unchecked_mut(|s| &mut s.future) }.poll(cx)
            } else {
                Poll::Ready((
                    QDesc::from(0),
                    OperationResult::Failed(Err::new(libc::EALREADY, "called more than once")),
                ))
            }
        }
    }

    impl Drop for SafeHandle {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    /// Create an I/O completion port, mapping the error return to
    fn make_iocp() -> Result<IoCompletionPort> {
        IoCompletionPort::new().map_err(|err: Fail| anyhow!("Failed to create I/O completion port: {}", err))
    }

    /// Post an overlapped instance to the I/O completion port.
    fn post_completion(iocp: &IoCompletionPort, overlapped: *const OVERLAPPED, completion_key: usize) -> Result<()> {
        unsafe { PostQueuedCompletionStatus(iocp.iocp, 0, completion_key, Some(overlapped)) }
            .map_err(|err| anyhow!("PostQueuedCompletionStatus failed: {}", err))
    }

    // Turn an anyhow Error into a Fail.
    fn anyhow_fail(error: anyhow::Error) -> Fail {
        Fail::new(libc::EFAULT, error.to_string().as_str())
    }

    // Check if an overlapped Result is ok, turning ERROR_IO_PENDING into an Ok(()).
    fn is_overlapped_ok(result: Result<(), windows::core::Error>) -> Result<(), Fail> {
        if let Err(err) = result {
            if err.code() == HRESULT::from(ERROR_IO_PENDING) {
                Ok(())
            } else {
                Err(err.into())
            }
        } else {
            Ok(())
        }
    }

    // An async function which can be marshaled into an Operation and which wraps some other async function which
    // returns Result<...>.
    async fn run_as_io_op<F: Future<Output = Result<OperationResult, Fail>>>(future: F) -> (QDesc, OperationResult) {
        match future.await {
            Ok(result) => (QDesc::from(0), result),
            Err(err) => (QDesc::from(0), OperationResult::Failed(err)),
        }
    }

    #[test]
    fn test_marshal_unmarshal() -> Result<()> {
        let cv: SharedConditionVariable = SharedConditionVariable::default();
        let mut completion: Pin<Box<OverlappedCompletionWithState<()>>> = OverlappedCompletionWithState::new(cv, ());

        // Ensure that the marshal returns the address of the overlapped member.
        ensure_eq!(
            completion.as_mut().marshal() as *const OVERLAPPED as usize,
            &completion.inner.overlapped as *const OVERLAPPED as usize
        );

        // Ensure that marshal returns the address of the completion.
        ensure_eq!(
            completion.as_mut().marshal() as usize,
            completion.as_ref().get_ref() as *const OverlappedCompletionWithState<()> as usize
        );

        // This can be inferred transitively from above. The overlapped member must be at offset 0.
        ensure_eq!(
            (&completion.inner.overlapped as *const OVERLAPPED) as usize,
            (completion.as_ref().get_ref() as *const OverlappedCompletionWithState<()>) as usize
        );

        let overlapped_ptr: NonNull<OVERLAPPED> = NonNull::new(completion.as_mut().marshal()).unwrap();
        let unmarshalled: Pin<&mut OverlappedCompletion> = OverlappedCompletion::unmarshal(overlapped_ptr);

        // Test that unmarshal returns an address which is the same as the OVERLAPPED, which is the same as the original
        // OverlappedCompletionWithState. This implies that OVERLAPPED is at member offset 0 in all structs.
        ensure_eq!(
            unmarshalled.as_ref().get_ref() as *const OverlappedCompletion as usize,
            completion.as_ref().get_ref() as *const OverlappedCompletionWithState<()> as usize
        );

        Ok(())
    }

    /// Test that the event processor can dequeue an overlapped event from the completion port.
    #[test]
    fn test_event_processor() -> Result<()> {
        const COMPLETION_KEY: usize = 123;
        let mut iocp: IoCompletionPort = make_iocp()?;
        let mut cv: SharedConditionVariable = SharedConditionVariable::default();
        let mut overlapped: Pin<Box<OverlappedCompletionWithState<()>>> =
            OverlappedCompletionWithState::new(cv.clone(), ());

        post_completion(&iocp, overlapped.as_mut().marshal(), COMPLETION_KEY)?;

        iocp.process_events()?;

        let unpinned_overlapped: &mut OverlappedCompletionWithState<()> =
            unsafe { overlapped.as_mut().get_unchecked_mut() };

        ensure!(
            unpinned_overlapped.inner.condition_variable.is_none(),
            "yielder should be cleared by iocp"
        );
        ensure_eq!(
            unpinned_overlapped.inner.completion_key,
            COMPLETION_KEY,
            "completion key not updated"
        );

        let mut cx: Context = Context::from_waker(noop_waker_ref());
        let fut: Pin<&mut dyn Future<Output = ()>> = pin!(cv.wait());
        ensure_eq!(fut.poll(&mut cx), Poll::Ready(()), "condition variable not signaled");

        Ok(())
    }

    #[test]
    fn test_overlapped_named_pipe() -> Result<()> {
        const MESSAGE: &str = "Hello world!";
        const PIPE_NAME: PCSTR = s!(r"\\.\pipe\demikernel-test-pipe");
        const BUFFER_SIZE: u32 = 128;
        const COMPLETION_KEY: usize = 0xFEEDF00D;
        let server_pipe: SafeHandle = SafeHandle(unsafe {
            CreateNamedPipeA(
                PIPE_NAME,
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                BUFFER_SIZE,
                BUFFER_SIZE,
                0,
                None,
            )
        }?);
        let server_state: Rc<AtomicU32> = Rc::new(AtomicU32::new(0));
        let server_state_view: Rc<AtomicU32> = server_state.clone();
        let mut iocp: UnsafeCell<IoCompletionPort> = UnsafeCell::new(make_iocp().map_err(anyhow_fail)?);
        iocp.get_mut().associate_handle(server_pipe.0, COMPLETION_KEY)?;
        let iocp_ref: &mut IoCompletionPort = unsafe { &mut *iocp.get() };

        let server: Pin<Box<Operation>> = Box::pin(
            run_as_io_op(async move {
                unsafe {
                    iocp_ref.do_io(
                        |overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                            server_state.fetch_add(1, Ordering::Relaxed);
                            is_overlapped_ok(ConnectNamedPipe(server_pipe.0, Some(overlapped)))
                        },
                        |result: OverlappedResult| -> Result<(), Fail> { result.ok() },
                    )
                }
                .await?;

                server_state.fetch_add(1, Ordering::Relaxed);

                let mut buffer: Rc<Vec<u8>> =
                    Rc::new(iter::repeat(0u8).take(BUFFER_SIZE as usize).collect::<Vec<u8>>());
                buffer = unsafe {
                    iocp_ref.do_io_with(
                        buffer,
                        |state: Pin<&mut Rc<Vec<u8>>>, overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                            let vec: &mut Vec<u8> = Rc::get_mut(state.get_mut()).unwrap();
                            vec.resize(BUFFER_SIZE as usize, 0u8);
                            is_overlapped_ok(ReadFile(
                                server_pipe.0,
                                Some(vec.as_mut_slice()),
                                None,
                                Some(overlapped),
                            ))
                        },
                        |mut state: Pin<&mut Rc<Vec<u8>>>, result: OverlappedResult| -> Result<Rc<Vec<u8>>, Fail> {
                            match result.ok() {
                                Ok(()) => {
                                    if result.bytes_transferred == 0 {
                                        Err(Fail::new(libc::EINVAL, "not bytes received"))
                                    } else {
                                        Rc::get_mut(state.as_mut().get_mut())
                                            .unwrap()
                                            .resize(result.bytes_transferred as usize, 0u8);
                                        Ok(Rc::clone(Pin::get_mut(state)))
                                    }
                                },

                                Err(fail) => Err(fail),
                            }
                        },
                    )
                }
                .await?;

                let message: &str = std::str::from_utf8(buffer.as_slice())
                    .map_err(|_| Fail::new(libc::EINVAL, "utf8 conversion failed"))?;
                if message != MESSAGE {
                    let err_msg: String = format!("expected \"{}\", got \"{}\"", MESSAGE, message);
                    Err(Fail::new(libc::EINVAL, err_msg.as_str()))
                } else {
                    // Dummy result
                    Ok(OperationResult::Close)
                }
            })
            .fuse(),
        );

        let runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        let server_task: QToken = runtime.insert_io_coroutine("server", server).unwrap();

        let mut wait_for_state = |state| -> Result<(), Fail> {
            while server_state_view.load(Ordering::Relaxed) < state {
                iocp.get_mut().process_events()?;
                runtime.poll();
                if let Some((_, result)) = runtime.get_completed_task(&server_task) {
                    return match result {
                        OperationResult::Failed(fail) => Err(fail),
                        _ => Err(Fail::new(libc::EFAULT, "server completed early unexpectedly")),
                    };
                }
            }

            Ok(())
        };

        wait_for_state(1)?;

        let client_handle: SafeHandle = SafeHandle(unsafe {
            CreateFileA(
                PIPE_NAME,
                GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES::default(),
                HANDLE(0),
            )
        }?);

        wait_for_state(2)?;

        let mut bytes_written: u32 = 0;
        unsafe {
            WriteFile(
                client_handle.0,
                Some(MESSAGE.as_bytes()),
                Some(&mut bytes_written),
                None,
            )?;
        }

        std::mem::drop(client_handle);

        let result: OperationResult = loop {
            iocp.get_mut().process_events()?;
            runtime.poll();
            if let Some((_, result)) = runtime.get_completed_task(&server_task) {
                break result;
            }
        };

        ensure_eq!(result, OperationResult::Close, "server did not complete successfully");

        Ok(())
    }

    /// Test I/O cancellation.
    #[test]

    fn test_cancel_io() -> Result<()> {
        const PIPE_NAME: PCSTR = s!(r"\\.\pipe\demikernel-test-cancel-pipe");
        const BUFFER_SIZE: u32 = 128;
        const COMPLETION_KEY: usize = 0xFEEDF00D;
        let server_pipe: SafeHandle = SafeHandle(unsafe {
            CreateNamedPipeA(
                PIPE_NAME,
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                BUFFER_SIZE,
                BUFFER_SIZE,
                0,
                None,
            )
        }?);
        let server_state: Rc<AtomicU32> = Rc::new(AtomicU32::new(0));
        let server_state_view: Rc<AtomicU32> = server_state.clone();
        let mut iocp: UnsafeCell<IoCompletionPort> = UnsafeCell::new(make_iocp().map_err(anyhow_fail)?);
        iocp.get_mut().associate_handle(server_pipe.0, COMPLETION_KEY)?;
        let iocp_ref: &mut IoCompletionPort = unsafe { &mut *iocp.get() };

        let server: Pin<Box<Operation>> = Box::pin(
            PollOnceFuture::new(run_as_io_op(async move {
                unsafe {
                    iocp_ref.do_io(
                        |overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                            server_state.fetch_add(1, Ordering::Relaxed);
                            is_overlapped_ok(ConnectNamedPipe(server_pipe.0, Some(overlapped)))
                        },
                        |result: OverlappedResult| -> Result<(), Fail> { result.ok() },
                    )
                }
                .await?;
                Ok(OperationResult::Close)
            }))
            .fuse(),
        );

        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        let server_task: QToken = runtime.insert_io_coroutine("server", server).unwrap();

        ensure!(
            server_state_view.load(Ordering::Relaxed) < 1,
            "server execution should not start yet"
        );

        iocp_ref.process_events()?;
        runtime.poll();

        ensure!(
            runtime.get_completed_task(&server_task).is_none(),
            "server should not be done"
        );

        // Poll the runtime again, which

        yielder_handle.wake_with(Err(Fail::new(libc::ECANCELED, "I/O cancelled")));

        let task = loop {
            iocp.get_mut().process_events()?;
            if let Some(task) = scheduler.get_next_completed_task(64) {
                if task.get_id() == server_handle {
                    break task;
                }
            }
        };

        let result: Result<(), Fail> = get_server_result(task);
        if let Err(err) = result {
            if err.errno == libc::ECANCELED {
                Ok(())
            } else {
                bail!("coroutine failed with unexpected code: {:?}", err)
            }
        } else {
            bail!("expected coroutine to fail")
        }
    }
}
