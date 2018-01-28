/*!
A mutex where waiting threads to specify a priority.

The API is very similar to `std::sync::Mutex`.  The key difference, of course, is that `lock` takes
a priority.  If multiple threads are waiting for the mutex when it's freed, the one which gave the
highest priorty will recieve it.

The other difference is that `std::sync::Mutex` implements `Sync` but not `Clone`, whereas
`priomutex::Mutex` implements `Clone` but not `Sync`.  In practice this means (1) that you don't
need to wrap a priomutex in an `Arc`, and (2) that we can't implement `into_inner` and `get_mut`.

```
# extern crate rand;
# extern crate priomutex;
# fn main() {
use priomutex::Mutex;
use rand::{Rng, thread_rng};
use std::mem;
use std::thread;
use std::time::Duration;

const N: usize = 10;

let data = Mutex::new(Vec::new());
let guard = data.lock(0);

let mut tids = Vec::new();
for _ in 0..N {
    let data = data.clone();
    tids.push(thread::spawn(move || {
        let mut rng = thread_rng();
        let prio = rng.gen::<usize>();      // generate a random priority
        let mut data = data.lock(prio);     // wait on the mutex
        data.push(prio);                    // push priority onto the list
    }));
}

// Give the threads time to spawn and wait on the mutex
thread::sleep(Duration::from_millis(10));

mem::drop(guard);             // go go go!
for t in tids { t.join(); }   // wait until they've all modified the mutex

// Check that every thread pushed an element
let d1 = data.lock(0);
assert_eq!(d1.len(), N);

// Check that the threads were woken in priority order
let mut d2 = d1.clone(); d2.sort();
assert_eq!(*d1, d2);
# }
```

## Poisoning

Currently, priomutexes don't support poisoning; they are *not* poisoned if the thread holding the
lock panics.

*/

extern crate pwmutex;

use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread, ThreadId};

mod with_prio; use with_prio::*;

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    inner: Arc<pwmutex::Mutex<Inner<T>, ThreadId>>,
    tx: mpsc::Sender<WithPrio<Thread>>,
}

// This is derived anyway by the autotrait rules, but we make it explicit so that it shows up in
// the docs.
unsafe impl<T: Send> Send for Mutex<T> { }

impl<T> Clone for Mutex<T> {
    fn clone(&self) -> Self {
        Mutex {
            inner: self.inner.clone(),
            tx: self.tx.clone(),
        }
    }
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        let (tx, rx) = mpsc::channel();
        Mutex {
            inner: Arc::new(pwmutex::Mutex::new(Inner {
                data: data,
                rx: rx,
                heap: BinaryHeap::new(),
            })),
            tx: tx,
        }
    }

    /// Takes the lock.  If another thread is holding it, this function will block until the lock
    /// is released.
    ///
    /// Waiting threads are woken up in order of priority.  0 is the highest priority, 1 is
    /// second-highest, etc.
    pub fn lock(&self, prio: usize) -> MutexGuard<T> {
        loop {
            if let Some(x) = self.try_lock() {
                return x;
            } else {
                let me = WithPrio { prio: prio, inner: thread::current() };
                self.tx.send(me).unwrap();
                thread::park();
            }
        }
    }

    /// Attempts to take the lock.  If another thread is holding it, this function returns `None`.
    pub fn try_lock(&self) -> Option<MutexGuard<T>> {
        self.inner.try_lock_pw(thread::current().id()).map(|inner| MutexGuard { __inner: inner })
    }
}

struct Inner<T> {
    data: T,
    rx: mpsc::Receiver<WithPrio<Thread>>,
    heap: BinaryHeap<WithPrio<Thread>>,
}

impl<T> Inner<T> {
    fn next_thread(&mut self) -> Option<Thread> {
        for x in self.rx.try_iter() {
            self.heap.push(x);
        }
        self.heap.pop().map(|x| x.inner)
    }
}

/// An RAII guard.  Frees the mutex when dropped.
///
/// It can be dereferenced to access the data protected by the mutex.
pub struct MutexGuard<'a, T: 'a> {
    __inner: pwmutex::MutexGuard<'a, Inner<T>, ThreadId>,
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    /// Release the lock.
    ///
    /// If any threads are ready to take the mutex (ie. are currently blocked calling `lock`), then
    /// the one with the highest priority will receive it; if not, the mutex will just be freed.
    ///
    /// This function performs a syscall when there are threads waiting.  On my machine this takes
    /// ~3 μs.
    fn drop(&mut self) {
        // Release the lock first, and *then* wake the next thread.  If we rely on the Drop impl
        // for simple::MutexGuard then these operations happen in the reverse order, which can lead
        // to a deadlock.
        let next_thread = self.__inner.next_thread();
        match next_thread.as_ref() {
            Some(thread) => self.__inner.release_protected(thread.id()),
            None => self.__inner.release(),
        }
        if let Some(h) = next_thread {
            h.unpark();
        }
    }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &(*self.__inner).data
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut (*self.__inner).data
    }
}
