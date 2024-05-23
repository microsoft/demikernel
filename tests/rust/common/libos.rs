// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::common::runtime::SharedDummyRuntime;
use ::crossbeam_channel::{
    Receiver,
    Sender,
};
use ::demikernel::{
    demi_sgarray_t,
    demikernel::{
        config::Config,
        libos::network::libos::SharedNetworkLibOS,
    },
    inetstack::SharedInetStack,
    runtime::{
        fail::Fail,
        logging,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        QDesc,
        QToken,
        SharedDemiRuntime,
    },
    OperationResult,
};
use ::std::{
    ops::{
        Deref,
        DerefMut,
    },
    time::{
        Duration,
        Instant,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A default amount of time to wait on an operation to complete. This was chosen arbitrarily to be quite small to make
/// timeouts fast.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1);

pub struct DummyLibOS(SharedNetworkLibOS<SharedInetStack<SharedDummyRuntime>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl DummyLibOS {
    /// Initializes the libOS.
    pub fn new_test(config_path: &str, tx: Sender<DemiBuffer>, rx: Receiver<DemiBuffer>) -> Result<Self, Fail> {
        let config: Config = Config::new(config_path.to_string())?;
        let runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        let network: SharedDummyRuntime = SharedDummyRuntime::new(rx, tx);

        logging::initialize();
        let transport = SharedInetStack::new_test(&config, runtime.clone(), network)?;
        Ok(Self(SharedNetworkLibOS::<SharedInetStack<SharedDummyRuntime>>::new(
            config.local_ipv4_addr()?,
            runtime,
            transport,
        )))
    }

    /// Cooks a buffer.
    pub fn cook_data(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let fill_char: u8 = b'a';

        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        for a in &mut buf[..] {
            *a = fill_char;
        }
        let data: demi_sgarray_t = self.get_transport().into_sgarray(buf)?;
        Ok(data)
    }

    #[allow(dead_code)]
    pub fn wait(&mut self, qt: QToken, timeout: Option<Duration>) -> Result<(QDesc, OperationResult), Fail> {
        // First check if the task has already completed.
        if let Some(result) = self.get_runtime().get_completed_task(&qt) {
            return Ok(result);
        }

        // Otherwise, actually run the scheduler.
        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];
        let mut prev: Instant = Instant::now();
        let mut remaining_time: Duration = timeout.unwrap_or(DEFAULT_TIMEOUT);

        // Call run_any() until the task finishes.
        loop {
            // Run for one quanta and if one of our queue tokens completed, then return.
            if let Some((offset, qd, qr)) = self.get_runtime().run_any(&qt_array, remaining_time) {
                debug_assert_eq!(offset, 0);
                return Ok((qd, qr));
            }
            let now: Instant = Instant::now();
            let elapsed_time: Duration = now - prev;
            if elapsed_time >= remaining_time {
                break;
            } else {
                remaining_time = remaining_time - elapsed_time;
                prev = now;
            }
        }
        Err(Fail::new(libc::ETIMEDOUT, "wait timed out"))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for DummyLibOS {
    type Target = SharedNetworkLibOS<SharedInetStack<SharedDummyRuntime>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DummyLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
