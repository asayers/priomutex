/*!
A (spinlock-only) mutex which can be password-protected.

A pwmutex will be freed as usual when the guard goes out of scope, but it can also be explicity
freed by calling one of the `release` methods on the guard.  When releasing the lock, it's possible
to specify a password.  In this case, `try_lock` will succeed only when the correct password is
given.

pwmutex comes with support for blocking.  As such, there is no `lock`:  only `try_lock.  Waiting
threads should spin until `try_lock` succeeds.  The lack of blocking support means that pwmutex can
be implemented with no syscalls or platform-specific functionality.

pwmutex comes with no support for poisoning:  if a thread panics while holding the lock, the mutex
will be freed normally.

```
use pwmutex::Mutex;
use std::sync::Arc;
use std::thread;

const ITERS: usize = 100_000;
const THREADS: usize = 3;

let m = Arc::new(Mutex::new(0));
let mut tids = vec![];
for i in 0..THREADS {
    let m1 = m.clone();
    tids.push(::std::thread::spawn(move|| {
        for _ in 0..ITERS {
            let mut g = loop {
                if let Some(x) = m1.try_lock_pw(i) { break x }
                else { thread::yield_now(); }
            };
            *g += 1;
            g.release_protected((i + 1) % THREADS);
        }
    }));
}
for t in tids { t.join().unwrap(); }
let g = m.try_lock().unwrap();
assert_eq!(*g, THREADS * ITERS);
```
*/

#[cfg(test)] extern crate easybench;
extern crate fnv;

use std::cell::UnsafeCell;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A mutex with no blocking support, but the ability to password-protect it.
pub struct Mutex<T: ?Sized, P> {
    password: UnsafeCell<Option<P>>,
    state: AtomicUsize,
    data: UnsafeCell<T>,
}

impl<T, P> Mutex<T, P> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T, P> {
        Mutex {
            password: UnsafeCell::new(None),
            state: AtomicUsize::new(STATE_FREE),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized, P: Hash + PartialEq> Mutex<T, P> {
    /// Attempts to acquire this lock.  If the lock is free, or has been assigned to this thread by
    /// the last holder, then the lock will be acquired.
    ///
    /// If the lock could not be acquired at this time, then None is returned. Otherwise, an RAII
    /// guard is returned. The lock will be unlocked when the guard is dropped.
    ///
    /// This function does not block.
    //
    // If we had a reliable way of extracting the u64 from a ThreadId, and if AtomicU64 were
    // stable, then this function could be implemented much more effeciently (a single CAS).
    // Instead, locking currently requires two or three atomic operations :-(
    pub fn try_lock(&self) -> Option<MutexGuard<T, P>> {
        loop {
            let orig = self.state.load(Ordering::SeqCst);
            match orig {
                STATE_FREE => {
                    // It's free.  Let's take it!  (Transition 1)
                    let x = self.state.compare_and_swap(orig, STATE_LOCKED, Ordering::SeqCst);
                    if x != orig { /* Our CAS didn't work. Loop! */ continue; }
                    // assert_eq!(*self.password.get(), None, "inv-2 violated");
                    return Some(MutexGuard::new(self));
                }
                STATE_LOCKED => return None,  // Already locked.
                STATE_CHECKING => {} // Someone else is checking.  Very soon the state will change
                                     // to either LOCKED or PW_PROTECTED. Loop!
                _password_hash => {
                    // It's password protected.  We don't care! Take the lock!  (Transition 2)
                    let x = self.state.compare_and_swap(orig, STATE_LOCKED, Ordering::SeqCst);
                    if x != orig { /* Our CAS didn't work. Loop! */ continue; }
                    return Some(MutexGuard::new(self));
                }
            }
        }
    }

    pub fn try_lock_pw(&self, pw_attempt: P) -> Option<MutexGuard<T, P>> {
        loop {
            let orig = self.state.load(Ordering::SeqCst);
            match orig {
                STATE_FREE => {
                    // It's free.  Let's take it!  (Transition 1)
                    let x = self.state.compare_and_swap(orig, STATE_LOCKED, Ordering::SeqCst);
                    if x != orig { /* Our CAS didn't work. Loop! */ continue; }
                    // assert_eq!(*self.password.get(), None, "inv-2 violated");
                    return Some(MutexGuard::new(self));
                }
                STATE_LOCKED => {
                    // Is's locked.  We failed to acquire it.
                    return None;
                }
                STATE_CHECKING => {
                    // Someone else is checking.  Very soon the state will change to either LOCKED
                    // or PW_PROTECTED.
                    /* loop */
                }
                password_hash => {
                    // It's password protected.  Does our password look promising...?
                    let attempt_hash = hash_pw(&pw_attempt);
                    if password_hash != attempt_hash { /* ...no */ return None; }
                    // ...yes! Ok, let's check if our password is really correct.
                    // (Transition 3)
                    let x = self.state.compare_and_swap(orig, STATE_CHECKING, Ordering::SeqCst);
                    if x != orig { /* Our CAS didn't work. Loop! */ continue; }
                    let pw_is_correct = unsafe {
                        // We put the mutex into CHECKING, so it's safe to access password
                        *self.password.get() == Some(pw_attempt)
                    };
                    if pw_is_correct {
                            // Nice!  Take the lock.  (Transition 4)
                            let x = self.state.swap(STATE_LOCKED, Ordering::SeqCst);
                            assert_eq!(x, STATE_CHECKING, "inv-1 violated");
                            return Some(MutexGuard::new(self));
                    } else {
                        // Boo...  Put it back how we found it.  (Transition 5)
                        let x = self.state.swap(orig, Ordering::SeqCst);
                        assert_eq!(x, STATE_CHECKING, "inv-1 violated");
                        return None;
                    }
                }
            }
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T where T: Sized {
        unsafe {
            // We know statically that there are no outstanding references to `self` so there's no
            // need to lock the inner mutex.
            self.data.into_inner()
        }
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the `Mutex` mutably, no actual locking needs to take place---the
    /// mutable borrow statically guarantees no locks exist.
    pub fn get_mut(&mut self) -> &mut T {
        unsafe {
            // We know statically that there are no other references to `self`, so there's no need
            // to lock the inner mutex.
            &mut *self.data.get()
        }
    }
}

unsafe impl<T: ?Sized + Send, P> Send for Mutex<T, P> { }
unsafe impl<T: ?Sized + Send, P> Sync for Mutex<T, P> { }

/// An RAII guard.  Frees the mutex when dropped.
///
/// While the guard is still valid, it can be dereferenced to access the data protected by the
/// mutex.  Attempting to dereference a guard which has been released will result in a panic!
pub struct MutexGuard<'a, T: ?Sized + 'a, P: 'a + Hash> {
    __lock: &'a Mutex<T, P>,
    __is_valid: bool,
}

impl<'a, T: ?Sized, P: Hash> MutexGuard<'a, T, P> {
    fn new(mutex: &'a Mutex<T, P>) -> MutexGuard<'a, T, P> {
        MutexGuard {
            __lock: mutex,
            __is_valid: true,
        }
    }

    /// Invalidate the guard and release the lock without password-protecting it.  Any password
    /// will work for re-taking the lock.  If this lock has already been released, calling this
    /// function does nothing.
    ///
    /// **It is not necessary to call this function yourself**, since it will be run automatically
    /// when the guard goes out of scope.
    pub fn release(&mut self) {
        if self.__is_valid {
            unsafe {
                // We put the mutex into LOCKED (inv-3), so this is safe:
                *self.__lock.password.get() = None;
            }
            self.__is_valid = false;
            // Transition 6
            let x = self.__lock.state.swap(STATE_FREE, Ordering::SeqCst);
            assert_eq!(x, STATE_LOCKED);  // [inv-3]
        }
    }

    /// Invalidate the guard and release the lock, protecting it with a password.  The lock can
    /// only be re-taken using the password specified.  If this lock has already been released,
    /// calling this function does nothing.
    pub fn release_protected(&mut self, password: P) {
        if self.__is_valid {
            let hash = hash_pw(&password);
            unsafe {
                // We put the mutex into LOCKED (inv-3), so this is safe:
                *self.__lock.password.get() = Some(password);
            }
            self.__is_valid = false;
            // Transition 7
            let x = self.__lock.state.swap(hash, Ordering::SeqCst);
            assert_eq!(x, STATE_LOCKED);  // [inv-3]
        }
    }
}

impl<'a, T: ?Sized, P: Hash> Drop for MutexGuard<'a, T, P> {
    /// Release the mutex, without password-protecting it.
    fn drop(&mut self) {
        self.release();
    }
}

impl<'a, T: ?Sized, P: Hash> Deref for MutexGuard<'a, T, P> {
    type Target = T;
    /// Will panic if the guard has already been released via a call to `release_protected`.
    fn deref(&self) -> &T {
        assert!(self.__is_valid);
        unsafe {
            // We put the mutex into LOCKED (inv-3), so this is safe:
            &(*self.__lock.data.get())
        }
    }
}

impl<'a, T: ?Sized, P: Hash> DerefMut for MutexGuard<'a, T, P> {
    /// Will panic if the guard has already been released via a call to `release_protected`.
    fn deref_mut(&mut self) -> &mut T {
        assert!(self.__is_valid);
        unsafe {
            // We put the mutex into LOCKED (inv-3), so this is safe:
            &mut (*self.__lock.data.get())
        }
    }
}

/* Note [State machine]

The state of the mutex is one of:

 * FREE         - The lock is available for anyone to take
 * PW_PROTECTED - The lock is available for someone with the password to take
 * LOCKED       - The lock is currently held by a thread
 * CHECKING     - Some thread is currently testing to see if it is allowed to take the lock

The allowed transitions are:

 (1)  FREE         -> LOCKED       - The lock was free, take it!
 (2)  PW_PROTECTED -> LOCKED       - The lock is protected, but we don't care. Take it anyway!
 (3)  PW_PROTECTED -> CHECKING     - The lock is protected. Check if our pw is correct.
 (4)  CHECKING     -> LOCKED       - We checked if our pw was correct, and it was!
 (5)  CHECKING     -> PW_PROTECTED - We checked if our pw was correct, and it wasn't :-(
 (6)  LOCKED       -> FREE         - Release the lock for anyone to take
 (7)  LOCKED       -> PW_PROTECTED - Release the lock for someone particular to take

If you change the state to LOCKED or CHECKING, you have exclusive access to the inner until you
change the state back to FREE or PW_PROTECTED.

 inv-1: If the state is LOCKED or CHECKING, only the thread which moved the mutex into that state
        is allowed to update the state.
 inv-2: If the state is FREE, then `password` must be None.
 inv-3: If a mutex guard exists and is valid, then the state must be LOCKED.  Furthermore, the
        mutex was put into that state by the thread holding the reference to the valid guard.

*/

const STATE_FREE: usize = 0;
const STATE_LOCKED: usize = 1;
const STATE_CHECKING: usize = 2;
// Anything else means "PW_PROTECTED". As an optimisation, the value is the hash of the password.

/// Hash to a usize which is guaranteed to be greater than 2, so that it doesn't collide with
/// STATE_{FREE,LOCKED,CHECKING}.
fn hash_pw<P: Hash>(password: &P) -> usize {
    let mut hasher = fnv::FnvHasher::default();
    password.hash(&mut hasher);
    (hasher.finish() as usize).saturating_add(2)
}

#[cfg(test)]
mod tests {

    use std::sync::Arc;
    use std::thread::{self, ThreadId};
    use std::time::*;
    use super::*;

    #[test]
    fn test() {
        let h = Arc::new(Mutex::new(vec![]));
        let mut tids = vec![];
        for i in 0..3 {
            let h = h.clone();
            tids.push(thread::spawn(move|| {
                loop {
                    if let Some(mut x) = h.try_lock_pw(thread::current().id()) {
                        x.push(i);
                        thread::sleep(Duration::from_millis(1));
                        break;
                    }
                }
            }));
        }
        for tid in tids { tid.join().unwrap(); }
        println!("{:?}", *h.try_lock_pw(thread::current().id()).unwrap());
    }

    #[test]
    fn test_hash_pw() {
        fn mk_thread() -> ThreadId {
            thread::spawn(||{ thread::sleep(Duration::from_millis(1)); }).thread().id()
        }
        const N: usize = 10_000;
        let hashes: Vec<usize> = (0..N).map(|_| hash_pw(&mk_thread())).collect();
        let mut hashes_sorted = hashes.clone(); hashes_sorted.sort();
        let mut hashes_uniq = hashes_sorted.clone(); hashes_uniq.dedup();
        assert_eq!(hashes.len(), N, "wrong number of threads!?");
        assert_eq!(hashes_sorted, hashes_uniq, "hash collision! oh no!");
        // assert_eq!(hashes_sorted, hashes, "hashes weren't monotonically increasing... weird...");
    }
}
