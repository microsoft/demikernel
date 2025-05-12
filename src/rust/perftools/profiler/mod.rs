// Copyright(c) Microsoft Corporation.
// Licensed under the MIT license.

//! This module provides a small performance profiler for the Demikernel libOSes.

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
    perftools::profiler::scope::{Guard, SharedScope},
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
    pub static PROFILER: SharedProfiler = SharedProfiler::new()
);

/// A `Profiler` stores the scope tree and keeps track of the currently active scope. Note that there is a global
/// thread-local instance of `Profiler` in [`PROFILER`](constant.PROFILER.html), so it is not possible to manually
/// create an instance of `Profiler`.
pub struct Profiler {
    root_scopes: Vec<SharedScope>,
    current_scope: Option<SharedScope>,
    perf_callback: Option<demi_callback_t>,
}

#[derive(Clone)]
pub struct SharedProfiler(SharedObject<Profiler>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Print profiling scope tree. Percentages represent the amount of time taken relative to the parent node. Frequencies
/// are computed with respect to the total amount of time spent in root nodes. Thus, if you have multiple root nodes
/// and they do not cover all code that runs in your program, the printed frequencies will be overestimated.
pub fn write<W: io::Write>(out: &mut W) -> io::Result<()> {
    PROFILER.with(|p| p.write(out))
}

pub fn reset() {
    PROFILER.with(|p| p.clone().reset());
}

pub fn set_callback(perf_callback: demi_callback_t) {
    PROFILER.with(|p| p.clone().set_callback(perf_callback));
}

impl SharedProfiler {
    pub fn new() -> SharedProfiler {
        Self(SharedObject::new(Profiler::new()))
    }

    /// Create a special async scopes that is rooted because it does not run under other scopes.
    #[inline]
    pub async fn coroutine_scope<F: FusedFuture>(name: &'static str, mut coroutine: Pin<Box<F>>) -> F::Output {
        AsyncScope::new(
            PROFILER.with(|p| p.clone().get_or_create_root_scope(name)),
            coroutine.as_mut(),
        )
        .await
    }
}

impl Profiler {
    fn new() -> Profiler {
        Self {
            root_scopes: Vec::new(),
            current_scope: None,
            perf_callback: None,
        }
    }

    pub fn set_callback(&mut self, perf_callback: demi_callback_t) {
        self.perf_callback = Some(perf_callback)
    }

    /// Create and enter a syncronous scope. Returns a [`Guard`](struct.Guard.html) that should be dropped upon
    /// leaving the scope. Usually, this method will be called by the [`profile`](macro.profile.html) macro,
    /// so it does not need to be used directly.
    #[inline]
    pub fn sync_scope(&mut self, name: &'static str) -> Guard {
        let scope = self.get_or_create_scope(name);
        self.enter_scope(&scope)
    }

    fn get_or_create_root_scope(&mut self, name: &'static str) -> SharedScope {
        let existing_root_scope = self.root_scopes.iter().find(|s| s.name == name).cloned();

        existing_root_scope.unwrap_or_else(|| {
            let new_scope: SharedScope = SharedScope::new(name, None, self.perf_callback);
            self.root_scopes.push(new_scope.clone());
            new_scope
        })
    }

    pub fn get_or_create_scope(&mut self, name: &'static str) -> SharedScope {
        match self.current_scope.as_ref() {
            Some(current_scope) => {
                let existing_scope: Option<SharedScope> =
                    current_scope.children_scopes.iter().find(|s| s.name == name).cloned();

                existing_scope.unwrap_or_else(|| {
                    let new_scope: SharedScope =
                        SharedScope::new(name, Some(current_scope.clone()), self.perf_callback);
                    current_scope.clone().add_child_scope(new_scope.clone());
                    new_scope
                })
            },
            None => self.get_or_create_root_scope(name),
        }
    }

    fn enter_scope(&mut self, scope: &SharedScope) -> Guard {
        let guard = scope.enter();
        self.current_scope = Some(scope.clone());
        guard
    }

    fn reset(&mut self) {
        self.root_scopes.clear();

        // Note that we could now still be anywhere in the previous profiling
        // tree, so we can not simply reset `self.current`. However, as the
        // frame comes to an end we will eventually leave a root node, at which
        // point `self.current` will be set to `None`.
    }

    #[inline]
    fn leave_scope(&mut self, duration: u64) {
        self.current_scope = if let Some(current_scope) = self.current_scope.as_ref() {
            current_scope.clone().leave(duration);
            current_scope.parent_scope.as_ref().cloned()
        } else {
            // This should not happen with proper usage.
            unreachable!("Called perftools::profiler::leave() while not in any scope");
        };
    }

    fn write<W: io::Write>(&self, out: &mut W) -> io::Result<()> {
        let thread_id = thread::current().id();
        let ns_per_cycle = Self::measure_ns_per_cycle();

        // Header row
        writeln!(
            out,
            "call_depth,thread_id,function_name,num_calls,cycles_per_call,nanoseconds_per_call,total_duration,total_duration_exclusive"
        )?;

        self.write_root_scopes(out, thread_id, ns_per_cycle)?;

        out.flush()
    }

    fn write_root_scopes<W: io::Write>(
        &self,
        out: &mut W,
        thread_id: thread::ThreadId,
        ns_per_cycle: f64,
    ) -> Result<(), io::Error> {
        for s in self.root_scopes.iter() {
            s.write_recursive(out, thread_id, 0, ns_per_cycle)?;
        }
        Ok(())
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
