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

use crate::perftools::profiler::scope::{
    Guard,
    Scope,
};
use ::futures::future::FusedFuture;
use ::std::{
    cell::RefCell,
    io,
    pin::Pin,
    rc::Rc,
    time::{
        Duration,
        SystemTime,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

#[cfg(feature = "auto-calibrate")]
const SAMPLE_SIZE: usize = 16641;

thread_local!(
    /// Global thread-local instance of the profiler.
    pub static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new())
);

/// A `Profiler` stores the scope tree and keeps track of the currently active
/// scope.
///
/// Note that there is a global thread-local instance of `Profiler` in
/// [`PROFILER`](constant.PROFILER.html), so it is not possible to manually
/// create an instance of `Profiler`.
pub struct Profiler {
    roots: Vec<Rc<RefCell<Scope>>>,
    current: Option<Rc<RefCell<Scope>>>,
    ns_per_cycle: f64,
    #[cfg(feature = "auto-calibrate")]
    clock_drift: u64,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Print profiling scope tree.
///
/// Percentages represent the amount of time taken relative to the parent node.
///
/// Frequencies are computed with respect to the total amount of time spent in
/// root nodes. Thus, if you have multiple root nodes and they do not cover
/// all code that runs in your program, the printed frequencies will be
/// overestimated.
pub fn write<W: io::Write>(out: &mut W, max_depth: Option<usize>) -> io::Result<()> {
    PROFILER.with(|p| p.borrow().write(out, max_depth))
}

/// Reset profiling information.
pub fn reset() {
    PROFILER.with(|p| p.borrow_mut().reset());
}

impl Profiler {
    fn new() -> Profiler {
        Profiler {
            roots: Vec::new(),
            current: None,
            ns_per_cycle: Self::measure_ns_per_cycle(),
            #[cfg(feature = "auto-calibrate")]
            clock_drift: Self::clock_drift(SAMPLE_SIZE),
        }
    }

    /// Create and enter a syncronous scope. Returns a [`Guard`](struct.Guard.html) that should be
    /// dropped upon leaving the scope.
    ///
    /// Usually, this method will be called by the
    /// [`profile`](macro.profile.html) macro, so it does not need to be used
    /// directly.
    #[inline]
    pub fn sync_scope(&mut self, name: &'static str) -> Guard {
        let scope = self.get_scope(name);
        self.enter_scope(scope)
    }

    /// Create and enter a coroutine scope. These are special async scopes that are always rooted because they do not
    /// run under other scopes.
    #[inline]
    pub async fn coroutine_scope<F: FusedFuture>(name: &'static str, mut coroutine: Pin<Box<F>>) -> F::Output {
        AsyncScope::new(
            PROFILER.with(|p| p.borrow_mut().get_root_scope(name)),
            coroutine.as_mut(),
        )
        .await
    }

    /// Looks up the scope at the root level using the name, creating a new one if not found.
    fn get_root_scope(&mut self, name: &'static str) -> Rc<RefCell<Scope>> {
        //Check if `name` already is a root.
        let existing_root = self.roots.iter().find(|root| root.borrow().get_name() == name).cloned();

        existing_root.unwrap_or_else(|| {
            // Add a new root node.
            let new_scope = Scope::new(name, None);
            let succ = Rc::new(RefCell::new(new_scope));

            self.roots.push(succ.clone());

            succ
        })
    }

    /// Look up the scope using the name.
    pub fn get_scope(&mut self, name: &'static str) -> Rc<RefCell<Scope>> {
        // Check if we have already registered `name` at the current point in
        // the tree.
        if let Some(current) = self.current.as_ref() {
            // We are currently in some scope.
            let existing_succ = current
                .borrow()
                .get_succs()
                .iter()
                .find(|succ| succ.borrow().get_name() == name)
                .cloned();

            existing_succ.unwrap_or_else(|| {
                // Add new successor node to the current node.
                let new_scope = Scope::new(name, Some(current.clone()));
                let succ = Rc::new(RefCell::new(new_scope));

                current.borrow_mut().add_succ(succ.clone());

                succ
            })
        } else {
            // We are currently not within any scope.
            self.get_root_scope(name)
        }
    }

    /// Actually enter a scope.
    fn enter_scope(&mut self, scope: Rc<RefCell<Scope>>) -> Guard {
        let guard = scope.borrow_mut().enter();

        self.current = Some(scope);

        guard
    }

    /// Completely reset profiling data.
    fn reset(&mut self) {
        self.roots.clear();

        // Note that we could now still be anywhere in the previous profiling
        // tree, so we can not simply reset `self.current`. However, as the
        // frame comes to an end we will eventually leave a root node, at which
        // point `self.current` will be set to `None`.
    }

    /// Leave the current scope.
    #[inline]
    fn leave_scope(&mut self, duration: u64) {
        self.current = if let Some(current) = self.current.as_ref() {
            cfg_if::cfg_if! {
                if #[cfg(feature = "auto-calibrate")] {
                    let d = duration.checked_sub(self.clock_drift);
                    current.borrow_mut().leave(d.unwrap_or(duration));
                } else {
                    current.borrow_mut().leave(duration);
                }
            }

            // Set current scope back to the parent node (if any).
            current.borrow().get_pred().as_ref().cloned()
        } else {
            // This should not happen with proper usage.
            log::error!("Called perftools::profiler::leave() while not in any scope");

            None
        };
    }

    fn write<W: io::Write>(&self, out: &mut W, max_depth: Option<usize>) -> io::Result<()> {
        let total_duration = self.roots.iter().map(|root| root.borrow().get_duration_sum()).sum();

        writeln!(
            out,
            "call-depth;function-name;percent-total;cycles-per-call;ns-per-call"
        )?;
        for root in self.roots.iter() {
            root.borrow()
                .write_recursive(out, total_duration, 0, max_depth, self.ns_per_cycle)?;
        }

        out.flush()
    }

    fn measure_ns_per_cycle() -> f64 {
        let start: SystemTime = SystemTime::now();
        let (start_cycle, _): (u64, u32) = unsafe { x86::time::rdtscp() };

        test::black_box((0..10000).fold(0, |old, new| old ^ new)); // dummy calculations for measurement

        let (end_cycle, _): (u64, u32) = unsafe { x86::time::rdtscp() };
        let since_the_epoch: Duration = SystemTime::now().duration_since(start).expect("Time went backwards");
        let in_ns: u64 = since_the_epoch.as_secs() * 1_000_000_000 + since_the_epoch.subsec_nanos() as u64;

        in_ns as f64 / (end_cycle - start_cycle) as f64
    }

    #[cfg(feature = "auto-calibrate")]
    fn clock_drift(nsamples: usize) -> u64 {
        let mut total = 0;

        for _ in 0..nsamples {
            let now: u64 = x86::time::rdtscp();
            let duration: u64 = x86::time::rdtscp() - now;

            let d = total + duration;
        }

        total / (nsamples as u64)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for Profiler {
    fn drop(&mut self) {
        self.write(&mut std::io::stdout(), None)
            .expect("failed to write to stdout");
    }
}
