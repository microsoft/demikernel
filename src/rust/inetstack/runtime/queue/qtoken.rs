// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Structures
//==============================================================================

/// Queue Token
///
/// This is used to uniquely identify operations on IO queues.
#[derive(Clone, Display, Copy, Debug, Eq, PartialEq, From, Into, Hash)]
pub struct QToken(u64);
