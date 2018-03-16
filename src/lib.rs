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
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const N: usize = 10;

let m = Arc::new(Mutex::new(Vec::new()));
let guard = m.lock(0).unwrap();

let mut tids = Vec::new();
for _ in 0..N {
    let m = m.clone();
    tids.push(thread::spawn(move || {
        let mut rng = thread_rng();
        let prio = rng.gen::<usize>();               // generate a random priority
        let mut data = m.lock(prio).unwrap();     // wait on the mutex
        data.push(prio);                             // push priority onto the list
    }));
}

// Give the threads time to spawn and wait on the mutex
thread::sleep(Duration::from_millis(10));

mem::drop(guard);             // go go go!
for t in tids { t.join(); }   // wait until they've all modified the mutex

// Check that every thread pushed an element
let d1 = m.lock(0).unwrap();
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

extern crate fnv;
#[macro_use] extern crate log;

use fnv::FnvHasher;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::{self, PoisonError, TryLockError};
use std::thread::{self, Thread};

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    spinner_lock: sync::Mutex<()>,
    heap: sync::Mutex<BinaryHeap<PV<Prio, Thread>>>,
    data: sync::Mutex<T>,
}

/// Prio guarantees a *total* order, even though the values provided by the user might only be
/// partially ordered.  It does this by also comparing on ThreadId.
///
/// Assumptions: only one Prio per thread; no hash collisions.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Prio {
    prio: usize,
    thread_hash: u64,
}

impl Prio {
    pub fn new(prio: usize) -> Prio {
        let mut s = FnvHasher::default();
        thread::current().id().hash(&mut s);
        Prio {
            prio: prio,
            thread_hash: s.finish(),
        }
    }
}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(data: T) -> Mutex<T> {
        let (tx, rx) = mpsc::channel();
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
            match self.try_lock() {
                // We took the data lock - mission accomplished!
                Ok(guard) => return Ok(guard),
                // The data's still not available, but we're the spinner.  Let's spin!
                Err(TryLockError::WouldBlock) if spinner_lock.is_some() => {
                    if self.should_sleep(prio) {
                        // someone else deserves to be the spinner
                        spinner_lock = None;   // let's release the spinner lock
                        self.heap.lock().unwrap().pop().unwrap().v.unpark();  // ... wake this other thread
                        self.sleep(prio);      // ... and then sleep ourselves
                    }
                }
                Err(TryLockError::WouldBlock) => {
                    // We're not the spinner yet - let's try to take the lock
                    match self.spinner_lock.try_lock() {
                        // we're now the spinner!  let's stash the lock
                        Ok(guard) => spinner_lock = Some(guard),
                        // someone else is spinning already. let's sleep until someone wakes us
                        Err(TryLockError::WouldBlock) => self.sleep(prio),
                        Err(TryLockError::Poisoned(_)) => panic!(),
                    }
                }
                Err(TryLockError::Poisoned(e)) => return Err(e),
            }
            thread::yield_now();
        }
    }

    fn sleep(&self, prio: Prio) {
        {
            let mut heap = self.heap.lock().unwrap();
            heap.push(PV { p: prio, v: thread::current() });
        }
        thread::park();
    }

    fn should_sleep(&self, prio: Prio) -> bool {
        self.heap.lock().unwrap().peek().map(|sleeper| sleeper.p < prio).unwrap_or(false)
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

//impl<'a, T> Drop for MutexGuard<'a, T> {
//    /// Release the lock.
//    ///
//    /// If any threads are ready to take the mutex (ie. are currently blocked calling `lock`), then
//    /// the one with the highest priority will receive it; if not, the mutex will just be freed.
//    ///
//    /// This function performs no syscalls.
//    fn drop(&mut self) {
//    }
//}

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



/// A value `V` with a priority `P`
struct PV<P,V> {
    p: P,
    v: V,
}

impl<P:PartialEq,V> PartialEq for PV<P,V> {
    fn eq(&self, other: &PV<P,V>) -> bool { other.p == self.p }
}

impl<P:Eq,V> Eq for PV<P,V> {}

impl<P:PartialOrd,V> PartialOrd for PV<P,V> {
    fn partial_cmp(&self, other: &PV<P,V>) -> Option<Ordering> { other.p.partial_cmp(&self.p) }
}

impl<P:Ord,V> Ord for PV<P,V> {
    fn cmp(&self, other: &PV<P,V>) -> Ordering { other.p.cmp(&self.p) }
}
