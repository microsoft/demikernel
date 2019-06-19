use std::ops::Index;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IoVec(Vec<Vec<u8>>);

impl IoVec {
    pub fn new(bytes: Vec<Vec<u8>>) -> IoVec {
        IoVec(bytes)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<Vec<u8>>> for IoVec {
    fn from(bytes: Vec<Vec<u8>>) -> IoVec {
        IoVec::new(bytes)
    }
}

impl From<Vec<u8>> for IoVec {
    fn from(bytes: Vec<u8>) -> IoVec {
        IoVec::new(vec![bytes])
    }
}

impl Index<usize> for IoVec {
    type Output = [u8];

    fn index(&self, n: usize) -> &Self::Output {
        self.0.index(n)
    }
}
