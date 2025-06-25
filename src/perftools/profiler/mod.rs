// Copyright(c) Microsoft Corporation.
// Licensed under the MIT license.

//! This module provides a small performance profiler for the Demikernel libOSes. It supports two kinds of profilers:
//! ones for synchronous blocks of code and ones for asynchronous blocks of code. For synchronous blocks, we use a
//! [SyncScopeGuard] to automatically scope the timer. The profiler creates the [SyncScopeGuard] when the timer starts
//! and the timer stops when the [SyncScopeGuard] goes out of scope. For asynchronous blocks, we simply create a
//! [SharedScope] and time the number of cycles for each poll. We do not use a guard for scoping asynchronous blocks.

//======================================================================================================================
// Exports
//======================================================================================================================

mod scope;
pub use crate::perftools::profiler::scope::AsyncScope;
#[cfg(test)]
mod tests;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    perftools::profiler::scope::{SharedScope, SyncScopeGuard},
    runtime::{types::demi_callback_t, SharedObject},
};
use ::futures::future::FusedFuture;
use ::std::{
    io,
    ops::{Deref, DerefMut},
    pin::Pin,
    thread,
    time::{Duration, SystemTime},
};

//======================================================================================================================
// Structures
//======================================================================================================================

thread_local!(
    pub static PROFILER: SharedProfiler = SharedProfiler::new();
);

/// A `Profiler` stores the scope tree and keeps track of the currently active scope. Note that there is a global
/// thread-local instance of `Profiler` in [`PROFILER`](constant.PROFILER.html), so it is not possible to manually
/// create an instance of `Profiler`.
pub struct Profiler {
    root_scopes: Vec<SharedScope>,
    current_sync_scope: Option<SharedScope>,
    current_async_scope: Option<SharedScope>,
    perf_callback: Option<demi_callback_t>,
}

#[derive(Clone)]
pub struct SharedProfiler(SharedObject<Profiler>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

pub fn reset() {
    PROFILER.with(|p| p.clone().reset());
}

pub fn set_callback(perf_callback: demi_callback_t) {
    PROFILER.with(|p| p.clone().set_callback(perf_callback));
}

/// Create a special async scopes that is rooted because it does not run under other scopes.
#[inline]
pub async fn coroutine_scope<F: FusedFuture>(name: &'static str, mut coroutine: Pin<Box<F>>) -> F::Output {
    AsyncScope::new(name, coroutine.as_mut()).await
}

impl Profiler {
    fn write<W: io::Write>(&self, out: &mut W) -> io::Result<()> {
        let thread_id = thread::current().id();
        let ns_per_cycle = Self::measure_ns_per_cycle();

        // Header row
        writeln!(
            out,
            "call_depth,thread_id,function_name,num_calls,cycles_per_call,nanoseconds_per_call,total_duration,total_duration_exclusive"
        )?;

        for s in self.root_scopes.iter() {
            s.write_recursive(out, thread_id, 0, ns_per_cycle)?;
        }

        out.flush()
    }

    fn measure_ns_per_cycle() -> f64 {
        let start: SystemTime = SystemTime::now();
        let start_cycle: u64 = unsafe { x86::time::rdtscp().0 };

        test::black_box((0..10000).fold(0, |old, new| old ^ new)); // dummy calculations for measurement

        let end_cycle: u64 = unsafe { x86::time::rdtscp().0 };
        let since_the_epoch: Duration = SystemTime::now().duration_since(start).expect("Time went backwards");
        let in_ns: u64 = since_the_epoch.as_secs() * 1_000_000_000 + since_the_epoch.subsec_nanos() as u64;

        in_ns as f64 / (end_cycle - start_cycle) as f64
    }
}

impl SharedProfiler {
    pub fn new() -> SharedProfiler {
        Self(SharedObject::new(Profiler {
            root_scopes: Vec::new(),
            current_sync_scope: None,
            current_async_scope: None,
            perf_callback: None,
        }))
    }

    pub fn set_callback(&mut self, perf_callback: demi_callback_t) {
        self.perf_callback = Some(perf_callback)
    }

    fn find_or_create_new_scope(
        scopes: &mut Vec<SharedScope>,
        name: &'static str,
        parent_scope: Option<SharedScope>,
        perf_callback: Option<demi_callback_t>,
    ) -> SharedScope {
        match scopes.iter().find(|s| s.name == name) {
            Some(existing_scope) => existing_scope.clone(),
            None => {
                let new_scope: SharedScope = SharedScope::new(name, parent_scope, perf_callback);
                scopes.push(new_scope.clone());
                new_scope
            },
        }
    }

    /// Create and enter a syncronous scope. Returns a [`Guard`](struct.Guard.html) that should be dropped upon
    /// leaving the scope. Usually, this method will be called by the [`profile`](macro.profile.html) macro,
    /// so it does not need to be used directly.
    fn create_scope(&mut self, current_scope: &mut Option<SharedScope>, name: &'static str) -> SharedScope {
        let perf_callback: Option<demi_callback_t> = self.perf_callback;
        match current_scope {
            Some(current_scope) => {
                let parent_scope: Option<SharedScope> = Some(current_scope.clone());
                Self::find_or_create_new_scope(&mut current_scope.children_scopes, name, parent_scope, perf_callback)
            },
            None => Self::find_or_create_new_scope(&mut self.root_scopes, name, None, perf_callback),
        }
    }

    pub fn create_and_enter_sync_scope(&mut self, name: &'static str) -> SyncScopeGuard {
        let mut current_scope: Option<SharedScope> = self.current_sync_scope.clone();
        let scope = self.create_scope(&mut current_scope, name);
        self.current_sync_scope = Some(scope.clone());
        scope.enter_sync_scope()
    }

    #[inline]
    fn leave_sync_scope(&mut self, duration: u64) {
        // Note that we could now still be anywhere in the previous profiling
        // tree, so we can not simply reset `self.current`. However, as the
        // frame comes to an end we will eventually leave a root node, at which
        // point `self.current` will be set to `None`.
        self.current_sync_scope = if let Some(mut current_scope) = self.current_sync_scope.take() {
            current_scope.add_duration(duration);
            current_scope.parent_scope.as_ref().cloned()
        } else {
            // This should not happen with proper usage.
            unreachable!("Called perftools::profiler::leave() while not in any scope");
        };
    }

    pub fn create_and_enter_async_scope(&mut self, name: &'static str) {
        let mut current_scope: Option<SharedScope> = self.current_async_scope.clone();
        let scope = self.create_scope(&mut current_scope, name);
        self.current_async_scope = Some(scope.clone());
    }

    #[inline]
    fn leave_async_scope(&mut self, duration: u64) {
        self.current_async_scope = if let Some(mut current_scope) = self.current_async_scope.take() {
            current_scope.add_duration(duration);
            current_scope.parent_scope.as_ref().cloned()
        } else {
            // This should not happen with proper usage.
            unreachable!("Called perftools::profiler::leave() while not in any scope");
        };
    }
    fn reset(&mut self) {
        self.root_scopes.clear();
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedProfiler {
    type Target = Profiler;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedProfiler {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Profiler {
    fn drop(&mut self) {
        self.write(&mut std::io::stdout()).expect("failed to write to stdout");
    }
}
