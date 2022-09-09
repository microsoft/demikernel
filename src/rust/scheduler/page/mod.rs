// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod page;
mod page_ref;
mod waker_ref;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    page::{
        WakerPage,
        WAKER_PAGE_SIZE,
    },
    page_ref::WakerPageRef,
    waker_ref::WakerRef,
};
