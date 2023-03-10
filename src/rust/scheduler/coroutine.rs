// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::std::{
    any::Any,
    future::Future,
};

//==============================================================================
// Traits
//==============================================================================

/// Coroutine
///
/// This abstraction is the basic unit of application work for Demikernel. Demikernel libOSes use coroutines to handle
/// processing for different events (e.g. packet arriving, pushing and popping from a queue, etc.).
pub trait Coroutine: Any + Future<Output = ()> + Unpin {
    /// Casts the target [Coroutine] into [Any].
    fn as_any(self: Box<Self>) -> Box<dyn Any>;

    /// Gets the underlying function to run the coroutine.
    fn get_coroutine(&self) -> &dyn Future<Output = ()>;
}
