// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::mbuf::Mbuf;
use ::runtime::memory::{
    Buffer,
    Bytes,
};
use ::std::ops::Deref;
use serde::{Serialize, Deserialize};
use std::fmt::Formatter;
use serde::{Deserializer, Serializer};
use serde::de::{self, EnumAccess, Error, MapAccess, SeqAccess, Visitor};

//==============================================================================
// Enumerations
//==============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DPDKBuf {
    External(Bytes),
    Managed(Mbuf),
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Deref for DPDKBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            DPDKBuf::External(ref buf) => buf.deref(),
            DPDKBuf::Managed(ref mbuf) => mbuf.deref(),
        }
    }
}

impl Serialize for DPDKBuf {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
        serializer.serialize_bytes(self.deref())
    }
}
struct ByteSliceVisitor;

impl<'de> Visitor<'de> for ByteSliceVisitor {
    type Value = DPDKBuf;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("Expecting a byte slice only!")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E> where E: Error {
        Ok(DPDKBuf::External(Bytes::from_slice(v)))
    }
}

impl<'de> Deserialize<'de> for DPDKBuf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
        deserializer.deserialize_bytes(ByteSliceVisitor)
    }
}

impl Buffer for DPDKBuf {
    fn empty() -> Self {
        DPDKBuf::External(Bytes::empty())
    }

    fn from_slice(_: &[u8]) -> Self {
        todo!()
    }

    fn adjust(&mut self, num_bytes: usize) {
        match self {
            DPDKBuf::External(ref mut buf) => buf.adjust(num_bytes),
            DPDKBuf::Managed(ref mut mbuf) => mbuf.adjust(num_bytes),
        }
    }

    fn trim(&mut self, num_bytes: usize) {
        match self {
            DPDKBuf::External(ref mut buf) => buf.trim(num_bytes),
            DPDKBuf::Managed(ref mut mbuf) => mbuf.trim(num_bytes),
        }
    }
}
