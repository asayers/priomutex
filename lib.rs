/*!
A mutex which allows waiting threads to specify a priority.
*/

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ptr;
use std::sync::atomic::{self, AtomicPtr};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};

/// A mutex which allows waiting threads to specify a priority.
pub struct PrioMutex<T> {
    // When locking, check this first to see if the guard is free...
    slot: Arc<AtomicPtr<PrioMutexGuard<T>>>,
    // ...and if not, send a work handle to the active thread.
    queue_tx: mpsc::Sender<WithPrio<Thread>>,
}

impl<T> Clone for PrioMutex<T> {
    fn clone(&self) -> Self {
        PrioMutex {
            slot: self.slot.clone(),
            queue_tx: self.queue_tx.clone(),
        }
    }
}

unsafe impl<T: Send> Send for PrioMutex<T> {}

impl<T> PrioMutex<T> {
    /// Create a new prio-mutex.
    pub fn new(inner: T) -> PrioMutex<T> {
        let (queue_tx, queue_rx) = mpsc::channel();
        let slot = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let guard = PrioMutexGuard {
            queue_rx: queue_rx,
            heap: BinaryHeap::new(),
            slot: slot.clone(),
            inner: inner,
        };
        guard.release();
        PrioMutex {
            slot: slot,
            queue_tx: queue_tx,
        }
    }

    /// Attempt to take the mutex.  If another thread is holding the mutex, this function will
    /// block until the mutex is released.  Waiting threads are woken up in order of priority.  0
    /// is the highest priority, 1 is second-highest, etc.
    pub fn lock(&self, prio: usize) -> Result<PrioMutexGuard<T>, Error> {
        loop {
            // First check if the guard is free
            let x = self.slot.swap(ptr::null_mut(), atomic::Ordering::Relaxed); // TODO: Relaxed?
            if x != ptr::null_mut() {
                // It's free!
                let guard = unsafe { Box::from_raw(x) }; // TODO: Is this necessary?
                return Ok(*guard)
            } else {
                // The guard is currently held by another thread.  Let's create a handle for waking
                // this thread, and send it to the active thread.
                let worker_h = WithPrio{ prio: prio, inner: thread::current() };
                match self.queue_tx.send(worker_h) {
                    Ok(()) => { /* ok! */ }
                    Err(mpsc::SendError(_)) => return Err(Error::GuardIsGone),
                }
                // And then wait for another thread to wake us up.
                thread::park();
            }
        }
    }
}

/// A guard encapsulates some state which may be passed around between threads.
pub struct PrioMutexGuard<T> {
    // When releasing, first check to see if there are any waiting threads to hand the guard to...
    queue_rx: mpsc::Receiver<WithPrio<Thread>>,
    heap: BinaryHeap<WithPrio<Thread>>,
    // ...and if not, free the guard.
    slot: Arc<AtomicPtr<PrioMutexGuard<T>>>,
    pub inner: T,
}

impl<T> PrioMutexGuard<T> {
    /// Release the guard.
    ///
    /// If any threads are ready to take the mutex (ie. are currently blocked calling `lock`), then
    /// the one with the highest priority will receive it; if not, the mutex will just be freed.
    ///
    /// This function performs a syscall.  On my machine it takes ~2.5 us.
    pub fn release(mut self) {
        // Drain the queue into the heap
        for x in self.queue_rx.try_iter() {
            self.heap.push(x);
        }
        // And take the max priority element
        let next_thread = self.heap.pop();

        // Free the guard
        let slot = self.slot.clone();
        let guard = Box::new(self);
        let old = slot.compare_and_swap(ptr::null_mut(),
                                       Box::into_raw(guard),
                                       atomic::Ordering::Relaxed); // TODO: is Relaxed ok?
        assert!(old == ptr::null_mut(), "Impossible: slot wasn't null!");

        // If there's another thread waiting to take over, wake it up.
        if let Some(h) = next_thread {
            h.inner.unpark();
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// Someone has dropped the guard.
    GuardIsGone,
}

#[derive(Debug)]
struct WithPrio<T> {
    prio: usize,
    inner: T,
}
impl<T> PartialEq for WithPrio<T> {
    fn eq(&self, other: &WithPrio<T>) -> bool { other.prio == self.prio }
}
impl<T> Eq for WithPrio<T> {}
impl<T> PartialOrd for WithPrio<T> {
    fn partial_cmp(&self, other: &WithPrio<T>) -> Option<Ordering> { other.prio.partial_cmp(&self.prio) }
}
impl<T> Ord for WithPrio<T> {
    fn cmp(&self, other: &WithPrio<T>) -> Ordering { other.prio.cmp(&self.prio) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::*;
    use std::mem;

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
}
