//======================================================================================================================
// Imports
//======================================================================================================================

use super::types::{demi_metric_callback_t, demi_metric_kind_t};
use std::cell::OnceCell;

pub struct Metric {
    id: u32,
    name: &'static str,
    kind: demi_metric_kind_t,
    description: &'static str,
    unit: &'static str,
}

struct StaticWrapper<T>(OnceCell<T>);

// NB this is only safe when we enforce the user calling all APIs from the same thread.
unsafe impl<T> Send for StaticWrapper<T> {}
unsafe impl<T> Sync for StaticWrapper<T> {}

//======================================================================================================================
// Macro Rules
//======================================================================================================================

macro_rules! define_metrics2 {
    ($id:expr, metric ($name:ident, $kind:expr, $description:expr, $unit:expr), $(metric ($name2:ident, $kind2:expr, $description2:expr, $unit2:expr)),* $(,)?) => {
        #[allow(non_upper_case_globals)]
        const $name: Metric = Metric::new($id, stringify!($name), $kind, $description, $unit);
        define_metrics2!(($id + 1), $(metric ($name2, $kind2, $description2, $unit2)),*);
    };
    ($id:expr, metric ($name:ident, $kind:expr, $description:expr, $unit:expr) $(,)?) => {
        #[allow(non_upper_case_globals)]
        const $name: Metric = Metric::new($id, stringify!($name), $kind, $description, $unit);
    };
}

macro_rules! define_metrics {
    ($(metric ($name:ident, $kind:expr, $description:expr, $unit:expr)),* $(,)?) => {
        pub struct Metrics {
            $(pub $name: Metric,)*
        }

        impl Metrics {
            const fn new() -> Self {
                define_metrics2!(0, $(metric ($name, $kind, $description, $unit)),*);
                Self {
                    $($name),*
                }
            }

            fn accessors() -> &'static [fn(&Metrics) -> &Metric] {
                static ACCESSOR_ARRAY: &[fn(&Metrics) -> &Metric] = &[
                    $(
                        |s: &Metrics| &s.$name,
                    )*
                ];
                ACCESSOR_ARRAY
            }
        }
    };
}

//======================================================================================================================
// Static Variables
//======================================================================================================================

define_metrics! {
    metric(tcp_retransmits, demi_metric_kind_t::DEMI_MK_EVENT, "An event triggered each time a TCP retransmit occurs", "event"),
    metric(tcp_out_of_order_frames, demi_metric_kind_t::DEMI_MK_SAMPLE, "The number of TCP out-of-order frames", "packet"),
    metric(tcp_unacked_frames, demi_metric_kind_t::DEMI_MK_SAMPLE, "The number of TCP which have not been ACK'd", "packet"),
    metric(tcp_unset_frames, demi_metric_kind_t::DEMI_MK_SAMPLE, "The number of TCP frames waiting to be sent", "packet"),
    metric(tcp_rto, demi_metric_kind_t::DEMI_MK_SAMPLE, "The TCP retransmission timeout", "second"),
}

static TRACE_CALLBACK: StaticWrapper<demi_metric_callback_t> = StaticWrapper(OnceCell::new());
pub const METRICS: Metrics = Metrics::new();

impl Metrics {
    pub fn len(&self) -> usize {
        Self::accessors().len()
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a Metric> {
        Self::accessors().iter().map(|f: &fn(&Metrics) -> &Metric| f(self))
    }
}

impl Metric {
    const fn new(
        id: u32,
        name: &'static str,
        kind: demi_metric_kind_t,
        description: &'static str,
        unit: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            description,
            unit,
        }
    }

    pub const fn id(&self) -> u32 {
        self.id
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn kind(&self) -> demi_metric_kind_t {
        self.kind
    }

    pub const fn description(&self) -> &'static str {
        self.description
    }

    pub const fn unit(&self) -> &'static str {
        self.unit
    }

    pub fn emit(&self, value: u32) {
        if let Some(cb) = TRACE_CALLBACK.0.get() {
            cb(self.id(), value);
        }
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

pub fn init_trace(callback: demi_metric_callback_t) {
    TRACE_CALLBACK.0.set(callback).unwrap();
}
