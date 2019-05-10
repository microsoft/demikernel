mod partitioned;
mod shared;

use crate::sync::{Mutex, Arc};

pub use partitioned::PartitionedState as Partitioned;
pub use shared::SharedState as Shared;

struct State {
    partitioned: Partitioned,
    shared: Arc<Mutex<Shared>>,
}
