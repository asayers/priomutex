/*!
A mutex which allows waiting threads to specify a priority.
*/

use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};

mod with_prio; use with_prio::*;
mod simple;
#[cfg(test)] mod bench;

/// A mutex which allows waiting threads to specify a priority.
pub struct Mutex<T> {
    inner: Arc<simple::Mutex<Inner<T>>>,
    tx: mpsc::Sender<WithPrio<Thread>>,
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
            if let Some(inner) = self.inner.try_lock() {
                // we took it!
                return MutexGuard {
                    __inner: inner,
                };
            } else {
                let me = WithPrio { prio: prio, inner: thread::current() };
                self.tx.send(me).unwrap();
                thread::park();
            }
        }
    }
}

unsafe impl<T: Send> Send for Mutex<T> { }

pub struct MutexGuard<'a, T: 'a> {
    __inner: simple::MutexGuard<'a, Inner<T>>,
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    /// Release the lock.  If any threads are ready to take the mutex (ie. are currently blocked
    /// calling `lock`), then the one with the highest priority will receive it; if not, the mutex
    /// will just be freed.  This function performs a syscall.  On my machine it takes 3-4 μs.
    fn drop(&mut self) {
        // Release the lock first, and *then* wake the next thread.  If we rely on the Drop impl
        // for simple::MutexGuard then these operations happen in the reverse order, which can lead
        // to a deadlock.
        let next_thread = self.__inner.next_thread();
        self.__inner.release();
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
}
