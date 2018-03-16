/*!
A high-performance variant of priomutex.

This mutex is very similar to the one in the root of the crate, except that the next-in-line thread
busy-waits.  This means that dropping the `MutexGuard` never requires any syscalls, and takes
~200ns on my machine (as opposed to ~3000ns for the starndard priomutex).  No matter how many
threads are waiting on your mutex, it's guaranteed that only one will be busy-waiting at any time.

Suppose thread 1 has the lock and thread 2 is busy-waiting for it.  Now thread 3 tries to lock the
mutex with a higher priority then thread 2.  Thread 3 will now busy-wait, while thread 2 goes to
sleep.  When thread 1 releases the lock, thread 3's `lock` call will return, while thread 2 wakes
up and starts busy-waiting once more.
*/
use internal::*;
use std::collections::BinaryHeap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::{self, PoisonError, TryLockError};
use std::thread::{self, Thread};

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    spinner_lock: sync::Mutex<()>,
    heap: sync::Mutex<BinaryHeap<PV<Prio, WakeToken>>>,
    data: sync::Mutex<T>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            spinner_lock: sync::Mutex::new(()),
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
        let prio = Prio::new(prio);
        let mut spinner_lock: Option<sync::MutexGuard<()>> = None;
        loop {
            let mut heap = self.heap.lock().unwrap();
            let should_sleep = match self.try_lock() {
                Ok(guard) => {
                    if let Some(x) = heap.pop() { x.v.wake(); }  // wake the next spinner
                    return Ok(guard);                              // mission accomplished!
                }
                Err(TryLockError::WouldBlock) => {
                    if spinner_lock.is_some() {              // are we the spinner?
                        if heap.peek().map(|sleeper| sleeper.p < prio).unwrap_or(false) {
                            spinner_lock = None;             // release the spinner lock
                            heap.pop().unwrap().v.wake();  // ... wake the rightful spinner
                            true                             // ... and then sleep
                        } else {
                            false                            // we're the rightful spinner
                        }
                    } else {
                        match self.spinner_lock.try_lock() {
                            Ok(guard) => {
                                spinner_lock = Some(guard);  // let's stash the lock
                                false                        // we're now the spinner!
                            }
                            Err(TryLockError::WouldBlock) => true, // someone else already spinning
                            Err(TryLockError::Poisoned(_)) => panic!(),
                        }
                    }
                }
                Err(TryLockError::Poisoned(e)) => return Err(e),
            };
            let (sleep_token, wake_token) = create_tokens();
            if should_sleep {
                heap.push(PV { p: prio, v: wake_token });
            }
            mem::drop(heap);
            if should_sleep {
                sleep_token.sleep();
            } else {
                thread::yield_now();
            }
        }
    }

    /// Attempts to take the lock.  If another thread is holding it, this function returns `None`.
    pub fn try_lock(&self) -> sync::TryLockResult<MutexGuard<T>> {
        self.data.try_lock().map(|guard| MutexGuard(guard)).map_err(|tle| match tle {
            TryLockError::WouldBlock => TryLockError::WouldBlock,
            TryLockError::Poisoned(pe) => TryLockError::Poisoned(
                PoisonError::new(MutexGuard(pe.into_inner()))
            ),
        })
    }
}

/// An RAII guard.  Frees the mutex when dropped.
///
/// It can be dereferenced to access the data protected by the mutex.
pub struct MutexGuard<'a, T: 'a>(sync::MutexGuard<'a, T>);

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
