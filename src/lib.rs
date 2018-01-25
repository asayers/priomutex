/*!
A mutex where waiting threads to specify a priority.

The API is very similar to `std::sync::Mutex`.  The key difference, of course, is that `lock` takes
a priority.  If multiple threads are waiting for the mutex when it's freed, the one which gave the
highest priorty will recieve it.

The other difference is that `std::sync::Mutex` implements `Sync` but not `Clone`, whereas
`priomutex::Mutex` implements `Clone` but not `Sync`.  In practice this means (1) that you don't
need to wrap a priomutex in an `Arc`, and (2) that we can't implement `into_inner` and `get_mut`.

```
use priomutex::Mutex;
use std::sync::mpsc::channel;
use std::thread;

const N: usize = 10;

// Spawn a few threads to increment a shared variable (non-atomically), and
// let the main thread know once all increments are done.
let data = Mutex::new(0);

let (tx, rx) = channel();
for _ in 0..N {
    let (data, tx) = (data.clone(), tx.clone());
    thread::spawn(move || {
        // The shared state can only be accessed once the lock is held.  Here
        // we spin-wait until the lock is acquired.
        let mut data = data.lock(0);
        // Our non-atomic increment is safe because we're the only thread
        // which can access the shared state when the lock is held.
        *data += 1;
        if *data == N {
            tx.send(()).unwrap();
        }
        // the lock is unlocked here when `data` goes out of scope.
    });
}

rx.recv().unwrap();
```

## Poisoning

Currently, priomutexes don't support poisoning; they are *not* poisoned if the thread holding the
lock panics.

*/

use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};

mod with_prio; use with_prio::*;
pub mod reservable;

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    inner: Arc<reservable::Mutex<Inner<T>>>,
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
            inner: Arc::new(reservable::Mutex::new(Inner {
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
        self.inner.try_lock().map(|inner| MutexGuard { __inner: inner })
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
    __inner: reservable::MutexGuard<'a, Inner<T>>,
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
        self.__inner.release_to(next_thread.as_ref().map(|x| x.id()));
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
