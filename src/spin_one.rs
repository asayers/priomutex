/*!
A high-performance variant of priomutex.

This mutex is very similar to the one in the root of the crate, except that the next-in-line thread
busy-waits.  This means that dropping the `MutexGuard` never requires any syscalls, and takes
100-200ns on my machine (20x faster than the standard priomutex).  No matter how many threads are
waiting on your mutex, it's guaranteed that only one will be busy-waiting at any time.

Suppose thread 1 has the lock and thread 2 is busy-waiting for it.  Now thread 3 tries to lock the
mutex with a higher priority then thread 2.  Thread 3 will now busy-wait, while thread 2 goes to
sleep.  When thread 1 releases the lock, thread 3's `lock` call will return, while thread 2 wakes
up and starts busy-waiting once more.
*/

use std::collections::BinaryHeap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::{self, PoisonError, TryLockError};
use std::thread;
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
    no_spinner: bool, // there's no spinner AND there noone waiting to become the spinner
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            bookkeeping: sync::Mutex::new(Bookkeeping { heap: BinaryHeap::new(), no_spinner: true }),
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
        if bk.no_spinner {
            // We're the spinner now
            bk.no_spinner = false;
            mem::drop(bk);
        } else {
            let (sleep_token, wake_token) = create_tokens();
            bk.heap.push(PV { p: prio, v: wake_token });
            mem::drop(bk);
            sleep_token.sleep();
        }

        let guard = loop {
            // Our WakeToken has been signalled.  That means we're the spinner now!
            match self.data.try_lock() {
                Ok(guard) =>
                    break MutexGuard(guard),
                Err(TryLockError::WouldBlock) =>
                    thread::yield_now(),
                Err(TryLockError::Poisoned(pe)) =>
                    return Err(PoisonError::new(MutexGuard(pe.into_inner()))),
            }

            let mut bk = self.bookkeeping.lock().unwrap();
            let (sleep_token, wake_token) = create_tokens();
            bk.heap.push(PV { p: prio, v: wake_token });
            bk.heap.pop().unwrap().v.wake();
            mem::drop(bk);
            sleep_token.sleep();
        };

        // We took the lock
        let mut bk = self.bookkeeping.lock().unwrap();
        if let Some(x) = bk.heap.pop() {
            // Wake the next spinner
            x.v.wake();
        } else {
            // Let the next thread to lock become the spinner
            bk.no_spinner = true;
        }
        Ok(guard)
    }

    /// Attempts to take the lock.  Fails if another thread it already holding it, or is another
    /// thread is already waiting to take it.
    pub fn try_lock(&self) -> sync::TryLockResult<MutexGuard<T>> {
        let bk = self.bookkeeping.lock().unwrap();
        if !bk.no_spinner {
            return Err(TryLockError::WouldBlock);
        }
        mem::drop(bk);
        match self.data.try_lock() {
            Ok(guard) => Ok(MutexGuard(guard)),
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
            Err(TryLockError::Poisoned(pe)) =>
                Err(TryLockError::Poisoned(PoisonError::new(MutexGuard(pe.into_inner())))),
        }
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
