use internal::*;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::{self, PoisonError, TryLockError};
use std::thread::{self, Thread};

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    heap: sync::Mutex<BinaryHeap<PV<Prio, Thread>>>,
    data: sync::Mutex<T>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            heap: sync::Mutex::new(BinaryHeap::new()),
            data: sync::Mutex::new(data),
        }
    }

    /// Takes the lock.  If another thread is holding it, this function will block until the lock
    /// is released.
    ///
    /// Waiting threads are woken up in order of priority.  0 is the highest priority, 1 is
    /// second-highest, etc.
    pub fn lock(&self, prio: usize) -> Result<MutexGuard<T>, PoisonError<MutexGuard<T>>> {
        // is it free?
        match self.try_lock() {
            Ok(guard) => return Ok(guard),  // mission accomplished!
            Err(TryLockError::WouldBlock) => {} // carry on...
            Err(TryLockError::Poisoned(e)) => return Err(e),
        }
        // no. let's sleep
        {
            let mut heap = self.heap.lock().unwrap();
            heap.push(PV { p: Prio::new(prio), v: thread::current() });
        }
        thread::park();
        // ok, we've been explicitly woken up.  it *must* be free! (soon)
        self.data.lock()
            .map(|g| MutexGuard(g, self))
            .map_err(|pe| PoisonError::new(MutexGuard(pe.into_inner(), self)))
    }

    /// Attempts to take the lock.  If another thread is holding it, this function returns `None`.
    pub fn try_lock(&self) -> sync::TryLockResult<MutexGuard<T>> {
        self.data.try_lock().map(|guard| MutexGuard(guard, self)).map_err(|tle| match tle {
            TryLockError::WouldBlock => TryLockError::WouldBlock,
            TryLockError::Poisoned(pe) => TryLockError::Poisoned(
                PoisonError::new(MutexGuard(pe.into_inner(), self))
            ),
        })
    }
}

/// An RAII guard.  Frees the mutex when dropped.
///
/// It can be dereferenced to access the data protected by the mutex.
pub struct MutexGuard<'a, T: 'a>(sync::MutexGuard<'a, T>, &'a Mutex<T>);

impl<'a, T> Drop for MutexGuard<'a, T> {
    /// Release the lock.
    ///
    /// If any threads are ready to take the mutex (ie. are currently blocked calling `lock`), then
    /// the one with the highest priority will receive it; if not, the mutex will just be freed.
    ///
    /// This function performs no syscalls.
    fn drop(&mut self) {
        let mut heap = self.1.heap.lock().unwrap();
        if let Some(x) = heap.pop() { x.v.unpark(); }  // wake the next thread
    }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.0
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.0
    }
}
