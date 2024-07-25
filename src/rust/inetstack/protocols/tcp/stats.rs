
use std::{
    collections::VecDeque,
    net::SocketAddrV4,
    cell::Cell, rc::Rc, fmt::Debug,
};

use arrayvec::ArrayVec;

use crate::{capy_log_mig, capy_log, capy_time_log};

//======================================================================================================================
// Constants
//======================================================================================================================

const BUCKET_SIZE_LOG2: usize = 3;

const ROLLING_AVG_WINDOW_LOG2: usize = 7;
const ROLLING_AVG_WINDOW: usize = 1 << ROLLING_AVG_WINDOW_LOG2;

pub const MAX_EXTRACTED_CONNECTIONS: usize = 20;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Stats {
    /// Global recv stat at any moment.
    global_stat: usize,
    /// Rolling average of global recv stat.
    avg_global_stat: RollingAverage,
    /// Sorted bucket list of connections according to individual recv stats.
    buckets: BucketList,
    /// List of stat handles to poll.
    handles: Vec<StatsHandle>,
    
    /// Global recv stat threshold above which migration is triggered.
    threshold: usize,
    /// TEMP TEST: Max number of connections to migrate for this test.
    max_proactive_migrations: Option<i32>,
    max_reactive_migrations: Option<i32>,
}

#[derive(Debug)]
struct BucketList {
    /// Index i: Connections with stat from (2^BUCKET_SIZE_LOG2) * i to (2^BUCKET_SIZE_LOG2) * (i+1) - 1. 
    ///
    /// i.e., bucket_index = stat >> BUCKET_SIZE_LOG2
    ///
    /// NOTE: 0 length connections are not stored
    buckets: Vec<Vec<StatsHandle>>,
}

#[derive(Clone)]
pub struct StatsHandle {
    inner: Rc<StatsHandleInner>
}

#[derive(Debug, Clone, Copy, Default)]
enum Stat {
    #[default]
    Disabled,

    Enabled(usize),
    MarkedForDisable(usize),
}

struct StatsHandleInner {
    /// Connection endpoints.
    conn: (SocketAddrV4, SocketAddrV4),
    /// Tracked stat. If None, stop tracking this stat.
    stat: Cell<Stat>,
    /// Signals to update stat stat with this stat.
    stat_to_update: Cell<Option<usize>>,
    /// Position of this stat in the bucket list. None if not present (stat is zero).
    bucket_list_position: Cell<Option<(usize, usize)>>,
    /// Index of this stat in the tracked handles list.
    handles_index: Cell<Option<usize>>,
    /// The value of the stat right before it was disabled.
    stat_right_before_disable: Cell<Option<usize>>,
}

pub struct RollingAverage {
    values: VecDeque<usize>,
    sum: usize,
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

impl Debug for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stats")
            .field("global_stat", &self.global_stat)
            .field("avg_global_stat", &self.avg_global_stat.get())
            .field("handles", &self.handles)
            .field("buckets", &self.buckets)
            .finish()
    }
}

impl Debug for StatsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.as_ref();
        f.debug_struct("StatsHandle")
            .field("conn", &format_args!("({}, {})", inner.conn.0, inner.conn.1))
            .field("stat", &format_args!("{:?}", inner.stat.get()))
            .field("stat_to_update", &format_args!("{:?}", inner.stat_to_update.get()))
            .field("bucket_list_position", &format_args!("{:?}", inner.bucket_list_position.get()))
            .field("handles_index", &format_args!("{:?}", inner.handles_index.get()))
            .finish()
    }
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl Stats {
    pub fn new() -> Self {
        Self {
            global_stat: 0,
            avg_global_stat: RollingAverage::new(),
            buckets: BucketList::new(),
            handles: Vec::new(),
            
            threshold: std::env::var("RECV_QUEUE_LEN_THRESHOLD").expect("No RECV_QUEUE_LEN_THRESHOLD specified")
                .parse().expect("RECV_QUEUE_LEN_THRESHOLD should be a number"),

            max_proactive_migrations: std::env::var("MAX_PROACTIVE_MIGS")
                .map(|e| Some(e.parse().expect("MAX_PROACTIVE_MIGS should be a number")))
                .unwrap_or(None),
            max_reactive_migrations: std::env::var("MAX_REACTIVE_MIGS")
                .map(|e| Some(e.parse().expect("MAX_REACTIVE_MIGS should be a number")))
                .unwrap_or(None),
        }
    }

    pub fn global_stat(&self) -> usize {
        self.global_stat
    }

    pub fn avg_global_stat(&self) -> usize {
        self.avg_global_stat.get()
    }

    pub fn threshold(&self) -> usize {
        self.threshold
    }

    pub fn update_threshold(&mut self, global_stat_sum: usize) {
        // TEMP
        const SERVER_COUNT: usize = 2;

        let global_stat_avg = global_stat_sum / SERVER_COUNT;
        
        #[cfg(not(feature = "manual-tcp-migration"))]{
            // Set threshold to 1.125 * global_stat_sum.
            self.threshold = global_stat_avg + (global_stat_avg >> 3);
        }
    }

    pub fn set_threshold(&mut self, threshold: usize) {
        self.threshold = threshold;
    }

    /// Extracts connections so that the global stat goes below the threshold.
    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn connections_to_proactively_migrate(&mut self) -> Option<ArrayVec<(SocketAddrV4, SocketAddrV4), MAX_EXTRACTED_CONNECTIONS>> {
        if let Some(val) = self.max_proactive_migrations {
            if val <= 0 {
                return None;
            }
        }
        let mut conns = ArrayVec::new();

        let mut should_end = self.global_stat <= self.threshold || conns.remaining_capacity() == 0;
        let mut removed_stat = 0;
        if should_end {
            return None;
        }

        let base_index = std::cmp::min((self.global_stat - self.threshold) >> BUCKET_SIZE_LOG2, self.buckets.buckets.len());

        capy_log_mig!("Need to migrate: base_index({})\nbuckets: {:#?}", base_index, self.buckets.buckets);

        let (left, right) = self.buckets.buckets.split_at_mut(base_index);
        let iter = right.iter_mut().chain(left.iter_mut().rev());

        'outer: for bucket in iter {
            for handle in bucket.iter().rev() {
                capy_log_mig!("Chose: {:#?}", handle);

                removed_stat += handle.inner.stat.get().expect_enabled("bucket list stat not enabled");
                conns.push(handle.inner.conn);
                should_end = self.global_stat - removed_stat <= self.threshold || conns.remaining_capacity() == 0;
                
                if let Some(val) = self.max_proactive_migrations.as_mut() {
                    *val -= 1;
                    if *val <= 0 {
                        break 'outer;
                    }
                }
                
                if should_end {
                    break 'outer;
                }
            }
        }

        Some(conns)
    }

    pub fn reset_stats(&mut self) {
        for bucket in self.buckets.buckets.iter_mut() {
            for stat in bucket {
                stat.inner.stat_to_update.set(Some(0));
            }
        }
        capy_log!("reset stat");
    }

    /// Extracts connections so that the global recv stat goes below the threshold.
    /// Returns `None` if no connections need to be migrated.
    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn connections_to_reactively_migrate(&mut self) -> Option<ArrayVec<(SocketAddrV4, SocketAddrV4), MAX_EXTRACTED_CONNECTIONS>> {
        return None;
        if let Some(val) = self.max_reactive_migrations {
            if val <= 0 {
                return None;
            }
        }

        if self.avg_global_stat.get() <= self.threshold || self.global_stat <= self.threshold {
            return None;
        }

        // Reset global avg.
        self.avg_global_stat.force_set(0);

        let mut conns = ArrayVec::new();

        let mut should_end = self.global_stat <= self.threshold || conns.remaining_capacity() == 0;
        let mut removed_stat = 0;
        let base_index = std::cmp::min((self.global_stat - self.threshold) >> BUCKET_SIZE_LOG2, self.buckets.buckets.len());

        capy_log_mig!("Need to migrate: base_index({})\nbuckets: {:#?}", base_index, self.buckets.buckets);

        let (left, right) = self.buckets.buckets.split_at_mut(base_index);
        let iter = right.iter_mut().chain(left.iter_mut().rev());

        'outer: for bucket in iter {
            for handle in bucket.iter().rev() {
                capy_log_mig!("Chose: {:#?}", handle);

                removed_stat += handle.inner.stat.get().expect_enabled("bucket list stat not enabled");
                conns.push(handle.inner.conn);
                should_end = self.global_stat - removed_stat <= self.threshold || conns.remaining_capacity() == 0;
                
                if let Some(val) = self.max_reactive_migrations.as_mut() {
                    *val -= 1;
                    if *val <= 0 {
                        break 'outer;
                    }
                }
                
                if should_end {
                    break 'outer;
                }
            }
        }

        self.avg_global_stat.update(self.global_stat);

        Some(conns)
    }

    pub fn poll(&mut self) {
        let mut i = 0;
        while i < self.handles.len() {
            let inner = self.handles[i].inner.as_ref();

            // This handle is disabled, remove it.
            let stat = match inner.stat.get() {
                Stat::MarkedForDisable(len) => {    
                    capy_log!("Disable stats for {:?}", inner.conn);
    
                    let position = inner.bucket_list_position.get();
    
                    // Remove from handles. It will not be polled anymore.
                    let handle = self.handles.swap_remove(i);
                    handle.inner.handles_index.set(None);
                    if i < self.handles.len() {
                        self.handles[i].inner.handles_index.set(Some(i));
                    }
    
                    // Remove from buckets.
                    if let Some(position) = position {
                        self.buckets.remove(position);
                        handle.inner.bucket_list_position.set(None);
                    }

                    handle.inner.stat.set(Stat::Disabled);

                    // Update global stat.
                    self.global_stat = self.global_stat.checked_sub(len).unwrap();

                    continue;
                },

                Stat::Enabled(len) => len,
                Stat::Disabled => panic!("disabled stat found in handles vector"),
            };

            // This handle was updated.
            if let Some(stat_to_update) = inner.stat_to_update.take() {
                // Only update if the new value is different from the old value.
                if stat_to_update != stat {
                    capy_log!("Update stats for {:?}", inner.conn);
                    
                    // Update global stat.
                    self.global_stat = (self.global_stat + stat_to_update) - stat;
                    
                    inner.stat.set(Stat::Enabled(stat_to_update));

                    // Update buckets.
                    self.buckets.update(&self.handles[i], stat_to_update);
                }
            }

            i += 1;
        }

        // Update rolling average of global stat.
        self.avg_global_stat.update(self.global_stat);
    }

    pub fn print_bucket_status(&self) {
        for (bucket_idx, bucket) in self.buckets.buckets.iter().enumerate() {
            for entry in bucket.iter() {
                capy_time_log!("RPS_PER_CONN,{},{},{:?}", bucket_idx, entry.inner.conn.1, entry.inner.stat.get().expect_enabled("bucket list stat not enabled"));
            }
        }
    }
}

impl BucketList {
    fn new() -> Self {
        Self { buckets: Vec::new() }
    }

    fn insert(&mut self, handle: &StatsHandle, val: usize) {
        if val > 0 {
            let index = val >> BUCKET_SIZE_LOG2;
            let bucket = self.get_bucket_mut(index);
            let sub_index = bucket.len();

            // Set its position in buckets in the handle.
            handle.inner.bucket_list_position.set(Some((index, sub_index)));

            bucket.push(handle.clone());
        }
    }

    fn remove(&mut self, position: (usize, usize)) -> StatsHandle {
        let (index, sub_index) = position;
        let bucket = &mut self.buckets[index];
        // Remove handle from position.
        let removed = bucket.swap_remove(sub_index);
        removed.inner.bucket_list_position.take();
        // Update the handle newly moved to that position.
        if sub_index < bucket.len() {
            bucket[sub_index].inner.bucket_list_position.set(Some((index, sub_index)));
        }
        removed
    }

    /// Returns the new position.
    fn update(&mut self, handle: &StatsHandle, val: usize) {
        let position = handle.inner.bucket_list_position.get();
        let new_index = val >> BUCKET_SIZE_LOG2;

        match position {
            // Does not exist in buckets.
            None => {
                // Must be added to buckets.
                if val > 0 {
                    self.push_into_bucket(new_index, handle.clone());
                }
            },

            // Exists in buckets.
            Some((index, sub_index)) => {
                // Needs to be moved.
                if val == 0 || index != new_index {
                    // Remove from old bucket.
                    let handle = self.remove((index, sub_index));

                    // Must be moved to the new bucket.
                    if val > 0 {
                        self.push_into_bucket(new_index, handle);
                    }
                }
            }
        }
        //capy_log_mig!("Stats Update Iteration: {:#?}", self.buckets);
    }

    #[inline]
    fn push_into_bucket(&mut self, index: usize, handle: StatsHandle) {
        let bucket = self.get_bucket_mut(index);
        let sub_index = bucket.len();
        let position = (index, sub_index);
        handle.inner.bucket_list_position.set(Some(position));
        bucket.push(handle);
    }

    #[inline]
    fn get_bucket_mut(&mut self, index: usize) -> &mut Vec<StatsHandle> {
        if index >= self.buckets.len() {
            self.buckets.resize(index + 1, Vec::new());
        }
        &mut self.buckets[index]
    }
}

impl StatsHandle {
    pub fn new(conn: (SocketAddrV4, SocketAddrV4)) -> Self {
        Self {
            inner: Rc::new(StatsHandleInner {
                conn,
                stat: Cell::new(Stat::Disabled),
                stat_to_update: Cell::new(None),
                bucket_list_position: Cell::new(None),
                handles_index: Cell::new(None),
                stat_right_before_disable: Cell::new(None),
            })
        }
    }

    #[inline]
    pub fn set(&self, val: usize) {
        let inner = &self.inner;
        if let Stat::Enabled(..) = inner.stat.get() {
            inner.stat_to_update.set(Some(val));
        }
        // capy_log_mig!("Setting stat val={val}: {:#?}", self);
    }

    #[inline]
    pub fn increment(&self) {
        let inner = &self.inner;
        if let Stat::Enabled(val) = inner.stat.get() {
            match inner.stat_to_update.get() {
                Some(val) => inner.stat_to_update.set(Some(val + 1)),
                None => inner.stat_to_update.set(Some(val + 1)),
            }
        }
        capy_log!("increment {:?} to {:?}", inner.conn, inner.stat_to_update.get());
    }

    /// Adds handle to list of polled stats handles.
    /// If `use_old_value` is true, old value is used if present.
    /// Otherwise `initial_val` is used.
    pub fn enable(&self, stats: &mut Stats, use_old_value: bool, initial_val: usize) {
        capy_log!("Enable stats for {:?}", self.inner.conn);

        let old_stat = self.inner.stat.get();

        let initial_val = if use_old_value {
            self.inner.stat_right_before_disable.take().unwrap_or(initial_val)
        } else { initial_val };

        if let Stat::Enabled(..) = old_stat {
            panic!("Enabling already enabled stat");
        }

        self.inner.stat.set(Stat::Enabled(initial_val));

        // Update global stat.
        stats.global_stat += initial_val;

        // If stat was only marked for disable, it hasn't been removed from the BucketList. No need to add another entry.
        if let Stat::Disabled = old_stat {
            // Add to handles.
            self.inner.handles_index.set(Some(stats.handles.len()));
            stats.handles.push(self.clone());

            // Add to buckets.
            stats.buckets.insert(self, initial_val);
        }
    }

    /// Marks handle for removal from stats.
    pub fn disable(&self) {
        if let Stat::Enabled(val) = self.inner.stat.get() {
            capy_log!("Mark stats disabled for {:?}", self.inner.conn);
            self.inner.stat.set(Stat::MarkedForDisable(val));
            self.inner.handles_index.set(None);
            self.inner.stat_right_before_disable.set(Some(val));
        }
    }
}

impl Stat {
    fn expect_enabled(self, msg: &str) -> usize {
        if let Stat::Enabled(val) = self {
            return val;
        }
        panic!("Stat::expect_enabled() failed with `{}`", msg);
    }
}

impl RollingAverage {
    fn new() -> Self {
        Self {
            values: {
                let mut queue = VecDeque::with_capacity(ROLLING_AVG_WINDOW);
                queue.resize(ROLLING_AVG_WINDOW, 0);
                queue
            },
            sum: 0,
        }
    }

    #[inline]
    fn update(&mut self, value: usize) {
        self.sum -= unsafe { self.values.pop_front().unwrap_unchecked() };
        self.sum += value;
        self.values.push_back(value);
    }

    #[inline]
    fn get(&self) -> usize {
        self.sum >> ROLLING_AVG_WINDOW_LOG2
    }

    fn force_set(&mut self, value: usize) {
        for e in &mut self.values {
            *e = value;
        }
        self.sum = value << ROLLING_AVG_WINDOW_LOG2;
    }
}