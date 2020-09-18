// Leaving this TODO for now, since I'm just going to fork `unicycle` for now.
// Here's two main ideas for how we could do this efficiently.
//
// Option 1 (alignment tricks):
// 1) Create a simple PinSlab<T> to store our futures.
// 2) Bound the number of futures in the system by N (say 4096). Or, accept lower
//    fidelity wakeups based on N.
// 3) Heap allocate a reference counted WakerState with alignment N.
// 4) When creating a Waker for Future number i, compute the pointer WakerState + i.
// 5) When waking a Waker, compute the original WakerState pointer and set the
//    bit for the original index.
// 6) When cloning a Waker, compute the original pointer, clone that, and then
//    return the *same* pointer.
//
// Option 2 (intrusive waker):
// 1) Build a specialized PinSlab, where each Page has a header and is aligned
//    to some N.
// 2) Wakers are just pointers to the future directly.
// 3) Each header contains a reference count, and creating a waker bumps this
//    reference count. The Waker has no guarantee that the original future is
//    still there, but it knows that the page is still live.
// 4) Each header has a reference counted pointer to a central control block,
//    which contains a root bitset indicating which pages have ready futures and
//    a Option<Waker> for the parent waker.
// 5) The AsyncSet then has a pointer to the control block it uses for finding
//    which futures are currently ready.
//
// I'm partial to Option 2 but we'll have to benchmark and see how it also plays
// into our multicore scalable design.
//

// use super::pin_slab::{PinSlab, SlabKey};
// use uniset::BitSet;
// use parking_lot::Mutex;
// use std::sync::Arc;
// use std::task::{Context, Wake, Waker, Poll, RawWaker, RawWakerVTable};
// use std::pin::Pin;
// use std::future::Future;
// use futures::Stream;

// const PAGE_SIZE: usize = 32;

// struct WakerState {
//     ready: BitSet,
//     parent_waker: Option<Waker>,
// }

// pub struct AsyncSet<T> {
//     slab: PinSlab<T, PAGE_SIZE>,

//     // TODO: Investigate using `PinSlab`'s optimizations here.
//     waker_state: Arc<Mutex<WakerState>>,
//     current_ready: BitSet,
// }

// impl<T> AsyncSet<T> {
//     pub fn new() -> Self {
//         let waker_state = WakerState {
//             ready: BitSet::new(),
//             parent_waker: None,
//         };
//         Self {
//             slab: PinSlab::new(),
//             waker_state: Arc::new(Mutex::new(waker_state)),
//             current_ready: BitSet::new(),
//         }
//     }

//     pub fn insert(&mut self, value: T) -> SlabKey {
//         let key = self.slab.alloc(value);

//         self.current_ready.set(key.into());

//         let mut waker_state = self.waker_state.lock();
//         if let Some(p) = waker_state.parent_waker.take() {
//             p.wake();
//         }

//         key
//     }

//     pub fn remove(&mut self, key: SlabKey) -> bool {
//         self.slab.free(key)
//     }

//     pub fn get(&self, key: SlabKey) -> Option<&T> {
//         self.slab.get(key)
//     }

//     pub fn get_pin_mut(&mut self, key: SlabKey) -> Option<Pin<&mut T>> {
//         self.slab.get_pin_mut(key)
//     }
// }

// impl<T: Future> Stream for AsyncSet<T> {
//     type Item = (SlabKey, T::Output);

//     fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
//         let self_ = self.get_mut();

//         for i in self_.current_ready.drain() {
//             let key = SlabKey::from(i);
//             let future = self_.slab.get_pin_mut(key).expect("Invalid ready bit");
//         }
//         todo!()
//     }
// }

// struct IndexWaker {
//     index: usize,
//     state: Arc<Mutex<WakerState>>,
// }

// impl IndexWaker {
//     unsafe fn clone(this: *const ()) -> RawWaker {
//         let this = &*(this as *const Self);

//     }
// }

// static INDEX_WAKER_VTABLE: &RawWakerVTable = &RawWakerVTable::new(
//     Index
// )
