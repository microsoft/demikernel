// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::{
    marker::PhantomData,
    mem,
    mem::ManuallyDrop,
    ptr::NonNull,
    rc::Rc,
};

// An intrusive singly-linked list (FIFO queue) with owned elements.
#[derive(Debug)]
pub struct IntrusiveQueue<T: IntrusivelyQueueable> {
    // Pointer to the first element in the queue.
    front: Option<NonNull<T>>,
    // Pointer to the last element in the queue.
    back: Option<NonNull<T>>,
    // Length of the queue in elements.
    len: usize,
    // Hint to compiler that this struct "owns" an Rc<T> (for safety determinations).
    phantom: PhantomData<Rc<T>>,
}

impl<T: IntrusivelyQueueable> IntrusiveQueue<T> {
    // Create an empty IntrusiveQueue.
    #[inline]
    pub const fn new() -> Self {
        IntrusiveQueue {
            front: None,
            back: None,
            len: 0,
            phantom: PhantomData,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.front.is_none()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    // Pop the first element off the front of the queue.
    // Call this pop_front to match VecDequeue?
    pub fn pop_element(&mut self) -> Option<Rc<T>> {
        if self.front.is_none() {
            // Nothing on the queue, so return None.
            None
        } else {
            // The queue contains at least one element, so pop it off and return it.

            // Get the first element as an Rc<T>.
            let popped: Rc<T> = unsafe { Rc::from_raw(self.front.unwrap().as_mut()) };

            // Repoint the front pointer at the next element (or None).
            self.front = popped.get_queue_next();

            // Check if the queue is now empty.
            if self.front.is_none() {
                // Clear the back pointer (which should have been pointing at popped).
                self.back = None;
            }

            // Clear the next pointer in the popped element.
            popped.set_queue_next(None);

            // Return the popped value.
            self.len -= 1;
            Some(popped)
        }
    }

    // Add the given element to the back of the queue.
    // Call this push_back to match VecDequeue?
    pub fn push_element(&mut self, added: Rc<T>) {
        // Ensure the new element's next pointer doesn't point to anything.
        added.set_queue_next(None);

        // Convert from an Rc<T> to a raw pointer.
        // Note: Rc::into_raw does NOT decrement the reference count (which is the behavior we want).
        let added: Option<NonNull<T>> = NonNull::new(Rc::into_raw(added) as *mut T);

        if self.front.is_none() {
            // Nothing currently on the queue, so the new element also becomes the front.
            self.front = added;
        } else {
            // Point the current last element's next pointer at the new element.  To do that we first need to reform an
            // Rc for this element in order to get at its IntrusivelyQueueable trait functions.
            // Note: we use ManuallyDrop when reforming the Rc so that we don't drop our original ref on the T when
            // old_back goes out of scope.  The element that was pointed to by old_back is still in the queue.
            let old_back: ManuallyDrop<Rc<T>> =
                unsafe { mem::ManuallyDrop::new(Rc::from_raw(self.back.unwrap().as_mut())) };
            old_back.set_queue_next(added);
        }

        // Repoint the back pointer at the new last element.
        self.back = added;
        self.len += 1;
    }
}

// Drop.
// We need an explicit drop implementation because we hold a Rc reference for each element on the list, and since we
// store the Rcs as raw pointers they won't drop automatically.
impl<T: IntrusivelyQueueable> Drop for IntrusiveQueue<T> {
    fn drop(&mut self) {
        // Pop everything off the queue.
        while self.pop_element().is_some() {}
    }
}

pub trait IntrusivelyQueueable {
    // Returns the next element in the queue.
    fn get_queue_next(&self) -> Option<NonNull<Self>>;

    // Sets the next element in the queue.
    fn set_queue_next(&self, element: Option<NonNull<Self>>);
}

// Unit tests for IntrusiveQueue type and IntrusivelyQueueable trait.
#[cfg(test)]
mod tests {
    use core::cell::Cell;
    use std::{
        ptr::NonNull,
        rc::Rc,
    };

    use super::{
        IntrusiveQueue,
        IntrusivelyQueueable,
    };

    // A test element.
    // This supports the IntrusivelyQueueable trait, so it can be put on an IntrusiveQueue.
    pub struct TestThingy {
        // Support for IntrusivelyQueueable trait.
        next: Cell<Option<NonNull<TestThingy>>>,

        // Some data value.
        pub data: u32,
    }

    impl TestThingy {
        fn new(value: u32) -> Self {
            TestThingy {
                next: Cell::new(None),
                data: value,
            }
        }
    }

    // Support for IntrusivelyQueueable trait.
    impl IntrusivelyQueueable for TestThingy {
        fn get_queue_next(&self) -> Option<NonNull<Self>> {
            self.next.get()
        }

        fn set_queue_next(&self, element: Option<NonNull<Self>>) {
            self.next.set(element);
        }
    }

    #[test]
    fn fifo_order() {
        // Create the queue.
        let mut test_iq: IntrusiveQueue<TestThingy> = IntrusiveQueue::new();

        // Verify: The queue should be empty.
        assert!(test_iq.is_empty());
        assert_eq!(test_iq.len(), 0);

        // Create some test elements.
        let element1: Rc<TestThingy> = Rc::new(TestThingy::new(1));
        let element2: Rc<TestThingy> = Rc::new(TestThingy::new(2));
        let element3: Rc<TestThingy> = Rc::new(TestThingy::new(3));
        let element4: Rc<TestThingy> = Rc::new(TestThingy::new(4));

        // Push the elements onto the end of the queue.
        test_iq.push_element(element1);
        test_iq.push_element(element2);
        test_iq.push_element(element3);
        test_iq.push_element(element4);

        // Verify: The queue should now contain 4 elements.
        assert_eq!(test_iq.is_empty(), false);
        assert_eq!(test_iq.len(), 4);

        // Pop the elements off of the front of the queue.
        // They should come off in the same order they went on (i.e. FIFO queue).
        let mut check_data: u32 = 0;
        while let Some(popped_element) = test_iq.pop_element() {
            check_data += 1;

            // Verify the correct element popped.
            assert_eq!(popped_element.data, check_data);

            // Verify refcount on element Rc is 1.
            assert_eq!(Rc::strong_count(&popped_element), 1);

            // Verify length of queue is correct.
            assert_eq!(test_iq.len(), 4 - check_data as usize);
        }

        // Verify: The queue should be empty.
        assert!(test_iq.is_empty());
        assert_eq!(test_iq.len(), 0);
    }

    #[test]
    fn drop() {
        // Create some test elements.
        let element5: Rc<TestThingy> = Rc::new(TestThingy::new(5));
        let element6: Rc<TestThingy> = Rc::new(TestThingy::new(6));
        let element7: Rc<TestThingy> = Rc::new(TestThingy::new(7));
        let element8: Rc<TestThingy> = Rc::new(TestThingy::new(8));

        // Verify refcount on each Rc is 1.
        assert_eq!(Rc::strong_count(&element5), 1);
        assert_eq!(Rc::strong_count(&element6), 1);
        assert_eq!(Rc::strong_count(&element7), 1);
        assert_eq!(Rc::strong_count(&element8), 1);

        // Call a subroutine, passing a clone of each element.
        sub(element5.clone(), element6.clone(), element7.clone(), element8.clone());

        // Verify that upon return from the subroutine, the refcount on each Rc is back to 1.
        assert_eq!(Rc::strong_count(&element5), 1);
        assert_eq!(Rc::strong_count(&element6), 1);
        assert_eq!(Rc::strong_count(&element7), 1);
        assert_eq!(Rc::strong_count(&element8), 1);

        fn sub(element5: Rc<TestThingy>, element6: Rc<TestThingy>, element7: Rc<TestThingy>, element8: Rc<TestThingy>) {
            // Create another queue.
            let mut another_iq: IntrusiveQueue<TestThingy> = IntrusiveQueue::new();

            // Verify refcount on each Rc is now 2.
            assert_eq!(Rc::strong_count(&element5), 2);
            assert_eq!(Rc::strong_count(&element6), 2);
            assert_eq!(Rc::strong_count(&element7), 2);
            assert_eq!(Rc::strong_count(&element8), 2);

            // Put only two of the elements on the queue.
            another_iq.push_element(element5);
            another_iq.push_element(element7);

            // Verify: The queue should now contain 2 elements.
            assert_eq!(another_iq.is_empty(), false);
            assert_eq!(another_iq.len(), 2);

            // Leaving this scope should drop the IntrusiveQueue with the two elements that are on it, as well as the
            // two unattached elements.
        }
    }
}
