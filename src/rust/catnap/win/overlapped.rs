// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    cell::{
        Cell,
        UnsafeCell,
    },
    marker::{
        PhantomData,
        PhantomPinned,
    },
    pin::Pin,
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
        fail::Fail,
        scheduler::{
            Yielder,
            YielderHandle,
        },
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
    yielder_handle: Option<YielderHandle>,
    /// Set by the I/O processor to indicate overlapped result.
    completion_key: usize,
    /// A callback to free all resources associated with the completion for abandoned waits.
    free: unsafe fn(*mut OVERLAPPED) -> (),
    /// Ensure the data stays pinned since the OVERLAPPED must be pinned.
    _marker: PhantomPinned,
}

/// The set of data which must live as long as the I/O operation, including any optional state from the caller.
#[repr(C)]
struct StatefulOverlappedCompletion<S> {
    /// Inner data required by the completion processor; must be first to "downcast" to OVERLAPPED.
    inner: OverlappedCompletion,
    /// Caller state, encased in a cell for interior mutability.
    state: UnsafeCell<S>,
}

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
    pub async unsafe fn do_io_with<F1, F2, F3, R, S>(
        &mut self,
        state: S,
        yielder: &Yielder,
        start: F1,
        cancel: F2,
        finish: F3,
    ) -> Result<R, Fail>
    where
        S: 'static,
        for<'a> F1: FnOnce(Pin<&'a mut S>, *mut OVERLAPPED) -> Result<(), Fail>,
        for<'a> F2: FnOnce(Pin<&'a mut S>, *mut OVERLAPPED) -> Result<(), Fail>,
        for<'a> F3: FnOnce(Pin<&'a mut S>, OverlappedResult) -> Result<R, Fail>,
    {
        let mut completion: Pin<Box<StatefulOverlappedCompletion<S>>> = Box::pin(StatefulOverlappedCompletion {
            inner: OverlappedCompletion {
                overlapped: OVERLAPPED::default(),
                yielder_handle: Some(yielder.get_handle()),
                completion_key: 0,
                free: StatefulOverlappedCompletion::<S>::drop_overlapped,
                _marker: PhantomPinned,
            },
            state: UnsafeCell::new(state),
        });

        let overlapped: *mut OVERLAPPED = completion.as_mut().marshal();
        match start(completion.get_state_ref(), overlapped) {
            // Operation in progress, pending overlapped completion.
            Ok(()) => loop {
                let status: Result<(), Fail> = yielder.yield_until_wake().await;

                // NB If the yielder handle is cleared, the event was dequeued from the completion port and
                // processed. If the coroutine was also cancelled, depending on the order of scheduling the result
                // may still indicate failure. YielderHandler absence takes higher precedence here -- no need to
                // signal failure if it's not semantically useful.
                if completion.inner.yielder_handle.is_none() {
                    return finish(
                        completion.get_state_ref(),
                        OverlappedResult::new(&completion.inner.overlapped, completion.inner.completion_key),
                    );
                }

                match status {
                    Ok(()) => {
                        // Spurious wake-up.
                        continue;
                    },

                    Err(err) => {
                        if err.errno == libc::ECANCELED {
                            if let Err(cancel_err) = cancel(completion.get_state_ref(), overlapped) {
                                warn!("cancellation failed: {}", cancel_err);
                            }
                        }

                        // NOTE: the semantics of completion ports with cancellation is unclear: CancelIoEx
                        // documentation implies that some operations may not post a completion packet after this
                        // operation. If completions are found to leak, this may be why.
                        completion.abandon();
                        return Err(err);
                    },
                }
            },

            // Operation failed to start.
            Err(err) => Err(err),
        }
    }

    /// Same as `do_io_with`, but does not use an intermediate state value.
    pub async unsafe fn do_io<F1, F2, F3, R>(
        &mut self,
        yielder: &Yielder,
        start: F1,
        cancel: F2,
        finish: F3,
    ) -> Result<R, Fail>
    where
        F1: FnOnce(*mut OVERLAPPED) -> Result<(), Fail>,
        F2: FnOnce(*mut OVERLAPPED) -> Result<(), Fail>,
        F3: FnOnce(OverlappedResult) -> Result<R, Fail>,
    {
        self.do_io_with(
            (),
            yielder,
            |_, overlapped: *mut OVERLAPPED| -> Result<(), Fail> { start(overlapped) },
            |_, overlapped: *mut OVERLAPPED| -> Result<(), Fail> { cancel(overlapped) },
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
            if let Some(mut yielder_handle) = overlapped.yielder_handle.take() {
                debug_assert!(entry.dwNumberOfBytesTransferred as usize == overlapped.overlapped.InternalHigh);
                overlapped.completion_key = entry.lpCompletionKey;
                yielder_handle.wake_with(Ok(()));
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
    /// Marshal an OVERLAPPED pointer back into an OverlappedCompletion.
    fn unmarshal<'a>(overlapped: std::ptr::NonNull<OVERLAPPED>) -> Pin<&'a mut Self> {
        unsafe { Pin::new_unchecked(&mut *(overlapped.as_ptr() as *mut Self)) }
    }
}

impl<S> StatefulOverlappedCompletion<S> {
    /// Marshal a StatefulOverlappedCompletion into an OVERLAPPED pointer. This type must be pinned for marshaling.
    fn marshal(mut self: Pin<&mut Self>) -> *mut OVERLAPPED {
        unsafe { self.as_mut().get_unchecked_mut() as *mut Self }.cast()
    }

    /// Called by the I/O event processor to free this instance if the completion is abandoned by the waiter.
    unsafe fn drop_overlapped(overlapped: *mut OVERLAPPED) {
        if overlapped != std::ptr::null_mut() {
            let overlapped: *mut Self = overlapped.cast();
            let overlapped: Box<Self> = unsafe { Box::from_raw(overlapped) };
            std::mem::drop(overlapped);
        }
    }

    /// Structural pinning (required) for `inner`.
    fn get_inner<'a>(self: &'a mut Pin<Box<Self>>) -> Pin<&'a mut OverlappedCompletion> {
        unsafe { Pin::new_unchecked(&mut self.as_mut().get_unchecked_mut().inner) }
    }

    /// Structure pinning (probably required) for `state`.
    fn get_state_ref<'a>(self: &Pin<Box<Self>>) -> Pin<&'a mut S> {
        unsafe { Pin::new_unchecked(&mut *self.state.get()) }
    }

    /// Set the yielder handle for the waiter. The yielder handle is not structurally pinned.
    fn set_yielder(self: &mut Pin<Box<Self>>, yielder_handle: Option<YielderHandle>) {
        // Safety: updating the yielder does not violate pinning invariants.
        unsafe { self.get_inner().get_unchecked_mut() }.yielder_handle = yielder_handle;
    }

    /// Abandon this overlapped instance. This will consume and forget `self` so that the waiter may return without
    /// invalidating memory needed by the I/O event processor. This method should only be called when the OVERLAPPED
    /// has not yet been dequeued from the completion port.
    unsafe fn abandon(mut self: Pin<Box<Self>>) {
        self.set_yielder(None);
        std::mem::forget(self)
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

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
        iter,
        pin::pin,
        ptr::NonNull,
        rc::Rc,
        sync::atomic::{
            AtomicU32,
            Ordering,
        },
    };

    use crate::{
        ensure_eq,
        runtime::scheduler::{
            Scheduler,
            Task,
            TaskId,
            TaskWithResult,
        },
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

    #[test]
    fn test_marshal_unmarshal() -> Result<()> {
        let mut completion: Pin<&mut StatefulOverlappedCompletion<()>> = pin!(StatefulOverlappedCompletion {
            inner: OverlappedCompletion {
                overlapped: OVERLAPPED::default(),
                yielder_handle: None,
                completion_key: 0,
                free: StatefulOverlappedCompletion::<()>::drop_overlapped,
                _marker: PhantomPinned,
            },
            state: UnsafeCell::new(()),
        });

        // Ensure that the marshal returns the address of the overlapped member.
        ensure_eq!(
            completion.as_mut().marshal() as *const OVERLAPPED as usize,
            &completion.inner.overlapped as *const OVERLAPPED as usize
        );

        // Ensure that marshal returns the address of the completion.
        ensure_eq!(
            completion.as_mut().marshal() as usize,
            completion.as_ref().get_ref() as *const StatefulOverlappedCompletion<()> as usize
        );

        // This can be inferred transitively from above. The overlapped member must be at offset 0.
        ensure_eq!(
            (&completion.inner.overlapped as *const OVERLAPPED) as usize,
            (completion.as_ref().get_ref() as *const StatefulOverlappedCompletion<()>) as usize
        );

        let overlapped_ptr: NonNull<OVERLAPPED> = NonNull::new(completion.as_mut().marshal()).unwrap();
        let unmarshalled: Pin<&mut OverlappedCompletion> = OverlappedCompletion::unmarshal(overlapped_ptr);

        // Test that unmarshal returns an address which is the same as the OVERLAPPED, which is the same as the original
        // StatefulOverlappedCompletion. This implies that OVERLAPPED is at member offset 0 in all structs.
        ensure_eq!(
            unmarshalled.as_ref().get_ref() as *const OverlappedCompletion as usize,
            completion.as_ref().get_ref() as *const StatefulOverlappedCompletion<()> as usize
        );

        Ok(())
    }

    /// Test that the event processor can dequeue an overlapped event from the completion port.
    #[test]
    fn test_event_processor() -> Result<()> {
        const COMPLETION_KEY: usize = 123;
        let mut iocp: IoCompletionPort = make_iocp()?;
        let mut yielder_handle: YielderHandle = YielderHandle::new();
        let mut overlapped: Pin<&mut StatefulOverlappedCompletion<()>> = pin!(StatefulOverlappedCompletion {
            inner: OverlappedCompletion {
                overlapped: OVERLAPPED::default(),
                yielder_handle: Some(yielder_handle.clone()),
                completion_key: 0,
                free: StatefulOverlappedCompletion::<()>::drop_overlapped,
                _marker: PhantomPinned,
            },
            state: UnsafeCell::new(()),
        });

        post_completion(&iocp, overlapped.as_mut().marshal(), COMPLETION_KEY)?;

        iocp.process_events()?;

        let unpinned_overlapped: &mut StatefulOverlappedCompletion<()> = unsafe { overlapped.get_unchecked_mut() };

        ensure!(
            unpinned_overlapped.inner.yielder_handle.is_none(),
            "yielder should be cleared by iocp"
        );
        ensure_eq!(
            unpinned_overlapped.inner.completion_key,
            COMPLETION_KEY,
            "completion key not updated"
        );

        let result: Option<Result<(), Fail>> = yielder_handle.get_result();
        ensure!(result.is_some(), "yielder handle not woken");
        ensure!(result.unwrap().is_ok(), "yielder handle result not success");

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

        let server: Pin<Box<dyn FusedFuture<Output = Result<(), Fail>>>> = Box::pin(
            async move {
                let yielder: Yielder = Yielder::new();

                unsafe {
                    iocp_ref.do_io(
                        &yielder,
                        |overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                            server_state.fetch_add(1, Ordering::Relaxed);
                            is_overlapped_ok(ConnectNamedPipe(server_pipe.0, Some(overlapped)))
                        },
                        |_| -> Result<(), Fail> { Err(Fail::new(libc::ENOTSUP, "cannot cancel")) },
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
                        &yielder,
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
                        |_, _| -> Result<(), Fail> { Err(Fail::new(libc::ENOTSUP, "cannot cancel")) },
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
                    Ok(())
                }
            }
            .fuse(),
        );

        let mut scheduler: Scheduler = Scheduler::default();
        let server_handle: TaskId = scheduler
            .insert_task(TaskWithResult::<Result<(), Fail>>::new("server".into(), server))
            .unwrap();

        let get_server_result = |task: Box<dyn Task>| {
            TaskWithResult::<Result<(), Fail>>::try_from(task.as_any())
                .expect("should be correct task type")
                .get_result()
                .unwrap()
        };

        let mut wait_for_state = |scheduler: &mut Scheduler, state| -> Result<(), Fail> {
            while server_state_view.load(Ordering::Relaxed) < state {
                iocp.get_mut().process_events()?;
                // Loop for an arbitrary number of quanta.
                if let Some(task) = scheduler.get_next_completed_task(64) {
                    if task.get_id() == server_handle {
                        return Err(get_server_result(task).unwrap_err());
                    }
                }
            }

            Ok(())
        };

        wait_for_state(&mut scheduler, 1)?;

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

        wait_for_state(&mut scheduler, 2)?;

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

        let task = loop {
            iocp.get_mut().process_events()?;
            if let Some(task) = scheduler.get_next_completed_task(64) {
                if task.get_id() == server_handle {
                    break task;
                }
            }
        };

        get_server_result(task)?;

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
        let yielder: Yielder = Yielder::new();
        let mut yielder_handle: YielderHandle = yielder.get_handle();

        let server: Pin<Box<dyn FusedFuture<Output = Result<(), Fail>>>> = Box::pin(
            async move {
                let result: Result<(), Fail> = unsafe {
                    iocp_ref.do_io(
                        &yielder,
                        |overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                            server_state.fetch_add(1, Ordering::Relaxed);
                            is_overlapped_ok(ConnectNamedPipe(server_pipe.0, Some(overlapped)))
                        },
                        |overlapped: *mut OVERLAPPED| -> Result<(), Fail> {
                            CancelIoEx(server_pipe.0, Some(overlapped)).map_err(Fail::from)
                        },
                        |result: OverlappedResult| -> Result<(), Fail> { result.ok() },
                    )
                }
                .await;

                result
            }
            .fuse(),
        );

        let mut scheduler: Scheduler = Scheduler::default();
        let server_handle: TaskId = scheduler
            .insert_task(TaskWithResult::<Result<(), Fail>>::new("server".into(), server))
            .unwrap();

        let get_server_result = |task: Box<dyn Task>| {
            TaskWithResult::<Result<(), Fail>>::try_from(task.as_any())
                .expect("should be correct task type")
                .get_result()
                .unwrap()
        };

        let mut wait_for_state = |scheduler: &mut Scheduler, state| -> Result<(), Fail> {
            while server_state_view.load(Ordering::Relaxed) < state {
                iocp.get_mut().process_events()?;
                // Loop for an arbitrary number of quanta.
                if let Some(task) = scheduler.get_next_completed_task(64) {
                    if task.get_id() == server_handle {
                        return Err(get_server_result(task).unwrap_err());
                    }
                }
            }
            Ok(())
        };

        wait_for_state(&mut scheduler, 1)?;

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
