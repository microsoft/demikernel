// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This file defines a type to represent a TCP Sequence Number.
//
// RFC 793, Section 3.3 defines TCP sequence numbers.  The sequence number space ranges from 0 to 2^32 - 1.  This space
// "wraps around", so all arithmetic dealing with sequence numbers must be performed modulo 2^32.  This also means that
// excluding equality, all comparisons between sequence numbers are non-transitive.  That is, for any three distinct
// sequence numbers a, b, & c, having a < b and b < c being true does NOT necessarily imply that a < c.  One can have
// the situation that a < b < c < a.  For this reason, we define sequence numbers to be their own type.
//

use std::{
    cmp::Ordering,
    convert::From,
    fmt,
};

// Internally, we store sequence numbers as unsigned 32-bit integers.
//
// We allow our sequence numbers to be cloned, copied, created, and checked for equality the same as for u32.  We
// restrict all other behaviors to those we explicitly define below.
//
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SeqNumber {
    value: u32,
}

// To create a u32 from a sequence number.
impl From<SeqNumber> for u32 {
    #[inline]
    fn from(item: SeqNumber) -> u32 {
        item.value
    }
}

// To create a sequence number from a u32.
impl From<u32> for SeqNumber {
    #[inline]
    fn from(item: u32) -> Self {
        SeqNumber { value: item }
    }
}

// Display a sequence number.
impl std::fmt::Display for SeqNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}

// Add two sequence numbers together.
impl std::ops::Add for SeqNumber {
    type Output = SeqNumber;

    #[inline]
    fn add(self, other: SeqNumber) -> SeqNumber {
        (self.value.wrapping_add(other.value)).into()
    }
}

// Subtract a sequence number from another one.
impl std::ops::Sub for SeqNumber {
    type Output = SeqNumber;

    #[inline]
    fn sub(self, other: SeqNumber) -> SeqNumber {
        (self.value.wrapping_sub(other.value)).into()
    }
}

// Note that we don't define std::ops::Mul or std::ops::Div as these aren't needed for standard sequence number usage.
// Likewise we don't define any bitwise or shift operations on sequence numbers.
//
// We define the PartialOrd trait in order to support the "<", "<=", ">", and ">=" operators on sequence numbers.
// Strictly speaking, however, sequence numbers are not a partially ordered set (much less a totally ordered set) due to
// the fact that they wrap around.  So to avoid problems with other code that might assume our implementation of the
// PartialOrd trait means that sequence numbers can be uniquely ordered, we don't implement the partial_cmp function of
// this trait.  Well, actually we do, because the compiler complains if we don't, but we have it panic if it is called.
impl std::cmp::PartialOrd for SeqNumber {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        panic!("Somebody called partial_cmp on a sequence number!  Don't do that!");
        // (self.value.wrapping_sub(other.value) as i32).partial_cmp(&0)
    }

    #[inline]
    fn lt(&self, other: &Self) -> bool {
        (self.value.wrapping_sub(other.value) as i32) < 0
    }

    #[inline]
    fn le(&self, other: &Self) -> bool {
        (self.value.wrapping_sub(other.value) as i32) <= 0
    }

    #[inline]
    fn gt(&self, other: &Self) -> bool {
        (self.value.wrapping_sub(other.value) as i32) > 0
    }

    #[inline]
    fn ge(&self, other: &Self) -> bool {
        (self.value.wrapping_sub(other.value) as i32) >= 0
    }
}

// Note that we specifically don't define std::cmp:Ord for sequence numbers, as there is no total order for them.
// There is no max or min value, and if you have more than two of them, they can't be sorted into an unique order.

// Unit tests for SeqNumber type.
#[cfg(test)]
mod tests {
    use super::SeqNumber;
    use ::anyhow::Result;

    // Test basic comparisons between sequence numbers of various values.
    #[test]
    fn comparison() -> Result<()> {
        let s0: SeqNumber = SeqNumber::from(0);
        let s1: SeqNumber = SeqNumber::from(1);
        let s2: SeqNumber = SeqNumber::from(0x20000000);
        let s3: SeqNumber = SeqNumber::from(0x3fffffff);
        let s4: SeqNumber = SeqNumber::from(0x7fffffff);
        let s5: SeqNumber = SeqNumber::from(0x80000000);
        let s6: SeqNumber = SeqNumber::from(0x80000001);
        let s7: SeqNumber = SeqNumber::from(0xffffffff);

        crate::ensure_eq!(s0, s0);
        crate::ensure_neq!(s0, s1);
        crate::ensure_neq!(s0, s1);
        crate::ensure_neq!(s0, s7);

        crate::ensure_eq!(!(s0 < s0), true);
        crate::ensure_eq!(!(s0 > s0), true);

        crate::ensure_eq!(s0 < s1, true);
        crate::ensure_eq!(s0 < s2, true);
        crate::ensure_eq!(s0 < s3, true);
        crate::ensure_eq!(s0 < s4, true);
        crate::ensure_eq!(s0 < s5, true);
        crate::ensure_eq!(s0 > s6, true);
        crate::ensure_eq!(s0 > s7, true);

        Ok(())
    }

    // Test that basic comparisons (and addition) handle wrap around properly.
    #[test]
    fn wrap_around() -> Result<()> {
        let zero = SeqNumber::from(0);
        let one = SeqNumber::from(1);
        let big = SeqNumber::from(0xffffffff);

        let something = big + one;

        crate::ensure_neq!(zero, big);
        crate::ensure_eq!(something, zero);

        let half = SeqNumber::from(0x80000000);

        for number in 0..((2 ^ 32) - 1) {
            let current = SeqNumber::from(number);
            let next = current + one;
            crate::ensure_eq!(current < next, true);

            crate::ensure_eq!(current < current + half, true);
            crate::ensure_eq!(current > next + half, true);
        }

        Ok(())
    }
}
