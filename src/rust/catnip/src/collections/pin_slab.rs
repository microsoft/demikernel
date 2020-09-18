// See comment at the top of `async_slab.rs` for why this isn't used.
// Adapted from `unicycle`'s `PinSlab` (which itself is adapted from `slab`)
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::convert::From;

#[derive(Copy, Clone, Debug)]
pub struct SlabKey(usize);

impl From<SlabKey> for usize {
    fn from(s: SlabKey) -> usize {
        s.0
    }
}

impl From<usize> for SlabKey {
    fn from(n: usize) -> SlabKey {
        SlabKey(n)
    }
}

enum Entry<T> {
    // Invariant: Only a suffix of entries in a page are set to `None`. For entry `i` in a page,
    // this value is equivalent to `Free(i + 1)`.
    None,
    Vacant { next_free: SlabKey },
    Occupied(T),
}

pub struct PinSlab<T, const PAGE_SIZE: usize> {
    // Note that our pages are constant size. Since we're not moving existing entries on allocation,
    // we don't need to amortize anything.
    pages: Vec<Box<[Entry<T>; PAGE_SIZE]>>,

    // How many `Entry::Occupied`s are there?
    len: usize,

    // This and the pointer in `Entry::Vacant` are allowed to be out of bounds by one to indicate
    // that there isn't a next free entry.
    first_free: SlabKey,
}

impl<T, const PAGE_SIZE: usize> PinSlab<T, PAGE_SIZE> {
    pub fn new() -> Self {
        Self {
            pages: Vec::new(),
            len: 0,
            first_free: SlabKey(0),
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        if n == 0 {
            return Self::new();
        }
        let num_pages = (n - 1) / PAGE_SIZE + 1;
        let mut pages = Vec::with_capacity(num_pages);
        for _ in 0..num_pages {
            pages.push(Self::empty_page());
        }
        Self {
            pages,
            len: 0,
            first_free: SlabKey(0),
        }

    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get_pin_mut(&mut self, key: SlabKey) -> Option<Pin<&mut T>> {
        let (page_ix, entry_ix) = (key.0 / PAGE_SIZE, key.0 % PAGE_SIZE);
        match self.pages.get_mut(page_ix)?[entry_ix] {
            Entry::Occupied(ref mut value) => Some(unsafe { Pin::new_unchecked(value) }),
            _ => None,
        }
    }

    pub fn get(&self, key: SlabKey) -> Option<&T> {
        let (page_ix, entry_ix) = (key.0 / PAGE_SIZE, key.0 % PAGE_SIZE);
        match self.pages.get(page_ix)?[entry_ix] {
            Entry::Occupied(ref value) => Some(value),
            _ => None,
        }
    }

    pub fn alloc(&mut self, value: T) -> SlabKey {
        // Allocate a new page if needed.
        let pages_len = self.pages.len();

        if self.first_free.0 >= pages_len * PAGE_SIZE {
            self.pages.push(Self::empty_page());
            self.first_free = SlabKey(pages_len * PAGE_SIZE);
        }

        let key = self.first_free;
        let (page_ix, entry_ix) = (key.0 / PAGE_SIZE, key.0 % PAGE_SIZE);

        let entry = &mut self.pages[page_ix][entry_ix];
        let next_free = match entry {
            Entry::None => SlabKey(self.first_free.0 + 1),
            Entry::Vacant { next_free } => *next_free,
            Entry::Occupied(..) => panic!("First free pointed to occupied slot: {:?}", self.first_free),
        };
        self.first_free = next_free;
        *entry = Entry::Occupied(value);

        key
    }

    pub fn free(&mut self, key: SlabKey) -> bool {
        let (page_ix, entry_ix) = (key.0 / PAGE_SIZE, key.0 % PAGE_SIZE);

        let entry = &mut self.pages[page_ix][entry_ix];
        match entry {
            Entry::Occupied(..) => (),
            Entry::None | Entry::Vacant { .. } => return false,
        }
        *entry = Entry::Vacant { next_free: self.first_free };
        self.first_free = key;

        true
    }

    fn empty_page() -> Box<[Entry<T>; PAGE_SIZE]> {
        unsafe {
            let mut page_box: Box<MaybeUninit<[Entry<T>; PAGE_SIZE]>> = Box::new_uninit();
            let entry_ptr = page_box.as_mut_ptr() as *mut Entry<T>;
            for i in 0..PAGE_SIZE {
                entry_ptr.offset(i as isize).write(Entry::None);
            }
            page_box.assume_init()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PinSlab;

    use std::ptr;
    use std::rc::Rc;
    use std::cell::RefCell;

    #[test]
    fn test_drop_zst() {
        struct Zst;
        static mut ZST_PTR: *mut Zst = ptr::null_mut();
        impl Drop for Zst {
            fn drop(&mut self) {
                unsafe {
                    ZST_PTR = self as *mut _;
                }
            }
        }

        let mut p = PinSlab::<_, 16>::new();
        let key = p.alloc(Zst);
        let pinned_ptr = p.get_pin_mut(key).unwrap().get_mut() as *mut _;

        drop(p);
        assert_eq!(unsafe { ZST_PTR }, pinned_ptr);
    }

    #[test]
    fn test_drop_nzst() {
        let addrs_at_drop = Rc::new(RefCell::new(vec![]));

        struct DropLogger(Rc<RefCell<Vec<*mut DropLogger>>>);
        impl Drop for DropLogger {
            fn drop(&mut self) {
                let ptr = self as *mut _;
                let mut addrs = self.0.borrow_mut();
                addrs.push(ptr);
            }
        }

        let mut p = PinSlab::<_, 2>::new();

        let mut keys = vec![];
        let mut addrs_at_alloc = vec![];
        for _ in 0..5 {
            let key = p.alloc(DropLogger(addrs_at_drop.clone()));
            addrs_at_alloc.push(p.get_pin_mut(key).unwrap().get_mut() as *mut _);
            keys.push(key);
        }
        let mut addrs_at_end = vec![];
        for key in keys {
            addrs_at_end.push(p.get_pin_mut(key).unwrap().get_mut() as *mut _);
        }

        drop(p);
        assert_eq!(addrs_at_alloc, addrs_at_end);
        assert_eq!(addrs_at_end, *addrs_at_drop.borrow());
    }

    #[test]
    fn test_get() {
        let mut p = PinSlab::<_, 16>::new();

        let mut keys = vec![];
        for i in 0..512 {
            let key = p.alloc(i);
            keys.push((key, i));
        }
        for &(key, i) in &keys {
            assert_eq!(p.get(key), Some(&i));
        }

        // Free half of the keys and check our gets again.
        let mut remaining_keys = vec![];
        for (key, i) in keys {
            if i % 2 == 0 {
                remaining_keys.push((key, i));
            } else {
                p.free(key);
            }
        }

        for (key, i) in remaining_keys {
            assert_eq!(p.get(key), Some(&i));
        }
    }
}
