use std::{ops::Index, vec::IntoIter};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IoVec(Vec<Vec<u8>>);

impl IoVec {
    pub fn new() -> IoVec {
        IoVec::default()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn byte_count(&self) -> usize {
        self.0.iter().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, segment: Vec<u8>) {
        self.0.push(segment);
    }
}

impl From<Vec<Vec<u8>>> for IoVec {
    fn from(bytes: Vec<Vec<u8>>) -> IoVec {
        IoVec(bytes)
    }
}

impl From<Vec<u8>> for IoVec {
    fn from(bytes: Vec<u8>) -> IoVec {
        IoVec::from(vec![bytes])
    }
}

impl Index<usize> for IoVec {
    type Output = [u8];

    fn index(&self, n: usize) -> &Self::Output {
        self.0.index(n)
    }
}

impl IntoIterator for IoVec {
    type IntoIter = IntoIter<Vec<u8>>;
    type Item = Vec<u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
