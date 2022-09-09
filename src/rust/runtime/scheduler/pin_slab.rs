// Copyright (c) Microsoft Corporation.
// MIT License

// Copyright (c) 2019 John-John Tedro
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED *AS IS*, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! A slab-like, pre-allocated storage where the slab is divided into immovable
//! slots. Each allocated slot doubles the capacity of the slab.
//!
//! Converted from <https://github.com/carllerche/slab>, this slab however
//! contains a growable collection of fixed-size regions called slots.
//! This allows is to store immovable objects inside the slab, since growing the
//! collection doesn't require the existing slots to move.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::{
    mem,
    pin::Pin,
    ptr,
    ptr::NonNull,
};

//======================================================================================================================
// Constants
//======================================================================================================================

// Size of the first slot.
const FIRST_SLOT_SIZE: usize = 16;
// The initial number of bits to ignore for the first slot.
const FIRST_SLOT_MASK: usize = std::mem::size_of::<usize>() * 8 - FIRST_SLOT_SIZE.leading_zeros() as usize - 1;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Pre-allocated storage for a uniform data type, with slots of immovable
/// memory regions.
#[derive(Clone)]
pub struct PinSlab<T> {
    // Slots of memory. Once one has been allocated it is never moved.
    // This allows us to store entries in there and fetch them as `Pin<&mut T>`.
    slots: Vec<ptr::NonNull<Entry<T>>>,
    // Number of Filled elements currently in the slab
    len: usize,
    // Offset of the next available slot in the slab.
    next: usize,
}

//======================================================================================================================
// Enumerations
//======================================================================================================================

enum Entry<T> {
    // Each slot is pre-allocated with entries of `None`.
    None,
    // Removed entries are replaced with the vacant tomb stone, pointing to the
    // next vacant entry.
    Vacant(usize),
    // An entry that is occupied with a value.
    Occupied(T),
}

//======================================================================================================================
// Associated Implementations
//======================================================================================================================

impl<T> PinSlab<T> {
    /// Construct a new, empty [PinSlab] with the default slot size.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            next: 0,
            len: 0,
        }
    }

    /// Insert a value into the pin slab.
    pub fn insert(&mut self, val: T) -> Option<usize> {
        let key: usize = self.next;
        self.insert_at(key, val)?;
        Some(key)
    }

    /// Access the given key as a pinned mutable value.
    pub fn get_pin_mut(&mut self, key: usize) -> Option<Pin<&mut T>> {
        // Safety: all storage is pre-allocated in chunks, and each chunk
        // doesn't move. We only provide mutators to drop the storage through
        // `remove` (but it doesn't return it).
        unsafe {
            let entry: &mut T = self.internal_get_mut(key)?;
            Some(Pin::new_unchecked(entry))
        }
    }

    /// Get a reference to the value at the given slot.
    pub fn get(&self, key: usize) -> Option<&T> {
        // Safety: We only use this to acquire an immutable reference.
        // The internal calculation guarantees that the key is in bounds.
        unsafe { self.internal_get(key) }
    }

    /// Get a mutable reference to the value at the given slot.
    #[inline(always)]
    unsafe fn internal_get_mut(&mut self, key: usize) -> Option<&mut T> {
        let (slot, offset, len): (usize, usize, usize) = calculate_key(key)?;
        let slot: NonNull<Entry<T>> = *self.slots.get_mut(slot)?;

        // Safety: all slots are fully allocated and initialized in `new_slot`.
        // As long as we have access to it, we know that we will only find
        // initialized entries assuming offset < len.
        debug_assert!(offset < len);

        let entry: &mut T = match &mut *slot.as_ptr().add(offset) {
            Entry::Occupied(entry) => entry,
            _ => return None,
        };

        Some(entry)
    }

    /// Get a reference to the value at the given slot.
    #[inline(always)]
    unsafe fn internal_get(&self, key: usize) -> Option<&T> {
        let (slot, offset, len): (usize, usize, usize) = calculate_key(key)?;
        let slot: NonNull<Entry<T>> = *self.slots.get(slot)?;

        // Safety: all slots are fully allocated and initialized in `new_slot`.
        // As long as we have access to it, we know that we will only find
        // initialized entries assuming offset < len.
        debug_assert!(offset < len);

        let entry: &T = match &*slot.as_ptr().add(offset) {
            Entry::Occupied(entry) => entry,
            _ => return None,
        };

        Some(entry)
    }

    /// Remove the key from the slab.
    ///
    /// If successful, returns `Some(true)` if the entry was removed, `Some(false)` otherwise.
    /// Removing a key which does not exist has no effect, and `false` will be
    /// returned. On failure, it returns `None`.
    ///
    /// We need to take care that we don't move it, hence we only perform
    /// operations over pointers below.
    pub fn remove(&mut self, key: usize) -> Option<bool> {
        let (slot, offset, len): (usize, usize, usize) = calculate_key(key)?;

        let slot: NonNull<Entry<T>> = match self.slots.get_mut(slot) {
            Some(slot) => *slot,
            None => return Some(false),
        };

        // Safety: all slots are fully allocated and initialized in `new_slot`.
        // As long as we have access to it, we know that we will only find
        // initialized entries assuming offset < len.
        debug_assert!(offset < len);
        unsafe {
            let entry: *mut Entry<T> = slot.as_ptr().add(offset);

            match &*entry {
                Entry::Occupied(..) => (),
                _ => return Some(false),
            }

            ptr::drop_in_place(entry);
            ptr::write(entry, Entry::Vacant(self.next));
            self.len -= 1;
            self.next = key;
        }

        Some(true)
    }

    /// Method to take out an `Unpin` value
    pub fn remove_unpin(&mut self, key: usize) -> Option<T>
    where
        T: Unpin,
    {
        let (slot, offset, len): (usize, usize, usize) = calculate_key(key)?;
        let slot: NonNull<Entry<T>> = match self.slots.get_mut(slot) {
            Some(slot) => *slot,
            None => return None,
        };
        debug_assert!(offset < len);
        unsafe {
            let entry: *mut Entry<T> = slot.as_ptr().add(offset);

            match &*entry {
                Entry::Occupied(..) => (),
                _ => return None,
            }
            let value = match mem::replace(&mut *entry, Entry::Vacant(self.next)) {
                Entry::Occupied(v) => v,
                _ => panic!("Entried changed to vacant?"),
            };
            self.len -= 1;
            self.next = key;
            Some(value)
        }
    }

    /// Clear all available data in the PinSlot.
    pub fn clear(&mut self) {
        for (len, entry) in slot_sizes().zip(self.slots.iter_mut()) {
            // reconstruct the vector for the slot.
            drop(unsafe { Vec::from_raw_parts(entry.as_ptr(), len, len) });
        }

        unsafe {
            self.slots.set_len(0);
        }
    }

    /// Construct a new slot.
    fn new_slot(&self, len: usize) -> ptr::NonNull<Entry<T>> {
        let mut d = Vec::with_capacity(len);

        for _ in 0..len {
            d.push(Entry::None);
        }

        let ptr: *mut Entry<T> = d.as_mut_ptr();
        mem::forget(d);

        // Safety: We just initialized the pointer to be non-null above.
        unsafe { ptr::NonNull::new_unchecked(ptr) }
    }

    /// Insert a value at the given slot.
    fn insert_at(&mut self, key: usize, val: T) -> Option<()> {
        let (slot, offset, len): (usize, usize, usize) = calculate_key(key)?;

        if let Some(slot) = self.slots.get_mut(slot) {
            // Safety: all slots are fully allocated and initialized in
            // `new_slot`. As long as we have access to it, we know that we will
            // only find initialized entries assuming offset < slot_size.
            // We also know it's safe to have unique access to _any_ slots,
            // since we have unique access to the slab in this function.
            debug_assert!(offset < len);
            let entry: &mut Entry<T> = unsafe { &mut *slot.as_ptr().add(offset) };

            self.next = match *entry {
                Entry::None => key + 1,
                Entry::Vacant(next) => next,
                // NB: unreachable because insert_at is an internal function,
                // which can only be appropriately called on non-occupied
                // entries. This is however, not a safety concern.
                _ => unreachable!(),
            };

            *entry = Entry::Occupied(val);
        } else {
            unsafe {
                let slot: NonNull<Entry<T>> = self.new_slot(len);
                *slot.as_ptr() = Entry::Occupied(val);
                self.slots.push(slot);
                self.next = key + 1;
            }
        }

        self.len += 1;

        Some(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

unsafe impl<T> Send for PinSlab<T> {}
unsafe impl<T> Sync for PinSlab<T> {}

impl<T> Default for PinSlab<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for PinSlab<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Calculate the key as a (slot, offset, len) tuple.
fn calculate_key(key: usize) -> Option<(usize, usize, usize)> {
    // Check arguments.
    if key >= (1usize << (mem::size_of::<usize>() * 8 - 1)) {
        return None;
    }

    let slot: usize =
        ((mem::size_of::<usize>() * 8) as usize - key.leading_zeros() as usize).saturating_sub(FIRST_SLOT_MASK);

    let (start, end): (usize, usize) = if key < FIRST_SLOT_SIZE {
        (0, FIRST_SLOT_SIZE)
    } else {
        (FIRST_SLOT_SIZE << (slot - 1), FIRST_SLOT_SIZE << slot)
    };

    Some((slot, key - start, end - start))
}

fn slot_sizes() -> impl Iterator<Item = usize> {
    (0usize..).map(|n| match n {
        0 | 1 => FIRST_SLOT_SIZE,
        n => FIRST_SLOT_SIZE << (n - 1),
    })
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use ::std::{
        mem,
        pin::Pin,
    };

    #[test]
    fn slot_sizes() {
        assert_eq!(
            vec![
                super::FIRST_SLOT_SIZE,
                super::FIRST_SLOT_SIZE,
                super::FIRST_SLOT_SIZE << 1,
                super::FIRST_SLOT_SIZE << 2,
                super::FIRST_SLOT_SIZE << 3
            ],
            super::slot_sizes().take(5).collect::<Vec<_>>()
        );
    }

    #[test]
    fn calculate_key_invalid() {
        let invalid_key: usize = 1usize << (mem::size_of::<usize>() * 8 - 1);
        super::calculate_key(invalid_key);
    }

    #[test]
    fn calculate_key_valid() {
        // NB: range of the first slot.
        let expected_key: (usize, usize, usize) = (0, 0, 16);
        let returned_key: (usize, usize, usize) = match super::calculate_key(0) {
            Some(key) => key,
            None => panic!("calculate_key() failed"),
        };
        assert_eq!(returned_key, expected_key);

        let expected_key: (usize, usize, usize) = (0, 15, 16);
        let returned_key: (usize, usize, usize) = match super::calculate_key(15) {
            Some(key) => key,
            None => panic!("calculate_key() failed"),
        };
        assert_eq!(returned_key, expected_key);

        for i in 4..=62 {
            let end_range: usize = 1usize << i;
            let expected_key: (usize, usize, usize) = (i - 3, 0, end_range);
            let returned_key: (usize, usize, usize) = match super::calculate_key(end_range) {
                Some(key) => key,
                None => panic!("calculate_key() failed"),
            };
            assert_eq!(returned_key, expected_key);

            let expected_key: (usize, usize, usize) = (i - 3, end_range - 1, end_range);
            let returned_key: (usize, usize, usize) = match super::calculate_key((1usize << (i + 1)) - 1) {
                Some(key) => key,
                None => panic!("calculate_key() failed"),
            };
            assert_eq!(returned_key, expected_key);
        }
    }

    #[test]
    fn insert_get_remove_many() {
        let mut slab: super::PinSlab<Box<u128>> = super::PinSlab::new();
        let mut keys: Vec<(u128, usize)> = Vec::new();

        for i in 0..1024 {
            let key: usize = match slab.insert(Box::new(i as u128)) {
                Some(key) => key,
                None => panic!("insert() failed"),
            };
            keys.push((i as u128, key));
        }

        for (expected, key) in keys.iter().copied() {
            let value: Pin<&mut Box<u128>> = match slab.get_pin_mut(key) {
                Some(value) => value,
                None => panic!("get_pin_mut() failed"),
            };
            assert_eq!(expected, **value.as_ref());
            let contains: bool = match slab.remove(key) {
                Some(contains) => contains,
                None => panic!("remove() failed"),
            };
            assert!(contains);
        }

        for (_, key) in keys.iter().copied() {
            assert!(slab.get_pin_mut(key).is_none());
        }
    }

    #[test]
    fn remove_unpin() {
        let mut slab: super::PinSlab<i32> = super::PinSlab::new();
        let key: usize = match slab.insert(1) {
            Some(key) => key,
            None => panic!("insert() failed"),
        };
        assert_eq!(slab.remove_unpin(key), Some(1));
    }
}
