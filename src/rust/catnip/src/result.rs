use crate::fail::Fail;
use std::result;

pub type StdResult<T, U> = result::Result<T, U>;
pub type Result<T> = StdResult<T, Fail>;
