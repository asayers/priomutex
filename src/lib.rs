/*!
A mutex which allows waiting threads to specify a priority.
*/

use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};

mod with_prio; use with_prio::*;
pub mod simple;

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    inner: Arc<simple::Mutex<Inner<T>>>,
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
    /// Create a new prio-mutex.
    pub fn new(data: T) -> Mutex<T> {
        let (tx, rx) = mpsc::channel();
        Mutex {
            inner: Arc::new(simple::Mutex::new(Inner {
                data: data,
                rx: rx,
                heap: BinaryHeap::new(),
            })),
            tx: tx,
        }
    }

    /// Attempt to take the mutex.  If another thread is holding the mutex, this function will
    /// block until the mutex is released.  Waiting threads are woken up in order of priority.  0
    /// is the highest priority, 1 is second-highest, etc.
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

pub struct MutexGuard<'a, T: 'a> {
    __inner: simple::MutexGuard<'a, Inner<T>>,
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    /// Release the lock.  If any threads are ready to take the mutex (ie. are currently blocked
    /// calling `lock`), then the one with the highest priority will receive it; if not, the mutex
    /// will just be freed.  This function performs a syscall.  On my machine it takes ~3 μs.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::*;

    #[test]
    fn test() {
        let h = Mutex::new(vec![]);
        let mut tids = vec![];
        for i in 0..5 {
            let h = h.clone();
            tids.push(thread::spawn(move|| {
                let mut x = h.lock(10-i);
                thread::sleep(Duration::from_millis(10));
                x.push(i);
            }));
        }
        for tid in tids { tid.join().unwrap(); }
        println!("{:?}", *h.lock(9));
    }

    #[test]
    // Check that the releasing thread doesn't have an unfair advantage in re-taking
    fn test_no_unfair_advantage() {
        let m1 = Mutex::new(0);
        let m2 = m1.clone();
        {
            let mut g = m1.lock(0);   // thread 1 takes the lock first
            thread::spawn(move|| {       // thread 2 simply:
                let mut g = m2.lock(0);  // waits for the lock...
                thread::sleep(Duration::from_millis(1000));  // and holds it forever (effectively)
                *g += 1;
            });
            thread::sleep(Duration::from_millis(500)); // let thread 2 fully go to sleep
            *g += 1;
        } // now release... and immediately try to re-acquire
        for _ in 0..100 {
            if m1.try_lock().is_some() {
                panic!("try_lock succeeded when there was a thread waiting!");
            }
        }
    }
}
