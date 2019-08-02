use std::{ops::Index, slice::Iter, vec::IntoIter};

#[derive(Clone, Debug, Default)]
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

    pub fn iter<'a>(&'a self) -> Iter<'a, Vec<u8>> {
        self.0.iter()
    }

    pub fn iter_bytes(&self) -> IoVecByteIter<'_> {
        IoVecByteIter::new(self)
    }

    pub fn structural_eq(&self, other: Self) -> bool {
        let mut lhs = self.iter();
        let mut rhs = other.iter();
        loop {
            let lhs = lhs.next();
            let rhs = rhs.next();
            if lhs != rhs {
                return false;
            }

            if lhs.is_none() && rhs.is_none() {
                return true;
            }
        }
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

impl PartialEq for IoVec {
    fn eq(&self, other: &Self) -> bool {
        let mut lhs = self.iter_bytes();
        let mut rhs = other.iter_bytes();
        loop {
            let lhs = lhs.next();
            let rhs = rhs.next();
            if lhs != rhs {
                return false;
            }

            if lhs.is_none() && rhs.is_none() {
                return true;
            }
        }
    }
}

impl Eq for IoVec {}

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

impl<'a> IntoIterator for &'a IoVec {
    type IntoIter = Iter<'a, Vec<u8>>;
    type Item = &'a Vec<u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct IoVecByteIter<'a> {
    iovec: &'a IoVec,
    segment: usize,
    offset: usize,
}

impl<'a> IoVecByteIter<'a> {
    pub fn new(iovec: &'a IoVec) -> Self {
        IoVecByteIter {
            iovec,
            segment: 0,
            offset: 0,
        }
    }
}

impl<'a> Iterator for IoVecByteIter<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        while self.segment < self.iovec.0.len() {
            if self.offset < self.iovec.0[self.segment].len() {
                let offset = self.offset;
                self.offset += 1;
                return Some(self.iovec.0[self.segment][offset]);
            }

            self.segment += 1;
            self.offset = 0;
        }

        None
    }
}
