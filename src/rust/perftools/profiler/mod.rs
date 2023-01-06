// Copyright(c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(test)]
mod tests;

use ::std::{
    cell::RefCell,
    io,
    rc::Rc,
    time::{
        Duration,
        SystemTime,
    },
};

#[cfg(feature = "auto-calibrate")]
const SAMPLE_SIZE: usize = 16641;

thread_local!(
    /// Global thread-local instance of the profiler.
    pub static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new())
);

/// Use this macro to add the current scope to profiling. In effect, the time
/// taken from entering to leaving the scope will be measured.
///
/// Internally, the scope is inserted in the scope tree of the global
/// thread-local [`PROFILER`](constant.PROFILER.html).
///
/// # Example
///
/// The following example will profile the scope `"foo"`, which has the scope
/// `"bar"` as a child.
///
/// ```
/// use inetstack::timer;
///
/// {
///     timer!("foo");
///
///     {
///         timer!("bar");
///         // ... do something ...
///     }
///
///     // ... do some more ...
/// }
/// ```

#[macro_export]
macro_rules! timer {
    ($name:expr) => {
        let _guard = $crate::perftools::profiler::PROFILER.with(|p| p.borrow_mut().enter($name));
    };
}

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

//==============================================================================
//
//==============================================================================

/// Internal representation of scopes as a tree.
struct Scope {
    /// Name of the scope.
    name: &'static str,

    /// Parent scope in the tree. Root scopes have no parent.
    pred: Option<Rc<RefCell<Scope>>>,

    /// Child scopes in the tree.
    succs: Vec<Rc<RefCell<Scope>>>,

    /// How often has this scope been visited?
    num_calls: usize,

    /// In total, how much time has been spent in this scope?
    duration_sum: u64,
}

impl Scope {
    fn new(name: &'static str, pred: Option<Rc<RefCell<Scope>>>) -> Scope {
        Scope {
            name,
            pred,
            succs: Vec::new(),
            num_calls: 0,
            duration_sum: 0,
        }
    }

    /// Enter this scope. Returns a `Guard` instance that should be dropped
    /// when leaving the scope.
    #[inline]
    fn enter(&mut self) -> Guard {
        Guard::enter()
    }

    /// Leave this scope. Called automatically by the `Guard` instance.
    #[inline]
    fn leave(&mut self, duration: u64) {
        self.num_calls += 1;

        // Even though this is extremely unlikely, let's not panic on overflow.
        self.duration_sum = self.duration_sum + duration;
    }

    fn write_recursive<W: io::Write>(
        &self,
        out: &mut W,
        total_duration: u64,
        depth: usize,
        max_depth: Option<usize>,
        ns_per_cycle: f64,
    ) -> io::Result<()> {
        if let Some(d) = max_depth {
            if depth > d {
                return Ok(());
            }
        }

        let total_duration_secs = (total_duration) as f64;
        let duration_sum_secs = (self.duration_sum) as f64;
        let pred_sum_secs = self
            .pred
            .clone()
            .map_or(total_duration_secs, |pred| (pred.borrow().duration_sum) as f64);
        let percent = duration_sum_secs / pred_sum_secs * 100.0;

        // Write markers.
        let mut markers = String::from(" +");
        for _ in 0..depth {
            markers.push('+');
        }
        writeln!(
            out,
            "{: <60} {: >6.2}%, {: >18.4} cycles, {: >18.4} ns",
            format!(" {}  {}", markers, self.name),
            percent,
            duration_sum_secs / (self.num_calls as f64),
            duration_sum_secs / (self.num_calls as f64) * ns_per_cycle,
        )?;

        // Write children
        for succ in &self.succs {
            succ.borrow()
                .write_recursive(out, total_duration, depth + 1, max_depth, ns_per_cycle)?;
        }

        Ok(())
    }
}

//==============================================================================
//
//==============================================================================

/// A guard that is created when entering a scope and dropped when leaving it.
pub struct Guard {
    enter_time: u64,
}

impl Guard {
    #[inline]
    fn enter() -> Self {
        let (now, _): (u64, u32) = unsafe { x86::time::rdtscp() };
        Self { enter_time: now }
    }
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        let (now, _): (u64, u32) = unsafe { x86::time::rdtscp() };
        let duration: u64 = now - self.enter_time;
        PROFILER.with(|p| p.borrow_mut().leave(duration));
    }
}

//==============================================================================
//
//==============================================================================

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

    /// Enter a scope. Returns a [`Guard`](struct.Guard.html) that should be
    /// dropped upon leaving the scope.
    ///
    /// Usually, this method will be called by the
    /// [`profile`](macro.profile.html) macro, so it does not need to be used
    /// directly.
    #[inline]
    pub fn enter(&mut self, name: &'static str) -> Guard {
        // Check if we have already registered `name` at the current point in
        // the tree.
        let succ = if let Some(current) = self.current.as_ref() {
            // We are currently in some scope.
            let existing_succ = current
                .borrow()
                .succs
                .iter()
                .find(|succ| succ.borrow().name == name)
                .cloned();

            existing_succ.unwrap_or_else(|| {
                // Add new successor node to the current node.
                let new_scope = Scope::new(name, Some(current.clone()));
                let succ = Rc::new(RefCell::new(new_scope));

                current.borrow_mut().succs.push(succ.clone());

                succ
            })
        } else {
            // We are currently not within any scope. Check if `name` already
            // is a root.
            let existing_root = self.roots.iter().find(|root| root.borrow().name == name).cloned();

            existing_root.unwrap_or_else(|| {
                // Add a new root node.
                let new_scope = Scope::new(name, None);
                let succ = Rc::new(RefCell::new(new_scope));

                self.roots.push(succ.clone());

                succ
            })
        };

        let guard = succ.borrow_mut().enter();

        self.current = Some(succ);

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
    fn leave(&mut self, duration: u64) {
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
            current.borrow().pred.as_ref().cloned()
        } else {
            // This should not happen with proper usage.
            log::error!("Called perftools::profiler::leave() while not in any scope");

            None
        };
    }

    fn write<W: io::Write>(&self, out: &mut W, max_depth: Option<usize>) -> io::Result<()> {
        let total_duration = self.roots.iter().map(|root| root.borrow().duration_sum).sum();

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
