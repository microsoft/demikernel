// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::std::env;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Names of LibOSes.
pub enum LibOSName {
    Catpowder,
    Catnap,
    CatnapW,
    Catcollar,
    Catnip,
    Catmem,
    Catloop,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for LibOSName.
impl LibOSName {
    pub fn from_env() -> Result<Self, Fail> {
        match env::var("LIBOS") {
            Ok(name) => Ok(name.into()),
            Err(_) => Err(Fail::new(libc::EINVAL, "missing value for LIBOS environment variable")),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Conversion trait implementation for LibOSName.
impl From<String> for LibOSName {
    fn from(str: String) -> Self {
        match str.to_lowercase().as_str() {
            "catpowder" => LibOSName::Catpowder,
            "catnap" => LibOSName::Catnap,
            "catnapw" => LibOSName::CatnapW,
            "catcollar" => LibOSName::Catcollar,
            "catnip" => LibOSName::Catnip,
            "catmem" => LibOSName::Catmem,
            "catloop" => LibOSName::Catloop,
            _ => panic!("unkown libos"),
        }
    }
}
