/*!
A mutex which allows waiting threads to specify a priority.
*/

use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};

mod with_prio; use with_prio::*;
mod simple;

/// A mutex which allows waiting threads to specify a priority.
#[derive(Clone)]
pub struct PrioMutex<T> {
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

impl<T> PrioMutex<T> {
    /// Create a new prio-mutex.
    pub fn new(data: T) -> PrioMutex<T> {
        let (tx, rx) = mpsc::channel();
        PrioMutex {
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
    pub fn lock(&self, prio: usize) -> PrioMutexGuard<T> {
        loop {
            if let Some(inner) = self.inner.try_lock() {
                // we took it!
                return PrioMutexGuard {
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

unsafe impl<T: Send> Send for PrioMutex<T> { }

pub struct PrioMutexGuard<'a, T: 'a> {
    __inner: simple::MutexGuard<'a, Inner<T>>,
}

impl<'a, T> Drop for PrioMutexGuard<'a, T> {
    /// Release the lock.  If any threads are ready to take the mutex (ie. are currently blocked
    /// calling `lock`), then the one with the highest priority will receive it; if not, the mutex
    /// will just be freed.  This function performs a syscall.  On my machine it takes ~2.5 us.
    fn drop(&mut self) {
        let next_thread = self.__inner.next_thread();
        if let Some(h) = next_thread {
            h.unpark();
        }
    }
}

impl<'a, T> Deref for PrioMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &(*self.__inner).data
    }
}

impl<'a, T> DerefMut for PrioMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut (*self.__inner).data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    // use std::time::*;
    // use std::mem;

    #[test]
    fn test() {
        let h = PrioMutex::new(vec![]);
        let mut tids = vec![];
        for i in 0..5 {
            let h = h.clone();
            tids.push(thread::spawn(move|| {
                let mut x = h.lock(10-i);
                thread::sleep_ms(10);
                x.push(i);
            }));
        }
        for tid in tids { tid.join().unwrap(); }
        println!("{:?}", *h.lock(9));
    }

    /*
    #[test]
    fn test_thread_pool_free() {
        let mutex = PrioMutex::new(0u8);
        let h1 = mutex.clone();
        let h2 = mutex.clone();
        thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
        thread::spawn(move|| { let guard = h2.lock(1).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_thread_pool_neq() {
        let mutex = PrioMutex::new(0u8);
        let h1 = mutex.clone();
        let h2 = mutex.clone();
        thread::spawn(move|| { let guard = h2.lock(1).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
        thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
        thread::sleep(Duration::from_millis(10));
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_thread_pool_eq() {
        let mutex = PrioMutex::new(0u8);
        let h1 = mutex.clone();
        let h2 = mutex.clone();
        thread::spawn(move|| { let guard = h2.lock(0).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
        thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_thread_pool_1() {
        let mutex = PrioMutex::new(0u8);
        let x = mutex.lock(0).unwrap();
        x.release();
        let x = mutex.lock(1).unwrap();
        x.release();
        let x = mutex.lock(2).unwrap();
        x.release();
        let x = mutex.lock(3).unwrap();
        x.release();
        let x = mutex.lock(4).unwrap();
        x.release();
    }

    #[test]
    fn test_thread_pool() {
        let mutex = PrioMutex::new((Instant::now(), vec![]));
        let mut guard = mutex.lock(9).unwrap();
        let mut threads = vec![];
        for thread_num in 1..4 {
            let mutex = mutex.clone();
            threads.push(thread::spawn(move||{
                for i in 0..(10 * thread_num) {
                    let mut guard = mutex.lock(thread_num).unwrap();
                    guard.inner.1.push(guard.inner.0.elapsed());
                    thread::sleep(Duration::from_millis(3));
                    let ts = Instant::now();
                    guard.inner.0 = ts;
                    guard.release();
                    println!("thread {}, iter {:>2}: releasing took {:>5} ns", thread_num, i, ts.elapsed().subsec_nanos());
                    thread::sleep(Duration::from_millis(5));
                }
            }));
        }
        thread::sleep(Duration::from_millis(10));

        // Let's go!
        guard.inner.0 = Instant::now();
        guard.release();
        for i in 0..30 {
            let mut guard = mutex.lock(9).unwrap();
            guard.inner.1.push(guard.inner.0.elapsed());
            thread::sleep(Duration::from_millis(3));
            let ts = Instant::now();
            guard.inner.0 = ts;
            guard.release();
            println!("thread 9, iter {:>2}: releasing took {:>5} ns", i, ts.elapsed().subsec_nanos());
        }
        for t in threads {
            t.join().unwrap();
        }
        let guard = mutex.lock(0).unwrap();
        println!("{:?}", guard.inner);
    }
    */
}
