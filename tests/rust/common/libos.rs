// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::common::runtime::SharedDummyRuntime;
use ::crossbeam_channel::{Receiver, Sender};
use ::demikernel::{
    demi_sgarray_t,
    demikernel::{config::Config, libos::network::libos::SharedNetworkLibOS},
    inetstack::{consts::MAX_HEADER_SIZE, SharedInetStack},
    runtime::{
        fail::Fail,
        logging,
        memory::{into_sgarray, DemiBuffer},
        QDesc, QToken, SharedDemiRuntime,
    },
    OperationResult,
};
use ::std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================
pub struct DummyLibOS(SharedNetworkLibOS<SharedInetStack>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl DummyLibOS {
    /// Initializes the libOS.
    pub fn new(config_path: &str, tx: Sender<DemiBuffer>, rx: Receiver<DemiBuffer>) -> Result<Self, Fail> {
        let config: Config = Config::new(config_path.to_string())?;
        let runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        let network: SharedDummyRuntime = SharedDummyRuntime::new(rx, tx);

        logging::initialize();
        let transport = SharedInetStack::new(&config, runtime.clone(), network)?;
        Ok(Self(SharedNetworkLibOS::<SharedInetStack>::new(runtime, transport)))
    }

    pub fn prepare_dummy_buffer(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let fill_char: u8 = b'a';

        let mut buf: DemiBuffer = DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16);
        for a in &mut buf[..] {
            *a = fill_char;
        }
        let data: demi_sgarray_t = into_sgarray(buf)?;
        Ok(data)
    }

    pub fn wait(&mut self, qt: QToken, timeout: Duration) -> Result<(QDesc, OperationResult), Fail> {
        self.get_runtime().wait(qt, timeout)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for DummyLibOS {
    type Target = SharedNetworkLibOS<SharedInetStack>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DummyLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
