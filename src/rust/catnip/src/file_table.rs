use slab::Slab;
use std::{
    cell::RefCell,
    rc::Rc,
};

pub type FileDescriptor = u32;

#[derive(Clone)]
pub struct FileTable {
    inner: Rc<RefCell<Inner>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum File {
    TcpSocket,
    UdpSocket,
}

impl FileTable {
    pub fn new() -> Self {
        let inner = Inner { table: Slab::new() };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    pub fn alloc(&self, file: File) -> FileDescriptor {
        let mut inner = self.inner.borrow_mut();
        let ix = inner.table.insert(file);
        let file = ix as u32 + 1;
        file
    }

    pub fn get(&self, fd: FileDescriptor) -> Option<File> {
        let ix = fd as usize - 1;
        let inner = self.inner.borrow();
        inner.table.get(ix).cloned()
    }

    pub fn free(&self, fd: FileDescriptor) -> File {
        let ix = fd as usize - 1;
        let mut inner = self.inner.borrow_mut();
        inner.table.remove(ix)
    }
}

struct Inner {
    table: Slab<File>,
}
