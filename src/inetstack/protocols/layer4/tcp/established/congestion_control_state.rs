use std::time::Instant;

use crate::inetstack::protocols::layer4::tcp::established::{congestion_control, rto::RtoCalculator};

/// Congestion Control Parameters for TCP connection state
/// This struct has only public members because it includes state that must be accessed
/// by the other TCP modules.
pub struct CongestionControlState {
    #[allow(dead_code)]
    // Retransmission Timeout (RTO) calculator.
    pub rto_calculator: RtoCalculator,
    pub cc_algorithm: Box<dyn congestion_control::CongestionControl>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CongestionControlState {
    pub fn new(congestion_control_algorithm: Box<dyn congestion_control::CongestionControl>) -> Self {
        Self {
            rto_calculator: RtoCalculator::new(),
            cc_algorithm: congestion_control_algorithm,
        }
    }

    pub fn add_sample(&mut self, segment_initial_tx: Option<Instant>, now: Instant) {
        // Add sample for RTO if we have an initial transmit time.
        // Note that in the case of repacketization, an ack for the first byte is enough for the time sample because it still represents the RTO for that single byte.
        // TODO: TCP timestamp support.
        if let Some(initial_tx) = segment_initial_tx {
            self.rto_calculator.add_sample(now - initial_tx);
        }
    }
}
