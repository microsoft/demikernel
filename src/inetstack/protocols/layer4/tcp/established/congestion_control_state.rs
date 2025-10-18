use crate::{
    inetstack::protocols::layer4::tcp::established::congestion_control,
    inetstack::protocols::layer4::tcp::established::rto::RtoCalculator,
};

/// Congestion Control Parameters for TCP connection state
/// This struct has only public members because it includes state that must be accessed
/// by the other TCP modules.
pub struct CongestionControlState {
    #[allow(dead_code)]
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
}
