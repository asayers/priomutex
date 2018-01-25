/*!
A spinlock-only mutex with the ability to reserve the lock for another thread.

This mutex comes with no blocking support: threads must spin while waiting to lock.  The API is
exactly like that of `std::sync::Mutex` except that it omits `lock`;  use `try_lock` instead.  The
lack of blocking support means that this mutex performs no syscalls and uses no platform-specific
functionality.

The mutex can be explicity freed, specifying the `ThreadId` of a thread.  In this case, `try_lock`
will succeed only if called from the specified thread.

```
use priomutex::reservable::Mutex;
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
        let mut data = loop {
            if let Some(x) = data.try_lock() { break x }
            else { thread::yield_now(); }
        };
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
use std::hash::{Hash, Hasher};
use std::thread::{self, ThreadId};

/// A mutex with no blocking support, but the ability to reserve the lock for another thread.
pub struct Mutex<T> {
    inner: UnsafeCell<Inner<T>>,
    state: AtomicUsize,
}

struct Inner<T> {
    data: T,
    reserved_for: Option<ThreadId>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            inner: UnsafeCell::new(Inner {
                data: data,
                reserved_for: None,
            }),
            state: AtomicUsize::new(STATE_FREE),
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
        loop {
            let orig = self.state.load(Ordering::SeqCst);
            match orig {
                STATE_FREE => {
                    // It's free.  Let's take it!  (Transition 1)
                    let x = self.state.compare_and_swap(orig, STATE_LOCKED, Ordering::SeqCst);
                    if x != orig { continue; }  // The value changed under us, our CAS did nothing. Loop!
                    // We put the mutex into LOCKED, so this is safe:
                    let reserved_for = unsafe { (*self.inner.get()).reserved_for };
                    assert_eq!(reserved_for, None);  // [inv-2]
                    return Some(MutexGuard::new(self));
                }
                STATE_LOCKED => {
                    // Is's locked.  We failed to acquire it.
                    return None;
                }
                STATE_CHECKING => {
                    // Someone else is checking.  Very soon the state will change to either LOCKED
                    // or RESERVED.
                    /* loop */
                }
                reserved_for_hash => {
                    // It's reserved for someone.  Us, perhaps?
                    let me = thread::current().id();
                    let me_hash = hash_tid(me);
                    if reserved_for_hash != me_hash { return None; }
                    // It was reserved for a thread with our hash.  Let's check if it's us.  (Transition 2)
                    let x = self.state.compare_and_swap(orig, STATE_CHECKING, Ordering::SeqCst);
                    if x != orig { continue; }  // The value changed under us, our CAS did nothing. Loop!
                    // We put the mutex into CHECKING, so this is safe:
                    let reserved_for = unsafe { (*self.inner.get()).reserved_for };
                    if reserved_for == Some(me) {
                        // It *was* reserved for us!  Take the lock.  (Transition 3)
                        let x = self.state.swap(STATE_LOCKED, Ordering::SeqCst);
                        assert_eq!(x, STATE_CHECKING);  // [inv-1]
                        return Some(MutexGuard::new(self));
                    } else {
                        // It was reserved for someone else...  Put it back how we found it.  (Transition 4)
                        let x = self.state.swap(orig, Ordering::SeqCst);
                        assert_eq!(x, STATE_CHECKING);  // [inv-1]
                        return None;
                    }
                }
            }
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock the inner mutex.
        unsafe { self.inner.into_inner().data }
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the `Mutex` mutably, no actual locking needs to
    /// take place---the mutable borrow statically guarantees no locks exist.
    pub fn get_mut(&mut self) -> &mut T {
        // We know statically that there are no other references to `self`, so
        // there's no need to lock the inner mutex.
        unsafe { &mut (*self.inner.get()).data }
    }
}

unsafe impl<T: Send> Send for Mutex<T> { }
unsafe impl<T: Send> Sync for Mutex<T> { }

/// An RAII guard.  Frees the mutex when dropped.
///
/// While the guard is still valid, it can be dereferenced to access the data protected by the
/// mutex.  Attempting to dereference a guard which has been released will result in a panic!
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

    /// Invalidate the guard and release the lock.  If this lock has already been released, calling
    /// this function again does nothing.
    ///
    /// * If no thread ID was specified:  any other thread will be able to take the lock.
    /// * If a thread ID was specified:  only the specified thread will be able to take the lock.
    ///
    /// It is not necessary to call this function yourself, since it will be run automatically when
    /// the guard goes out of scope.
    pub fn release_to(&mut self, thread_id: Option<ThreadId>) {
        if self.__is_valid {
            // We put the mutex into LOCKED (inv-3), so this is safe:
            unsafe { self.__lock.inner.get().as_mut().unwrap().reserved_for = thread_id; }
            self.__is_valid = false;
            let new_state = thread_id.map(hash_tid).unwrap_or(STATE_FREE);
            // Transition 5 or Transition 6
            let x = self.__lock.state.swap(new_state, Ordering::SeqCst);
            assert_eq!(x, STATE_LOCKED);  // [inv-3]
        }
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    /// Release the mutex, without reserving it for any particular thread.
    fn drop(&mut self) {
        self.release_to(None);
    }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    /// Will panic if the guard has already been released via a call to `release_to`.
    fn deref(&self) -> &T {
        assert!(self.__is_valid);
        // We put the mutex into LOCKED (inv-3), so this is safe:
        unsafe { &(*self.__lock.inner.get()).data }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    /// Will panic if the guard has already been released via a call to `release_to`.
    fn deref_mut(&mut self) -> &mut T {
        assert!(self.__is_valid);
        // The state is LOCKED (inv-3), so this is safe:
        unsafe { &mut (*self.__lock.inner.get()).data }
    }
}

/* Note [State machine]

The state of the mutex is one of:

 * FREE      - The lock is available for anyone to take
 * RESERVED  - The lock is available for a specific thread to take
 * LOCKED    - The lock is currently held by a thread
 * CHECKING  - Some thread is currently testing to see if it is allowed to take the lock

The allowed transitions are:

 (1)  FREE     -> LOCKED      - The lock was free, take it!
 (2)  RESERVED -> CHECKING    - The lock is reserved. Check if it's reserved for us.
 (3)  CHECKING -> LOCKED      - We checked if it was reseved for us, and it was!
 (4)  CHECKING -> RESERVED    - We checked if it was reseved for us, and it wasn't :-(
 (5)  LOCKED   -> FREE        - Release the lock for anyone to take
 (6)  LOCKED   -> RESERVED    - Release the lock for someone particular to take

If you change the state to LOCKED or CHECKING, you have exclusive access to the inner until you
change the state back to FREE or RESERVED.

[inv-1]: If the state is LOCKED or CHECKING, only the thread which moved the mutex into that state
         is allowed to update the state.
[inv-2]: If the state is FREE, then `reserved_for` must be None.
[inv-3]: If a mutex guard exists and is valid, then the state must be LOCKED.  Furthermore, the
         mutex was put into that state by the thread holding the reference to the valid guard.

*/

const STATE_FREE: usize = 0;
const STATE_LOCKED: usize = 1;
const STATE_CHECKING: usize = 2;
// Anything else means "RESERVED". As an optimisation, the value is the hash of the ThreadId.

/// Hash a ThreadId to a usize which is guaranteed to be greater than 2, so that it doesn't collide
/// with STATE_{FREE,LOCKED,CHECKING}.
///
/// In rust 1.23, this is guaranteed not to have any collisions for the first (usize::MAX - 3)
/// threads you spawn.
fn hash_tid(tid: ThreadId) -> usize {
    struct IdHasher(u64);
    impl Hasher for IdHasher {
        // If it's a u64, just take it (in rust 1.23, ThreadIds are just a u64)
        #[inline] fn write_u64(&mut self, x: u64) { self.0 = x; }
        // Uh oh! The implementation of ThreadId has changed! Fall back to FNV
        #[inline] fn write(&mut self, xs: &[u8])  {
            for x in xs {
                self.0 = self.0 ^ *x as u64;
                self.0 = self.0.wrapping_mul(0x100000001b3);
            }
        }
        #[inline] fn finish(&self) -> u64 { self.0 }
    }
    let mut hasher = IdHasher(0);
    tid.hash(&mut hasher);
    let x = hasher.0 as usize;
    x.saturating_add(3)
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
