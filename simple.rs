/*!
A very simple mutex with no blocking support.  Threads must spin while waiting to lock.

The API is exactly like that of `std::sync::Mutex`, except that it provides only `try_lock`;
`lock` is missing.  Internally it performs no syscalls and uses no platform-specific
functionality.

The other piece of unusual functionality is that the mutex can be explicity freed, specifying the
ID of a thread.  In this case, only that thread will be allowed to lock it.

```
use priomutex::simple::Mutex;
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::thread;

const N: usize = 10;

// Spawn a few threads to increment a shared variable (non-atomically), and
// let the main thread know once all increments are done.
//
// Here we're using an Arc to share memory among threads, and the data inside
// the Arc is protected with a mutex.
let data = Arc::new(Mutex::new(0));

let (tx, rx) = channel();
for _ in 0..N {
    let (data, tx) = (data.clone(), tx.clone());
    thread::spawn(move || {
        // The shared state can only be accessed once the lock is held.  Here
        // we spin-wait until the lock is acquired.
        let mut data = loop { if let Some(x) = data.try_lock() { break x } };
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
*/

use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, ThreadId};
use thread_id::*;
use std::usize;

/// A simple mutex with no blocking support.
pub struct Mutex<T> {
    data: UnsafeCell<T>,
    owner: AtomicUsize,
}

/// A magic thread ID used to designate that the lock is free to be taken by any thread.
const TID_ANY: usize = usize::MAX;

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            data: UnsafeCell::new(data),
            owner: AtomicUsize::new(TID_ANY),
        }
    }

    /// Attempts to acquire this lock.  If the lock is free, or has been assigned to this thread by
    /// the last holder, then the lock will be acquired.
    ///
    /// If the lock could not be acquired at this time, then None is returned. Otherwise, an RAII
    /// guard is returned. The lock will be unlocked when the guard is dropped.
    ///
    /// This function does not block.
    pub fn try_lock(&self) -> Option<MutexGuard<T>> {
        let me = extract_thread_id(thread::current().id()) as usize;
        assert_ne!(me, TID_ANY, "This thread's TID is equal to the magic TID used to designate \
                   that the lock is free!");
        let old = self.owner.compare_and_swap(TID_ANY, me, Ordering::SeqCst);
        if old == TID_ANY || old == me {
            // It was free and we took it, or it was already assigned to us
            Some(MutexGuard::new(self))
        } else {
            None
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock the inner mutex.
        unsafe { self.data.into_inner() }
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the `Mutex` mutably, no actual locking needs to
    /// take place---the mutable borrow statically guarantees no locks exist.
    pub fn get_mut(&mut self) -> &mut T {
        // We know statically that there are no other references to `self`, so
        // there's no need to lock the inner mutex.
        unsafe { &mut *self.data.get() }
    }
}

unsafe impl<T: Send> Send for Mutex<T> { }
unsafe impl<T: Send> Sync for Mutex<T> { }

/// An RAII guard.  Can be dereferenced to access the data protected by the mutex.  Frees the mutex
/// when dropped.
pub struct MutexGuard<'a, T: 'a> {
    __lock: &'a Mutex<T>,
    __is_valid: bool,
}

impl<'a, T> MutexGuard<'a, T> {
    fn new(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
        MutexGuard {
            __lock: mutex,
            __is_valid: true,
        }
    }

    /// Invalidate the guard and release the lock.
    ///
    /// After calling this, if no thread ID is specified then any other thread will be able to take
    /// the lock.  If a thread ID was specified then only the specified thread will be able to take
    /// the lock.
    ///
    /// **It is not necessary to call this function yourself**, since it will be run automatically
    /// when the guard goes out of scope.  This function is useful if, for some reason, you need to
    /// free the lock without dropping the guard.
    ///
    /// Calling `release_to` multiple times is safe, but won't change the thread assignment.
    /// Attempting to dereference a guard after calling `release_to` on it will result in a panic!
    pub fn release_to(&mut self, thread_id: Option<ThreadId>) {
        if self.__is_valid {
            let tid = thread_id.map(|x| extract_thread_id(x) as usize).unwrap_or(TID_ANY);
            self.__is_valid = false;
            self.__lock.owner.store(tid, Ordering::SeqCst);
        }
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.release_to(None);
    }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        assert!(self.__is_valid);
        unsafe { &*self.__lock.data.get() }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        assert!(self.__is_valid);
        unsafe { &mut *self.__lock.data.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::*;

    #[test]
    fn test() {
        let h = Arc::new(Mutex::new(vec![]));
        let mut tids = vec![];
        for i in 0..3 {
            let h = h.clone();
            tids.push(thread::spawn(move|| {
                loop {
                    if let Some(mut x) = h.try_lock() {
                        x.push(i);
                        thread::sleep(Duration::from_millis(1));
                        break;
                    }
                }
            }));
        }
        for tid in tids { tid.join().unwrap(); }
        println!("{:?}", *h.try_lock().unwrap());
    }
}
