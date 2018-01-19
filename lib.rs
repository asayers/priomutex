/*!
A baton encapsulates some state which may be passed around between the threads in a thread pool.
At any given time, the baton is held by exactly one member of the pool.

Each thread has a "handle" which it keeps hold of at all times.  This handle is used to take the
baton.  Don't call `tag_in` on your handle while holding the baton, or else the pool will be
deadlocked.
*/

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ptr;
use std::sync::atomic::{self, AtomicPtr};
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};

/// A baton encapsulates some state which may be passed around between the threads in a thread
/// pool.
pub struct Baton<T> {
    // When releasing, first check to see if there are any waiting threads to hand the baton to...
    queue_rx: mpsc::Receiver<WithPrio<Thread>>,
    heap: BinaryHeap<WithPrio<Thread>>,
    // ...and if not, free the baton.
    slot: Arc<AtomicPtr<Baton<T>>>,
    pub inner: T,
}

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

/// Allows taking the baton with high priority.
pub struct Handle<T> {
    // When locking, check this first to see if the baton is free...
    slot: Arc<AtomicPtr<Baton<T>>>,
    // ...and if not, send a work handle to the active thread.
    queue_tx: mpsc::Sender<WithPrio<Thread>>,
}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle {
            slot: self.slot.clone(),
            queue_tx: self.queue_tx.clone(),
        }
    }
}

unsafe impl<T: Send> Send for Handle<T> {}

impl<T> Baton<T> {
    /// Create a new baton.
    ///
    /// You also get a `Handle`, which may be cloned.  Threads may be added to or removed
    /// from the pool freely.
    pub fn new(inner: T) -> (Baton<T>, Handle<T>) {
        let (queue_tx, queue_rx) = mpsc::channel();
        let slot = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let baton = Baton {
            queue_rx: queue_rx,
            heap: BinaryHeap::new(),
            slot: slot.clone(),
            inner: inner,
        };
        let handle = Handle {
            slot: slot,
            queue_tx: queue_tx,
        };
        (baton, handle)
    }

    /// Pass the baton to another thread.
    ///
    /// If any threads are ready to take baton (are currently blocked on `tag_in`), then the one
    /// which "tagged in" with the highest priority will receive it; if not, the baton will be
    /// freed.
    ///
    /// This function performs a syscall.  On my machine it takes ~1.7 us.
    pub fn tag_out(mut self) {
        // Drain the queue into the heap
        for x in self.queue_rx.try_iter() {
            self.heap.push(x);
        }
        // And take the max priority element
        let next_thread = self.heap.pop();

        // Free the baton
        let slot = self.slot.clone();
        let baton = Box::new(self);
        let old = slot.compare_and_swap(ptr::null_mut(),
                                       Box::into_raw(baton),
                                       atomic::Ordering::Relaxed); // TODO: is Relaxed ok?
        assert!(old == ptr::null_mut(), "Impossible: slot wasn't null!");

        if let Some(h) = next_thread {
            // We have another thread waiting to take over. Let's wake it up.
            h.inner.unpark();
        }
    }
}

impl<T> Handle<T> {
    /// Declare that this thread is ready to receive the baton.
    ///
    /// Blocks until the baton is passed to it.  This function allocates and performs a syscall.
    //
    // TODO: Consume self, and return the handle when calling tag_out, in order to prevent
    // deadlocks.
    pub fn tag_in(&self, prio: usize) -> Result<Baton<T>, Error> {
        loop {
            // First check if the baton is free
            let x = self.slot.swap(ptr::null_mut(), atomic::Ordering::Relaxed); // TODO: Relaxed?
            if x != ptr::null_mut() {
                // It's free!
                let baton = unsafe { Box::from_raw(x) };
                return Ok(*baton)
            } else {
                // The baton is currently held by another thread.  Let's create a handle for waking
                // this thread, and send it to the active thread.
                let worker_h = WithPrio{ prio: prio, inner: thread::current() };
                match self.queue_tx.send(worker_h) {
                    Ok(()) => { /* ok! */ }
                    Err(mpsc::SendError(_)) => return Err(Error::BatonIsGone),
                }
                // And then wait for another thread to wake us up.
                thread::park();
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// Someone has dropped the baton.
    BatonIsGone,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::*;
    use std::mem;

    #[test]
    fn test_thread_pool_free() {
        let (baton, h) = Baton::new(0u8);
        let h1 = h.clone();
        let h2 = h.clone();
        baton.tag_out();
        thread::spawn(move|| { let baton = h1.tag_in(0).unwrap(); println!("got baton: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(baton); });
        thread::spawn(move|| { let baton = h2.tag_in(1).unwrap(); println!("got baton: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(baton); });
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_thread_pool_neq() {
        let (baton, h) = Baton::new(0u8);
        let h1 = h.clone();
        let h2 = h.clone();
        thread::spawn(move|| { let baton = h2.tag_in(1).unwrap(); println!("got baton: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(baton); });
        thread::spawn(move|| { let baton = h1.tag_in(0).unwrap(); println!("got baton: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(baton); });
        thread::sleep(Duration::from_millis(10));
        baton.tag_out();
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_thread_pool_eq() {
        let (baton, h) = Baton::new(0u8);
        let h1 = h.clone();
        let h2 = h.clone();
        thread::spawn(move|| { let baton = h2.tag_in(0).unwrap(); println!("got baton: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(baton); });
        thread::spawn(move|| { let baton = h1.tag_in(0).unwrap(); println!("got baton: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(baton); });
        baton.tag_out();
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_thread_pool_1() {
        let (baton, h) = Baton::new(0u8);
        baton.tag_out();
        let baton = h.tag_in(0).unwrap();
        baton.tag_out();
        let baton = h.tag_in(1).unwrap();
        baton.tag_out();
        let baton = h.tag_in(2).unwrap();
        baton.tag_out();
        let baton = h.tag_in(3).unwrap();
        baton.tag_out();
        let baton = h.tag_in(4).unwrap();
        baton.tag_out();
    }

    #[test]
    fn test_thread_pool() {
        let (mut baton, h) = Baton::new((Instant::now(), vec![]));
        let mut threads = vec![];
        for thread_num in 1..4 {
            let h = h.clone();
            threads.push(thread::spawn(move||{
                for i in 0..(10 * thread_num) {
                    let mut baton = h.tag_in(thread_num).unwrap();
                    baton.inner.1.push(baton.inner.0.elapsed());
                    thread::sleep(Duration::from_millis(3));
                    let ts = Instant::now();
                    baton.inner.0 = ts;
                    baton.tag_out();
                    println!("thread {}, iter {:>2}: releasing took {:>5} ns", thread_num, i, ts.elapsed().subsec_nanos());
                    thread::sleep(Duration::from_millis(5));
                }
            }));
        }
        thread::sleep(Duration::from_millis(10));

        // Let's go!
        baton.inner.0 = Instant::now();
        baton.tag_out();
        for i in 0..30 {
            let mut baton = h.tag_in(9).unwrap();
            baton.inner.1.push(baton.inner.0.elapsed());
            thread::sleep(Duration::from_millis(3));
            let ts = Instant::now();
            baton.inner.0 = ts;
            baton.tag_out();
            println!("thread 9, iter {:>2}: releasing took {:>5} ns", i, ts.elapsed().subsec_nanos());
        }
        for t in threads {
            t.join().unwrap();
        }
        let baton = h.tag_in(0).unwrap();
        println!("{:?}", baton.inner);
    }
}
