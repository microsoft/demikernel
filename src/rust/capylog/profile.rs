use std::{time::{Duration, Instant}, collections::HashMap};

//==============================================================================
// Data
//==============================================================================

#[allow(unused)]
static mut DATA: Option<Vec<(&str, Duration)>> = None;

#[allow(unused)]
static mut TOTAL_DATA: Option<HashMap<&str, Duration>> = None;

//==============================================================================
// Macros
//==============================================================================

macro_rules! __capy_profile {
    ($name:expr) => {
        let __capy_log_profile_dropped_object = crate::capylog::profile::__DroppedObject::new($name);
    };
}

/// Merges this time interval with the last one profiled, ensuring that the name is the same.
macro_rules! __capy_profile_merge_previous {
    ($name:expr) => {
        let __capy_log_profile_dropped_object = crate::capylog::profile::__MergeDroppedObject::new($name);
    };
}

macro_rules! __capy_profile_total {
    ($name:expr) => {
        let __capy_log_profile_dropped_object = crate::capylog::profile::__TotalDroppedObject::new($name);
    };
}

macro_rules! __capy_profile_dump {
    ($dump:expr) => {
        $crate::capylog::profile::__write_profiler_data($dump).expect("capy_profile_dump failed");
    };
}

#[allow(unused)]
pub(crate) use __capy_profile;
#[allow(unused)]
pub(crate) use __capy_profile_merge_previous;
#[allow(unused)]
pub(crate) use __capy_profile_total;
#[allow(unused)]
pub(crate) use __capy_profile_dump;

//==============================================================================
// Structures
//==============================================================================

#[allow(unused)]
pub(crate) struct __DroppedObject {
    name: &'static str,
    begin: Instant,
}

#[allow(unused)]
pub(crate) struct __MergeDroppedObject {
    begin: Instant,
}

#[allow(unused)]
pub(crate) struct __TotalDroppedObject {
    name: &'static str,
    begin: Instant,
}

//==============================================================================
// Standard Library Trait Implementations
//==============================================================================

#[allow(unused)]
impl Drop for __DroppedObject {
    fn drop(&mut self) {
        let time = self.begin.elapsed();
        data().push((self.name, time));
    }
}

#[allow(unused)]
impl Drop for __MergeDroppedObject {
    fn drop(&mut self) {
        let end = Instant::now();
        data().last_mut().expect("no previous value").1 += end - self.begin;
    }
}

#[allow(unused)]
impl Drop for __TotalDroppedObject {
    fn drop(&mut self) {
        let time = self.begin.elapsed();
        *total_data().entry(self.name).or_default() += time;
    }
}

//==============================================================================
// Implementations
//==============================================================================

#[allow(unused)]
impl __DroppedObject {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            begin: Instant::now()
        }
    }
}

#[allow(unused)]
impl __MergeDroppedObject {
    pub(crate) fn new(name: &'static str) -> Self {
        match data().last() {
            None => panic!("tcpmig_profiler: no previous value"),
            Some(&(prev, _)) if prev != name => panic!("tcpmig_profiler: expected \"{}\", found \"{}\"", name, prev),
            _ => (),
        }

        Self {
            begin: Instant::now()
        }
    }
}

#[allow(unused)]
impl __TotalDroppedObject {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            begin: Instant::now()
        }
    }
}

//==============================================================================
// Functions
//==============================================================================

#[allow(unused)]
pub(crate) fn __write_profiler_data<W: std::io::Write>(w: &mut W) -> std::io::Result<()> {
    eprintln!("\n[CAPYLOG] dumping profiler data");
    let data: &Vec<(&str, Duration)> = data();
    for (name, datum) in data {
        write!(w, "{},{}\n", name, datum.as_nanos())?;
    }

    eprintln!("\n[CAPYLOG] dumping total profiler data");
    let data: &HashMap<&str, Duration> = total_data();
    for (name, datum) in data {
        write!(w, "{},{}\n", name, datum.as_nanos())?;
    }
    Ok(())
}

#[allow(unused)]
#[inline]
fn data() -> &'static mut Vec<(&'static str, Duration)> {
    unsafe { DATA.as_mut().expect("capy-log profiler not initialised") }
}

#[allow(unused)]
#[inline]
fn total_data() -> &'static mut HashMap<&'static str, Duration> {
    unsafe { TOTAL_DATA.as_mut().expect("capy-log profiler not initialised") }
}

#[allow(unused)]
pub(super) fn init() {
    if unsafe { DATA.as_ref().is_some() } {
        panic!("Double initialisation of capy-log profiler");
    }
    unsafe { DATA = Some(Vec::with_capacity(64)); }
    unsafe { TOTAL_DATA = Some(HashMap::with_capacity(64)); }

    eprintln!("[CAPYLOG] capy_profile is on");
}