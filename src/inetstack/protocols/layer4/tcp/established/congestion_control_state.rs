#![allow(dead_code)]
use crate::{
    inetstack::protocols::layer4::tcp::established::congestion_control,
    inetstack::protocols::layer4::tcp::established::rto::RtoCalculator,
};

/// Congestion Control Parameters for TCP connection state
/// This struct has only public members because it includes state that must be accessed
/// by the other TCP modules.
pub struct CongestionControlState {
    pub rto_calculator: RtoCalculator,
    pub congestion_control_algorithm: Box<dyn congestion_control::CongestionControl>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CongestionControlState {
    pub fn new(
        rto_calculator: RtoCalculator,
        congestion_control_algorithm: Box<dyn congestion_control::CongestionControl>,
    ) -> Self {
        Self {
            rto_calculator,
            congestion_control_algorithm,
        }
    }
}
