use once_cell::sync::Lazy;
use std::{default, env};


pub const MAX_RECEIVE_BATCH_SIZE: usize = 200;

pub struct AutokernelParameters {
    pub receive_batch_size: usize,
    pub timer_resolution: usize,
    pub timer_finer_resolution: usize,

    pub rto_alpha: f64,
    pub rto_beta: f64,
    pub rto_granularity: f64,
    pub rto_lower_bound_sec: f64,
    pub rto_upper_bound_sec: f64,

    pub default_body_pool_size: usize,
    pub default_cache_size: usize,
}


// Global, lazily initialized parameters using once_cell::Lazy
pub static AK_PARMS: Lazy<AutokernelParameters> = Lazy::new(|| {
    let receive_batch_size = env::var("RECEIVE_BATCH_SIZE")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(4);
    let timer_resolution = env::var("TIMER_RESOLUTION")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(64);
    let timer_finer_resolution = env::var("TIMER_FINER_RESOLUTION")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(2);

    let rto_alpha = env::var("RTO_ALPHA")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(0.125);
    let rto_beta = env::var("RTO_BETA")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(0.25);
    let rto_granularity = env::var("RTO_GRANULARITY")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(0.001f64);
    let rto_lower_bound_sec = env::var("RTO_LOWER_BOUND_SEC")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(0.100f64);
    let rto_upper_bound_sec = env::var("RTO_UPPER_BOUND_SEC")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(60.0f64);

    let default_body_pool_size = env::var("DEFAULT_BODY_POOL_SIZE")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(8192 - 1);
    let default_cache_size = env::var("DEFAULT_CACHE_SIZE")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(250);


    eprintln!("RECEIVE_BATCH_SIZE: {}", receive_batch_size);
    eprintln!("TIMER_RESOLUTION: {}", timer_resolution);
    eprintln!("TIMER_FINER_RESOLUTION: {}", timer_finer_resolution);
    eprintln!("RTO_ALPHA: {}", rto_alpha);
    eprintln!("RTO_BETA: {}", rto_beta);
    eprintln!("RTO_GRANULARITY: {}", rto_granularity);
    eprintln!("RTO_LOWER_BOUND_SEC: {}", rto_lower_bound_sec);
    eprintln!("RTO_UPPER_BOUND_SEC: {}", rto_upper_bound_sec);
    eprintln!("DEFAULT_BODY_POOL_SIZE: {}", default_body_pool_size);
    eprintln!("DEFAULT_CACHE_SIZE: {}", default_cache_size);



    AutokernelParameters {
        receive_batch_size,
        timer_resolution,
        timer_finer_resolution,
        rto_alpha,
        rto_beta,
        rto_granularity,
        rto_lower_bound_sec,
        rto_upper_bound_sec,
        default_body_pool_size,
        default_cache_size,
    }
});

// // Example function that uses the global parameters
// pub fn print_receive_batch_size() {
//     // Access the global configuration
//     println!(
//         "Receive batch size: {}",
//         AK_PARMS.receive_batch_size
//     );
// }