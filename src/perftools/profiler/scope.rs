// Copyright(c) Microsoft Corporation.
// Licensed under the MIT license.

//! This module provides data structures related to scoping blocks of code for our profiler.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    perftools::profiler::PROFILER,
    runtime::{types::demi_callback_t, SharedObject},
};
use ::std::{
    fmt::{self, Debug},
    future::Future,
    io,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
    thread,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Internal representation of scopes as a tree. This tracks a single profiling block of code in relationship to other
/// profiled blocks.
pub struct Scope {
    pub name: &'static str,
    pub parent_scope: Option<SharedScope>,
    pub children_scopes: Vec<SharedScope>,
    /// Callback to report statistics. If this is set to None, we collect averages by default.
    pub perf_callback: Option<demi_callback_t>,
    pub num_calls: usize,
    /// In total, how much time has been spent in this scope?
    pub duration_sum: u64,
}

#[derive(Clone)]
pub struct SharedScope(SharedObject<Scope>);

/// A guard that is created when entering a scope and dropped when leaving it.
pub struct SyncScopeGuard {
    enter_time: u64,
}

/// A scope over an async block that may yield and re-enter several times.
pub struct AsyncScope<'a, F: Future> {
    name: &'static str,
    future: Pin<&'a mut F>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedScope {
    pub fn new(
        name: &'static str,
        parent_scope: Option<SharedScope>,
        perf_callback: Option<demi_callback_t>,
    ) -> SharedScope {
        Self(SharedObject::new(Scope {
            name,
            parent_scope,
            children_scopes: Vec::new(),
            num_calls: 0,
            duration_sum: 0,
            perf_callback,
        }))
    }

    /// Enter this scope. Returns a `Guard` instance that should be dropped when leaving the scope.
    #[inline]
    pub fn enter_sync_scope(&self) -> SyncScopeGuard {
        SyncScopeGuard::enter()
    }

    /// Leave this scope. Called automatically by the `Guard` instance.
    #[inline]
    pub fn add_duration(&mut self, duration: u64) {
        if let Some(callback_fn) = self.perf_callback {
            callback_fn(self.name.as_ptr() as *const i8, self.name.len() as u32, duration);
        } else {
            self.num_calls += 1;
            // Even though this is extremely unlikely, let's not panic on overflow.
            self.duration_sum = self.duration_sum + duration;
        }
    }

    pub fn compute_exclusive_duration(&self) -> u64 {
        let mut children_total_duration: u64 = 0;

        for s in &self.children_scopes {
            children_total_duration += s.duration_sum;
        }

        self.duration_sum - children_total_duration
    }

    pub fn write_recursive<W: io::Write>(
        &self,
        out: &mut W,
        thread_id: thread::ThreadId,
        depth: usize,
        ns_per_cycle: f64,
    ) -> io::Result<()> {
        let duration_sum: f64 = self.duration_sum as f64;

        // Write markers.
        let mut markers = String::from("+");
        for _ in 0..depth {
            markers.push('+');
        }

        writeln!(
            out,
            "{},{},{},{},{},{}",
            format!("{},{:?},{}", markers, thread_id, self.name),
            self.num_calls,
            (duration_sum / (self.num_calls as f64)).round(),
            (duration_sum / (self.num_calls as f64) * ns_per_cycle).round(),
            self.duration_sum,
            self.compute_exclusive_duration(),
        )?;

        for child_scope in &self.children_scopes {
            child_scope.write_recursive(out, thread_id, depth + 1, ns_per_cycle)?;
        }

        Ok(())
    }
}

impl<'a, F: Future> AsyncScope<'a, F> {
    pub fn new(name: &'static str, future: Pin<&'a mut F>) -> Self {
        Self { name, future }
    }
}

impl SyncScopeGuard {
    #[inline]
    pub fn enter() -> Self {
        let now: u64 = unsafe { x86::time::rdtscp().0 };
        Self { enter_time: now }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedScope {
    type Target = Scope;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedScope {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Debug for SharedScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl<'a, F: Future> Future for AsyncScope<'a, F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut Self = self.get_mut();

        PROFILER.with(|p| p.clone().create_and_enter_async_scope(&self_.name));
        let start: u64 = unsafe { x86::time::rdtscp().0 };
        let result = Future::poll(self_.future.as_mut(), ctx);
        let end: u64 = unsafe { x86::time::rdtscp().0 };
        let duration: u64 = end - start;
        PROFILER.with(|p| p.clone().leave_async_scope(duration));
        result
    }
}

impl Drop for SyncScopeGuard {
    #[inline]
    fn drop(&mut self) {
        let now: u64 = unsafe { x86::time::rdtscp().0 };
        let duration: u64 = now - self.enter_time;

        PROFILER.with(|p| p.clone().leave_sync_scope(duration));
    }
}
