use std::collections::BinaryHeap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::{self, PoisonError, TryLockError};
use token::*;
use types::*;

/// A mutex which allows waiting threads to specify a priority.
#[derive(Debug)]
pub struct Mutex<T> {
    bookkeeping: sync::Mutex<Bookkeeping>,
    data: sync::Mutex<T>,
}

// Essentially all operations on `Mutex` are done while holding the bookkeeping lock.  This means
// that it's impossible that eg. one thread will be trying to lock the mutex while another one is
// dropping it.
#[derive(Debug)]
struct Bookkeeping {
    heap: BinaryHeap<PV<usize, WakeToken>>,
    free: bool,  // there's noone holding it AND noone waiting to take it
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            bookkeeping: sync::Mutex::new(Bookkeeping { heap: BinaryHeap::new(), free: true }),
            data: sync::Mutex::new(data),
        }
    }

    /// Takes the lock.  If another thread is holding it, this function will block until the lock
    /// is released.
    ///
    /// Waiting threads are woken up in order of priority.  0 is the highest priority, 1 is
    /// second-highest, etc.
    pub fn lock(&self, prio: usize) -> sync::LockResult<MutexGuard<T>> {
        let mut bk = self.bookkeeping.lock().unwrap();
        if bk.free {
            // We took it!  The data must be free (soon).
            bk.free = false;
            return self.spin_lock_data();
        }
        // no. let's sleep
        let (sleep_token, wake_token) = create_tokens();
        bk.heap.push(PV { p: prio, v: wake_token });
        mem::drop(bk);
        sleep_token.sleep();
        // ok, we've been explicitly woken up.  It *must* be free!  (soon)
        self.spin_lock_data()
    }

    /// Attempts to take the lock.  Fails if another thread it already holding it, or is another
    /// thread is already waiting to take it.
    pub fn try_lock(&self) -> sync::TryLockResult<MutexGuard<T>> {
        let mut bk = self.bookkeeping.lock().unwrap();
        if bk.free {
            // We took it!  The data must be free (soon).
            bk.free = false;
            self.spin_lock_data().map_err(TryLockError::Poisoned)
        } else {
            // It's already taken
            Err(TryLockError::WouldBlock)
        }
    }

    /// Spin waits until the data lock becomes free.  Careful: make sure you're the next thread in
    /// line before calling this!
    fn spin_lock_data(&self) -> sync::LockResult<MutexGuard<T>> {
        loop {
            match self.data.try_lock() {
                Ok(guard) => return Ok(MutexGuard(guard, self)),
                Err(TryLockError::WouldBlock) => sync::atomic::spin_loop_hint(),
                Err(TryLockError::Poisoned(pe)) => return Err(
                    PoisonError::new(MutexGuard(pe.into_inner(), self))
                ),
            }
        }
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
    fn drop(&mut self) {
        let mut bk = self.1.bookkeeping.lock().unwrap();
        if let Some(x) = bk.heap.pop() {
            // wake the next thread
            x.v.wake();
        } else {
            // release the lock
            bk.free = true;
        }
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
